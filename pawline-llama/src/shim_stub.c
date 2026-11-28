/* Stub used when the crate is built without the "vendored" feature: the engine
   reports itself unavailable and every call fails cleanly. */
#include <stddef.h>

typedef struct pl_engine pl_engine;

int pl_available(void) { return 0; }
const char * pl_engine_version(void) { return "no vendored llama.cpp in this build"; }
pl_engine * pl_create(const char * model_path, int n_ctx, int n_threads, char * err, int err_len) {
    (void)model_path; (void)n_ctx; (void)n_threads;
    if (err && err_len > 0) { err[0] = 'n'; err[1] = 'o'; err[2] = 0; }
    return NULL;
}
int pl_generate(pl_engine * e, const char * prompt, int max_tokens, int top_k, float top_p, float temp, char * out, int out_len) {
    (void)e; (void)prompt; (void)max_tokens; (void)top_k; (void)top_p; (void)temp; (void)out; (void)out_len;
    return -1;
}
int pl_apply_template(pl_engine * e, const char * system, const char * user, char * out, int out_len) {
    (void)e; (void)system; (void)user; (void)out; (void)out_len;
    return -1;
}
void pl_destroy(pl_engine * e) { (void)e; }
