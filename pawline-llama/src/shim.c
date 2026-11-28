/* Small C surface over llama.cpp: the Rust side only sees scalars and char
   buffers, so no llama struct layout is mirrored in Rust.

   Built against the vendored llama.cpp headers; see build.rs. */
#include "llama.h"
#include "mtmd.h"
#include "mtmd-helper.h"

#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

struct pl_engine {
    struct llama_model * model;
    struct llama_context * ctx;
    const struct llama_vocab * vocab;
    int n_ctx;
    /* Prefix reuse: the tokens currently in the KV cache, so a question that
       shares a prefix with the previous one (the same screenshot, the same
       system prompt) does not pay to decode that prefix again. */
    llama_token * cached;
    int32_t n_cached;
    int n_threads;
    /* Set when a projector was loaded: the vision path runs through mtmd on
       this same model and context, no server. */
    struct mtmd_context * mctx;
    /* The last picture seen, by the sha-256 id mtmd gives the bitmap, and the
       position in the context just after its image tokens. A second question
       about the same picture then keeps the expensive encode and only decodes
       the new words. */
    char last_image_id[128];
    llama_pos n_past_after_image;
};

int pl_available(void) { return 1; }

const char * pl_engine_version(void) { return "llama.cpp b12811 (MIT)"; }

static void set_err(char * err, int err_len, const char * msg) {
    if (err && err_len > 0) {
        snprintf(err, (size_t) err_len, "%s", msg);
    }
}

struct pl_engine * pl_create(const char * model_path, int n_ctx, int n_threads, char * err, int err_len) {
    if (err && err_len > 0) err[0] = 0;
    llama_backend_init();

    struct llama_model_params mp = llama_model_default_params();
    mp.n_gpu_layers = 0;
    struct llama_model * model = llama_model_load_from_file(model_path, mp);
    if (!model) {
        set_err(err, err_len, "could not load the model file");
        return NULL;
    }

    struct llama_context_params cp = llama_context_default_params();
    cp.n_ctx = n_ctx > 0 ? (uint32_t) n_ctx : 2048;
    cp.n_threads = n_threads > 0 ? (int32_t) n_threads : 1;
    cp.n_threads_batch = cp.n_threads;
    struct llama_context * ctx = llama_init_from_model(model, cp);
    if (!ctx) {
        llama_model_free(model);
        set_err(err, err_len, "could not create a context");
        return NULL;
    }

    struct pl_engine * e = calloc(1, sizeof(*e));
    if (!e) {
        llama_free(ctx);
        llama_model_free(model);
        set_err(err, err_len, "out of memory");
        return NULL;
    }
    e->model = model;
    e->ctx = ctx;
    e->vocab = llama_model_get_vocab(model);
    e->n_ctx = (int) cp.n_ctx;
    e->n_threads = cp.n_threads;
    e->cached = malloc(sizeof(llama_token) * (size_t) cp.n_ctx);
    e->n_cached = 0;
    return e;
}

/* Wrap a system + user turn in the model's own chat template. Returns the
   number of bytes written, or -1 when no template is available. */
int pl_apply_template(struct pl_engine * e, const char * system, const char * user, char * out, int out_len) {
    if (!e || !out || out_len <= 0) return -1;
    const char * tmpl = llama_model_chat_template(e->model, NULL);
    if (!tmpl) return -1;
    struct llama_chat_message msgs[2];
    size_t n = 0;
    if (system && system[0]) {
        msgs[n].role = "system";
        msgs[n].content = system;
        n++;
    }
    msgs[n].role = "user";
    msgs[n].content = user ? user : "";
    n++;
    int32_t need = llama_chat_apply_template(tmpl, msgs, n, true, out, out_len);
    if (need < 0 || need > out_len) return -1;
    return (int) need;
}

static struct llama_sampler * make_sampler(int top_k, float top_p, float temp) {
    struct llama_sampler_chain_params sp = llama_sampler_chain_default_params();
    sp.no_perf = true;
    struct llama_sampler * smpl = llama_sampler_chain_init(sp);
    if (temp <= 0.0f) {
        llama_sampler_chain_add(smpl, llama_sampler_init_greedy());
        return smpl;
    }
    if (top_k > 0) llama_sampler_chain_add(smpl, llama_sampler_init_top_k(top_k));
    if (top_p > 0.0f && top_p < 1.0f) llama_sampler_chain_add(smpl, llama_sampler_init_top_p(top_p, 1));
    llama_sampler_chain_add(smpl, llama_sampler_init_temp(temp));
    llama_sampler_chain_add(smpl, llama_sampler_init_dist(LLAMA_DEFAULT_SEED));
    return smpl;
}

/* Generate a reply for `prompt` (already chat-templated). Returns the number of
   tokens generated, or -1 on error. `out` always ends up NUL-terminated. */
int pl_generate(struct pl_engine * e, const char * prompt, int max_tokens, int top_k, float top_p, float temp,
                char * out, int out_len) {
    if (!e || !prompt || !out || out_len <= 0) return -1;
    out[0] = 0;

    int32_t n_prompt_max = (int32_t) e->n_ctx;
    llama_token * toks = malloc(sizeof(llama_token) * (size_t) n_prompt_max);
    if (!toks) return -1;
    int32_t n_toks = llama_tokenize(e->vocab, prompt, (int32_t) strlen(prompt), toks, n_prompt_max, true, true);
    if (n_toks < 0 || n_toks >= e->n_ctx) {
        free(toks);
        snprintf(out, (size_t) out_len, "prompt does not fit the context");
        return -1;
    }

    /* How much of this prompt is already in the cache? Always leave at least
       one token to decode so the context has fresh logits. */
    int32_t keep = 0;
    int32_t max_keep = n_toks - 1;
    if (max_keep > e->n_cached) max_keep = e->n_cached;
    while (keep < max_keep && e->cached[keep] == toks[keep]) keep++;

    llama_memory_t mem = llama_get_memory(e->ctx);
    if (keep == 0) {
        llama_memory_clear(mem, true);
    } else if (keep < e->n_cached) {
        llama_memory_seq_rm(mem, 0, keep, -1);
    }
    e->n_cached = keep;

    if (llama_decode(e->ctx, llama_batch_get_one(toks + keep, n_toks - keep)) != 0) {
        free(toks);
        e->n_cached = 0;
        llama_memory_clear(mem, true);
        snprintf(out, (size_t) out_len, "decode of the prompt failed");
        return -1;
    }
    memcpy(e->cached, toks, sizeof(llama_token) * (size_t) n_toks);
    e->n_cached = n_toks;
    free(toks);

    struct llama_sampler * smpl = make_sampler(top_k, top_p, temp);
    int used = 0;
    int generated = 0;
    char piece[256];

    while (generated < max_tokens) {
        llama_token id = llama_sampler_sample(smpl, e->ctx, -1);
        if (llama_vocab_is_eog(e->vocab, id)) break;
        llama_sampler_accept(smpl, id);
        int32_t n = llama_token_to_piece(e->vocab, id, piece, (int32_t) sizeof(piece), 0, true);
        if (n > 0 && used + n < out_len - 1) {
            memcpy(out + used, piece, (size_t) n);
            used += n;
            out[used] = 0;
        }
        generated++;
        if (e->n_cached < e->n_ctx) {
            e->cached[e->n_cached++] = id;
        }
        if (llama_decode(e->ctx, llama_batch_get_one(&id, 1)) != 0) break;
    }

    llama_sampler_free(smpl);
    /* The cache stays: the next question reuses whatever prefix it shares with
       this one. It is trimmed, not cleared, when a prompt diverges. */
    return generated;
}


/* Load the mmproj projector into this engine. After this the engine can answer
   questions about an image. Returns 0 on success, -1 on failure. */
int pl_vision_enable(struct pl_engine * e, const char * mmproj, char * err, int err_len) {
    if (!e || !mmproj) return -1;
    if (e->mctx) return 0;
    struct mtmd_context_params p = mtmd_context_params_default();
    p.use_gpu = false;
    p.print_timings = false;
    p.n_threads = e->n_threads;
    p.warmup = false;
    e->mctx = mtmd_init_from_file(mmproj, e->model, p);
    if (!e->mctx) {
        set_err(err, err_len, "could not load the projector (mmproj) file");
        return -1;
    }
    if (!mtmd_support_vision(e->mctx)) {
        mtmd_free(e->mctx);
        e->mctx = NULL;
        set_err(err, err_len, "that projector has no vision support");
        return -1;
    }
    return 0;
}

int pl_vision_ready(struct pl_engine * e) { return (e && e->mctx) ? 1 : 0; }

/* Ask one question about one image (PNG or JPEG bytes in memory). The prompt is
   built with the model's own chat template around the media marker, tokenised
   by mtmd, evaluated chunk by chunk, then sampled like any other answer. */
int pl_vision_ask(struct pl_engine * e, const unsigned char * img, size_t img_len,
                  const char * question, int max_tokens, int top_k, float top_p, float temp,
                  char * out, int out_len) {
    if (!e || !e->mctx || !img || img_len == 0 || !out || out_len <= 0) return -1;
    out[0] = 0;

    struct mtmd_helper_init_opt opt = mtmd_helper_init_opt_default();
    struct mtmd_helper_bitmap_wrapper wrap = mtmd_helper_bitmap_init_from_buf(e->mctx, img, img_len, false, opt);
    if (!wrap.bitmap) {
        snprintf(out, (size_t) out_len, "could not read the screenshot image");
        return -1;
    }

    char content[8192];
    snprintf(content, sizeof(content), "%s\n%s", mtmd_default_marker(), question ? question : "");
    char prompt[16384];
    int n_prompt = pl_apply_template(e, NULL, content, prompt, (int) sizeof(prompt));
    if (n_prompt < 0) {
        snprintf(prompt, sizeof(prompt), "%s", content);
        n_prompt = (int) strlen(prompt);
    }

    struct mtmd_input_text in_text;
    in_text.text = prompt;
    in_text.text_len = (size_t) n_prompt;
    in_text.add_special = true;
    in_text.parse_special = true;

    mtmd_input_chunks * chunks = mtmd_input_chunks_init();
    const mtmd_bitmap * bitmaps[1] = { wrap.bitmap };
    int32_t rc = mtmd_tokenize(e->mctx, chunks, &in_text, bitmaps, 1);
    if (rc != 0) {
        mtmd_input_chunks_free(chunks);
        mtmd_bitmap_free(wrap.bitmap);
        snprintf(out, (size_t) out_len, "could not prepare the image for the model");
        return -1;
    }

    /* Where do the image tokens end? Everything before that is the same for
       every question about this picture, so it can stay in the cache. */
    size_t n_chunks = mtmd_input_chunks_size(chunks);
    size_t last_image_chunk = (size_t) -1;
    for (size_t i = 0; i < n_chunks; i++) {
        const mtmd_input_chunk * c = mtmd_input_chunks_get(chunks, i);
        if (mtmd_input_chunk_get_type(c) == MTMD_INPUT_CHUNK_TYPE_IMAGE) {
            last_image_chunk = i;
        }
    }
    llama_pos pos_after_image = 0;
    for (size_t i = 0; i <= last_image_chunk && i < n_chunks; i++) {
        pos_after_image += (llama_pos) mtmd_input_chunk_get_n_tokens(mtmd_input_chunks_get(chunks, i));
    }

    char image_id[128];
    image_id[0] = 0;
    const char * raw_id = mtmd_bitmap_get_id(wrap.bitmap);
    if (raw_id) snprintf(image_id, sizeof(image_id), "%s", raw_id);

    int reuse = image_id[0] && e->n_past_after_image > 0 &&
                last_image_chunk != (size_t) -1 &&
                pos_after_image == e->n_past_after_image &&
                strncmp(image_id, e->last_image_id, sizeof(e->last_image_id) - 1) == 0;

    llama_memory_t mem = llama_get_memory(e->ctx);
    e->n_cached = 0;
    llama_pos new_n_past = 0;

    if (getenv("PAWLINE_LLAMA_DEBUG")) {
        fprintf(stderr, "pl_vision: chunks=%zu last_image=%d pos_after_image=%d prev=%d id=%s prev_id=%s reuse=%d\n",
                n_chunks, (int) last_image_chunk, (int) pos_after_image,
                (int) e->n_past_after_image, image_id[0] ? image_id : "(none)", e->last_image_id, reuse);
        for (size_t i = 0; i < n_chunks; i++) {
            const mtmd_input_chunk * c = mtmd_input_chunks_get(chunks, i);
            fprintf(stderr, "  chunk %zu type=%d n_tokens=%zu\n", i, (int) mtmd_input_chunk_get_type(c),
                    mtmd_input_chunk_get_n_tokens(c));
        }
    }
    if (reuse) {
        /* Same picture: keep the image encode, drop the old question's words. */
        llama_memory_seq_rm(mem, 0, pos_after_image, -1);
        for (size_t i = last_image_chunk + 1; i < n_chunks; i++) {
            const mtmd_input_chunk * c = mtmd_input_chunks_get(chunks, i);
            size_t n_tokens = 0;
            const llama_token * toks = mtmd_input_chunk_get_tokens_text(c, &n_tokens);
            if (!toks || n_tokens == 0) continue;
            if (llama_decode(e->ctx, llama_batch_get_one((llama_token *) toks, (int32_t) n_tokens)) != 0) {
                rc = -1;
                break;
            }
            new_n_past += (llama_pos) n_tokens;
            rc = 0;
        }
        if (getenv("PAWLINE_LLAMA_DEBUG")) {
            fprintf(stderr, "pl_vision: reuse rc=%d new_n_past=%d\n", (int) rc, (int) new_n_past);
        }
    } else {
        llama_memory_clear(mem, true);
        rc = mtmd_helper_eval_chunks(e->mctx, e->ctx, chunks, 0, 0, (int32_t) e->n_ctx, true, &new_n_past);
    }

    mtmd_input_chunks_free(chunks);
    mtmd_bitmap_free(wrap.bitmap);
    if (rc != 0) {
        e->n_past_after_image = 0;
        e->last_image_id[0] = 0;
        snprintf(out, (size_t) out_len, "the model could not read the image");
        return -1;
    }

    if (image_id[0]) {
        snprintf(e->last_image_id, sizeof(e->last_image_id), "%s", image_id);
        e->n_past_after_image = pos_after_image;
    }

    struct llama_sampler * smpl = make_sampler(top_k, top_p, temp);
    int used = 0;
    int generated = 0;
    char piece[256];
    while (generated < max_tokens) {
        llama_token id = llama_sampler_sample(smpl, e->ctx, -1);
        if (llama_vocab_is_eog(e->vocab, id)) break;
        llama_sampler_accept(smpl, id);
        int32_t n = llama_token_to_piece(e->vocab, id, piece, (int32_t) sizeof(piece), 0, true);
        if (n > 0 && used + n < out_len - 1) {
            memcpy(out + used, piece, (size_t) n);
            used += n;
            out[used] = 0;
        }
        generated++;
        if (llama_decode(e->ctx, llama_batch_get_one(&id, 1)) != 0) break;
    }
    llama_sampler_free(smpl);
    return generated;
}

void pl_destroy(struct pl_engine * e) {
    if (!e) return;
    if (e->mctx) mtmd_free(e->mctx);
    free(e->cached);
    if (e->ctx) llama_free(e->ctx);
    if (e->model) llama_model_free(e->model);
    free(e);
}
