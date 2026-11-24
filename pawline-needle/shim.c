#include <stddef.h>
#include <stdint.h>
/* Legacy libc++ 64-bit murmur2 __hash_memory, hidden from newer libc++ exports.
   Provided so the prebuilt libcactus.a links on modern toolchains. */
size_t hashmem_shim(const void* ptr, size_t len) __asm__("_ZNSt3__113__hash_memoryEPKvm");
size_t hashmem_shim(const void* ptr, size_t len) {
    const size_t m = 0xc6a4a7935bd1e995ULL; const int r = 47;
    const unsigned char* buf = (const unsigned char*)ptr;
    size_t h = 0xc70f6907UL ^ (len * m);
    const unsigned char* end = buf + (len & ~(size_t)7);
    for (; buf != end; buf += 8) {
        size_t k; __builtin_memcpy(&k, buf, 8);
        k *= m; k ^= k >> r; k *= m; h ^= k; h *= m;
    }
    switch (len & 7) {
    case 7: h ^= (size_t)buf[6] << 48;
    case 6: h ^= (size_t)buf[5] << 40;
    case 5: h ^= (size_t)buf[4] << 32;
    case 4: h ^= (size_t)buf[3] << 24;
    case 3: h ^= (size_t)buf[2] << 16;
    case 2: h ^= (size_t)buf[1] << 8;
    case 1: h ^= (size_t)buf[0]; h *= m;
    }
    h ^= h >> r; h *= m; h ^= h >> r;
    return h;
}
