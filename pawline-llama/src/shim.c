/* Small C surface over llama.cpp: the Rust side only sees scalars and char
   buffers, so no llama struct layout is mirrored in Rust.

   Built against the vendored llama.cpp headers; see build.rs. */
#include "llama.h"

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

void pl_destroy(struct pl_engine * e) {
    if (!e) return;
    free(e->cached);
    if (e->ctx) llama_free(e->ctx);
    if (e->model) llama_model_free(e->model);
    free(e);
}
