#!/usr/bin/env python3
# Generates src/main.rs: N_MAX distinct copies of each tinywasm-like handler kind, dispatched through
# tables with guaranteed tail calls, like tinywasm's nightly-tail-calls dispatch.
N = 512
out = []
w = out.append
w('''#![feature(explicit_tail_calls, core_intrinsics)]
#![allow(incomplete_features, internal_features)]
//! Memory-dependence microbenchmark: K distinct copies of the same handler code, dispatched in a
//! cyclic or random order. See gen.py.
use std::time::Instant;

pub struct Stack { data: Vec<u32> }
pub struct Store { stack: Stack, side: Vec<u32> }
pub struct Exec<'a> { store: &'a mut Store }

type H = fn(&mut Exec<'_>, &[u16], usize, u64) -> u64;

unsafe extern "C" {
    fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut u64) -> i32;
    fn getpid() -> i32;
}
fn counters() -> (u64, u64) {
    let mut b = [0u64; 64];
    unsafe { proc_pid_rusage(getpid(), 4, b.as_mut_ptr()) };
    (b[31], b[32])
}

#[inline(always)]
fn next_ip(ip: usize, prog: &[u16]) -> usize { if ip + 1 == prog.len() { 0 } else { ip + 1 } }
''')
# kernel U: in-place update of the stack top, like BinOpStackConst32 (stack_update -> Stack::set)
for i in range(N):
    w(f'''#[inline(never)]
fn u{i}(ex: &mut Exec<'_>, prog: &[u16], ip: usize, n: u64) -> u64 {{
    let v = &mut ex.store.stack.data;
    match v.last_mut() {{ Some(t) => *t = t.wrapping_add({2*i+1}), None => core::intrinsics::abort() }}
    if n == 0 {{ return v[0] as u64; }}
    let ip = next_ip(ip, prog);
    become U[prog[ip] as usize](ex, prog, ip, n - 1)
}}''')
# kernel P: push a constant (like Const32), and pop two push one (like I32Add); prog alternates p/a
for i in range(N):
    w(f'''#[inline(never)]
fn p{i}(ex: &mut Exec<'_>, prog: &[u16], ip: usize, n: u64) -> u64 {{
    let v = &mut ex.store.stack.data;
    if v.len() == v.capacity() {{ core::intrinsics::abort() }}
    v.push({2*i+1});
    if n == 0 {{ return v[0] as u64; }}
    let ip = next_ip(ip, prog);
    become P[prog[ip] as usize](ex, prog, ip, n - 1)
}}
#[inline(never)]
fn a{i}(ex: &mut Exec<'_>, prog: &[u16], ip: usize, n: u64) -> u64 {{
    let v = &mut ex.store.stack.data;
    let b = match v.pop() {{ Some(x) => x, None => core::intrinsics::abort() }};
    match v.last_mut() {{ Some(t) => *t = t.wrapping_add(b ^ {i}), None => core::intrinsics::abort() }}
    if n == 0 {{ return v[0] as u64; }}
    let ip = next_ip(ip, prog);
    become P[prog[ip] as usize](ex, prog, ip, n - 1)
}}''')
# control C: the same update, but each copy updates its own slot, so consecutive handlers don't alias
for i in range(N):
    w(f'''#[inline(never)]
fn c{i}(ex: &mut Exec<'_>, prog: &[u16], ip: usize, n: u64) -> u64 {{
    let v = &mut ex.store.side;
    match v.get_mut({i} * 16) {{ Some(t) => *t = t.wrapping_add({2*i+1}), None => core::intrinsics::abort() }}
    if n == 0 {{ return v[0] as u64; }}
    let ip = next_ip(ip, prog);
    become C[prog[ip] as usize](ex, prog, ip, n - 1)
}}''')
w('static U: [H; %d] = [%s];' % (N, ', '.join(f'u{i}' for i in range(N))))
# P table: even entries push, odd entries add; handler index 2*i -> p_i, 2*i+1 -> a_i
w('static P: [H; %d] = [%s];' % (2*N, ', '.join(f'p{i}, a{i}' for i in range(N))))
w('static C: [H; %d] = [%s];' % (N, ', '.join(f'c{i}' for i in range(N))))
w('''
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let kernel = args[1].as_str();          // u | p | c
    let k: usize = args[2].parse().unwrap(); // distinct handler copies
    let order = args[3].as_str();           // cyclic | random
    let n: u64 = args[4].parse().unwrap();  // dispatches per run
    let runs: u32 = args.get(5).map(|s| s.parse().unwrap()).unwrap_or(1);
    assert!(k >= 1 && k <= %d);
    // A program of 4096 handler indices over k copies.
    let len = 4096usize;
    let mut prog: Vec<u16> = Vec::with_capacity(len);
    let mut s: u64 = 0x9E3779B97F4A7C15;
    for j in 0..len {
        let copy = if order == "cyclic" { j %% k }
            else if let Some(r) = order.strip_prefix("block") { (j / r.parse::<usize>().unwrap()) %% k }
            else { s ^= s << 13; s ^= s >> 7; s ^= s << 17; (s %% k as u64) as usize };
        match kernel { "p" => prog.push((2 * copy + (j & 1)) as u16), _ => prog.push(copy as u16) }
    }
    if kernel == "p" { for j in 0..len { if j & 1 == 0 { prog[j] &= !1 } else { prog[j] |= 1 } } }
    let mut store = Store { stack: Stack { data: Vec::with_capacity(1024) }, side: vec![0u32; 16 * %d] };
    store.stack.data.push(1);
    let table: &[H] = match kernel { "u" => &U, "p" => &P, "c" => &C, _ => panic!() };
    let mut r = 0u64;
    let (i0, c0) = counters();
    let t = Instant::now();
    for _ in 0..runs {
        let mut ex = Exec { store: &mut store };
        r = r.wrapping_add(table[prog[0] as usize](&mut ex, &prog, 0, n));
    }
    let el = t.elapsed();
    let (i1, c1) = counters();
    let d = (n as f64 + 1.0) * runs as f64;
    println!("{kernel} k={k} {order}: {:.2} ns/dispatch, {:.2} instr/dispatch, {:.3} cycles/dispatch (r={r})",
        el.as_secs_f64() * 1e9 / d, (i1 - i0) as f64 / d, (c1 - c0) as f64 / d);
}''' % (N, N))
import os
os.makedirs('src', exist_ok=True)
open('src/main.rs', 'w').write('\n'.join(out))
print('generated', len(out), 'items')
