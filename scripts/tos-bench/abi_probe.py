#!/usr/bin/env python3
"""How many integer and float arguments each calling convention passes in registers, per target.

    abi_probe.py <work dir> [targets...]

Writes a no_std probe crate into <work dir>: for each ABI and each k < 24, a function taking 24
u64 (or f64) arguments that returns argument k. Compiles it for each target with nightly-2026-07-05
and -Z build-std=core (no target std needed), and counts the leading arguments whose function
needs no load: those arrive in registers. MergeFunctions is off so identical probes stay apart.
"""
import glob, os, re, subprocess, sys

N = 24
ABIS = [('rust', 'Rust'), ('sysv64', 'sysv64'), ('win64', 'win64'), ('pnone', 'rust-preserve-none')]
TARGETS = sys.argv[2:] or ['aarch64-apple-darwin', 'arm64_32-apple-watchos', 'aarch64-linux-android',
                           'aarch64-pc-windows-msvc', 'x86_64-apple-darwin', 'x86_64-unknown-linux-gnu',
                           'x86_64-pc-windows-msvc']
work = os.path.abspath(sys.argv[1])
os.makedirs(f'{work}/src', exist_ok=True)
open(f'{work}/Cargo.toml', 'w').write('[package]\nname = "abi-probe"\nversion = "0.1.0"\nedition = "2024"\n'
                                       '[lib]\npath = "src/lib.rs"\n[profile.release]\nopt-level = 3\npanic = "abort"\n'
                                       'codegen-units = 1\n[workspace]\n')
ints = ', '.join(f'a{i}: u64' for i in range(N))
floats = ', '.join(f'f{i}: f64' for i in range(N))
lines = ['#![no_std]', '#![feature(rust_preserve_none_cc)]', '#![allow(unused)]']
for tag, abi in ABIS:
    cfg = '#[cfg(target_arch = "x86_64")] ' if abi in ('sysv64', 'win64') else ''
    for k in range(N):
        lines.append(f'{cfg}#[inline(never)] #[unsafe(no_mangle)] pub extern "{abi}" fn probe_int_{tag}_{k}({ints}) -> u64 {{ a{k} }}')
        lines.append(f'{cfg}#[inline(never)] #[unsafe(no_mangle)] pub extern "{abi}" fn probe_fp_{tag}_{k}({floats}) -> f64 {{ f{k} }}')
open(f'{work}/src/lib.rs', 'w').write('\n'.join(lines) + '\n')

env = dict(os.environ, RUSTFLAGS='-Z merge-functions=disabled', CARGO_TARGET_DIR=f'{work}/target',
           PATH=os.path.expanduser('~/.rustup/toolchains/nightly-2026-07-05-aarch64-apple-darwin/bin') + ':' + os.environ['PATH'])

def funcs(path):
    out, cur = {}, None
    for line in open(path):
        m = re.match(r'^_?(probe_\w+):', line)
        if m: cur = m.group(1); out[cur] = []; continue
        if cur and re.match(r'^\s*\.(cfi_endproc|seh_endproc)|^\.?Lfunc_end', line): cur = None; continue
        if cur and re.match(r'^\s+[a-z]', line) and not re.match(r'^\s+\.', line): out[cur].append(line.strip())
    return out

print('| target | ' + ' | '.join(a for _, a in ABIS) + ' |')
print('|---|' + '---|' * len(ABIS))
for t in TARGETS:
    subprocess.run(['cargo', 'rustc', '-q', '--release', '--target', t, '-Z', 'build-std=core', '--lib', '--',
                    '--emit', 'asm', '-C', 'llvm-args=-x86-asm-syntax=intel'], cwd=work, env=env, check=True)
    asm = sorted(glob.glob(f'{work}/target/{t}/release/deps/abi_probe*.s'), key=os.path.getmtime)[-1]
    fs = funcs(asm)
    x86 = t.startswith('x86_64')
    cells = []
    for tag, _ in ABIS:
        c = []
        for kind in ('int', 'fp'):
            n = 0
            for k in range(N):
                ins = fs.get(f'probe_{kind}_{tag}_{k}')
                if ins is None: break
                loads = any('[' in l and not l.startswith(('lea', 'push', 'pop')) for l in ins) if x86 else \
                        any(re.match(r'(ldr|ldur|ldp)\s', l) for l in ins)
                if loads: break
                n += 1
            c.append('—' if fs.get(f'probe_{kind}_{tag}_0') is None else (f'{n}+' if n == N else str(n)))
        cells.append(' / '.join(c))
    print(f'| {t} | ' + ' | '.join(cells) + ' |')
