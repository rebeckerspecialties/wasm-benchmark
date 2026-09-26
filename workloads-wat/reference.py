#!/usr/bin/env python3
"""Independent reference results for the Wasm 3.0 feature benchmarks.

Re-computes each benchmark's documented algorithm in plain Python (no wasm
involved) for a given seed, so the harness's expected values are checked
against the algorithm itself, not only against cross-runtime agreement.

Run: python3 workloads-wat/reference.py [seed]    (default seed 7)
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import gen  # noqa: E402  (shares the EH statement stream and extconst trees)

M32 = 0xFFFFFFFF
M64 = 0xFFFFFFFFFFFFFFFF


def s32(v):
    v &= M32
    return v - (1 << 32) if v >> 31 else v


def rotl32(v, n):
    v &= M32
    return ((v << n) | (v >> (32 - n))) & M32


def rotr32(v, n):
    return rotl32(v, 32 - n)


def rotl64(v, n):
    v &= M64
    return ((v << n) | (v >> (64 - n))) & M64


def lcg_bytes(n, s=0x2545F491):
    out = bytearray(n)
    for i in range(n):
        s = (s * 1664525 + 1013904223) & M32
        out[i] = s >> 24
    return out, s


def tailcall_fsm(seed):
    primes = [0x01000193, 0x9E3779B1, 0x85EBCA77, 0xC2B2AE3D,
              0x27D4EB2F, 0x165667B1, 0xD3A2646C, 0xFD7046C5]
    buf, _ = lcg_bytes(65536)
    a, k = seed & M32, 0
    for p in range(65536):
        b = buf[p]
        a = (((a ^ b) * primes[k]) + k + 1) & M32
        k = b & 7
    return s32(a)


def eh_parser(seed):
    stream, _ = gen.gen_statements(4096, 20260922)
    data = stream.encode()
    n = len(data)
    pos = 0

    class Err(Exception):
        pass

    def peek():
        return data[pos] if pos < n else 0

    def expr():
        nonlocal pos
        v = term()
        while peek() == 43:
            pos += 1
            v = (v + term()) & M32
        return v

    def term():
        nonlocal pos
        v = factor()
        while peek() == 42:
            pos += 1
            v = (v * factor()) & M32
        return v

    def factor():
        nonlocal pos
        c = peek()
        if ((c - 48) & M32) < 10:
            pos += 1
            return c - 48
        if c == 40:
            pos += 1
            v = expr()
            if peek() != 41:
                raise Err(pos)
            pos += 1
            return v
        raise Err(pos)

    def statement():
        nonlocal pos
        v = expr()
        if peek() != 59:
            raise Err(pos)
        pos += 1
        return v

    total, errs = seed & M32, 0
    while pos < n:
        try:
            total = (total * 31 + statement()) & M32
        except Err as e:
            total ^= e.args[0]
            errs += 1
            while pos < n:
                if data[pos] == 59:
                    pos += 1
                    break
                pos += 1
    return s32(total + errs * 1000003)


def gc_trees(seed):
    C = 0x9E3779B1
    memo = {}

    def check_tree(d, v):
        # check(make(d, v)) without building the tree
        v &= M32
        if d == 0:
            return (v * C + 1) & M32
        key = (d, v)
        if key in memo:
            return memo[key]
        r = (v * C + check_tree(d - 1, v << 1) + (check_tree(d - 1, (v << 1) | 1) ^ 1)) & M32
        memo[key] = r
        return r

    total = seed & M32
    for d in (4, 6, 8, 10):
        for i in range(1 << (14 - d)):
            total = (total + check_tree(d, i + seed)) & M32
    return s32(total + check_tree(12, 1))


def callref_dispatch(seed):
    fs = [
        lambda x: x * 0x01000193 + 1,
        lambda x: rotl32(x, 5) ^ 0x9E3779B1,
        lambda x: x * 0x85EBCA77 - 3,
        lambda x: (x >> 3) + x * 0x27D4EB2F,
        lambda x: (x * 0xC2B2AE3D & M32) ^ (x >> 16),
        lambda x: rotr32(x, 11) + 0x165667B1,
        lambda x: (x ^ 0xD3A2646C) * 0x1B873593,
        lambda x: rotl32(x, 13) - x * 5,
    ]
    acc = seed & M32
    for _ in range(200000):
        acc = fs[(acc ^ (acc >> 13)) & 7](acc) & M32
    return s32(acc)


def multimem_transform(seed):
    src, s = lcg_bytes(65536)
    lut = []
    for _ in range(256):
        s = (s * 1664525 + 1013904223) & M32
        lut.append(s)
    h = seed & M32
    dst = []
    for i in range(65536):
        h = (rotl32(h ^ lut[src[i]], 5) * 0x9E3779B1) & M32
        dst.append(h)
    total = 0
    for w in dst:
        total = (rotl32(total, 1) + w) & M32
    return s32(total)


def mem64_chase(seed):
    mask = (1 << 22) - 1
    p = seed & mask
    acc = 0
    for _ in range(262144):
        acc = (rotl64(acc, 7) + (p ^ 0x9E3779B97F4A7C15)) & M64
        p = (p * 1103515245 + 12345) & mask
    return s32((acc ^ (acc >> 32)) & M32)


def extconst_init(seed):
    # Rebuild the generator's random trees with the same seed and order.
    import random
    rng = random.Random(20260922)

    def tree(mask, depth):
        if depth == 0:
            return rng.randrange(1 << 16)
        op = rng.choice(["add", "sub", "mul"])
        a = tree(mask, depth - 1)
        b = tree(mask, depth - 1)
        return {"add": a + b, "sub": a - b, "mul": a * b}[op] & mask

    g32 = [tree(M32, 3) for _ in range(2048)]
    g64 = [tree(M64, 3) for _ in range(512)]
    segs = []
    for k in range(256):
        rng.randrange(1, 64)
        segs.append(rng.randrange(1 << 32))
    h, w = seed & M32, 0
    for v in g32:
        h = (rotl32(h, 3) + v) & M32
    for v in g64:
        w = (rotl64(w, 5) + v) & M64
    for word in segs:
        h = rotl32(h, 1) ^ word
    return s32(h + ((w ^ (w >> 32)) & M32))


def relaxed_dot(seed):
    s = (seed ^ 0x2545F491) & M32
    M, K = 64, 256
    A, BT = [], []
    for _ in range(M * K):
        s = (s * 1664525 + 1013904223) & M32
        a = s >> 24
        A.append(a - 256 if a >= 128 else a)
        s = (s * 1664525 + 1013904223) & M32
        BT.append((s >> 24) & 0x7F)
    total = 0
    for i in range(M):
        ai = A[i * K:(i + 1) * K]
        for j in range(M):
            bj = BT[j * K:(j + 1) * K]
            c = sum(x * y for x, y in zip(ai, bj)) & M32
            total = (rotl32(total, 3) + c) & M32
    return s32(total)


def relaxed_madd(seed):
    s = (seed ^ 0x9E3779B9) & M32
    X = []
    for _ in range(16 * 1024):
        s = (s * 1664525 + 1013904223) & M32
        X.append((s >> 24) % 7 - 3)
    coef = []
    for _ in range(9):
        s = (s * 1664525 + 1013904223) & M32
        coef.append((s >> 24) % 9 - 4)
    lanes = [0, 0, 0, 0]
    for i in range(0, len(X), 4):
        for l in range(4):
            x = X[i + l]
            p = coef[8]
            for d in range(7, -1, -1):
                p = p * x + coef[d]
            assert abs(p) < (1 << 24)
            lanes[l] = (lanes[l] * 31 + p) & M32
    return s32(lanes[0] ^ rotl32(lanes[1], 8) ^ rotl32(lanes[2], 16) ^ rotl32(lanes[3], 24))


BENCHMARKS = [
    ("tailcall_fsm", tailcall_fsm),
    ("eh_parser_exnref", eh_parser),
    ("eh_parser_legacy", eh_parser),
    ("gc_trees", gc_trees),
    ("callref_dispatch", callref_dispatch),
    ("relaxed_dot", relaxed_dot),
    ("relaxed_madd", relaxed_madd),
    ("mem64_chase", mem64_chase),
    ("multimem_transform", multimem_transform),
    ("extconst_init", extconst_init),
]

if __name__ == "__main__":
    seed = int(sys.argv[1], 0) if len(sys.argv) > 1 else 7
    for name, fn in BENCHMARKS:
        print(f"{name:20} {fn(seed)}")
