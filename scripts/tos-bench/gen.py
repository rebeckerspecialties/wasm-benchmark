#!/usr/bin/env python3
"""Generates src/main.rs for tos-bench (see README.md).

Every variant implements the same eight stack-machine handler kinds (const, local.get, local.set,
local.tee, binop, in-place update, load, store) in MAXM distinct copies, dispatched like tinywasm's
nightly tail-call dispatch: `become` through a table of handlers taking the executor, the
instruction slice, the instruction pointer and the next instruction. The variants differ only in
where the operand stack's state lives between handlers:

  vec       the stack is a Vec reached through the executor (tinywasm today): its length makes a
            store-to-load round trip between handlers, and so does the top value
  sp        the stack is a fixed slice, and its height travels as a handler argument
  sp_wt     sp, plus the top value as a handler argument, written through to the slice
  sp_wb     sp, plus the top value as a handler argument, stored only when a push covers it
  tos_flag  sp, plus a cached top whose validity is a runtime flag (a branch on every push and pop)
  vec_tos   the Vec stack of `vec` with the top value as a handler argument
  sp_spill  sp, passed as the ninth integer argument, so it goes through the stack
  sp_wt_pn  sp_wt with extern "rust-preserve-none" handlers

Each copy of const, binop and update adds its own constant. The other kinds' copies are identical,
so build with -Z merge-functions=disabled to keep them apart.
"""
import os

MAXM = 64
KINDS = 'cgstbulwSW'  # const, local.get, local.set, local.tee, binop, update, load, store,
# and S / W: a local.set / store that leaves the stack empty (the parser knows the height)

# Per variant: extra handler parameters (after ex, prog, ip, instr), the initial values passed
# to the first handler, how the end handler forwards the state, the ABI, and each kind's body.
# A body ends with `next!(<state>)`, the state to pass to the next handler.
V = {}

V['vec'] = dict(params='', init='', fwd='', abi='Rust', bodies=dict(
    c='let s = &mut ex.store.stack; vpush(s, (op(instr) as u32) ^ @K); next!()',
    g='let b = ex.base; let s = &mut ex.store.stack; let x = get(s, b + op(instr)); vpush(s, x); next!()',
    s='let b = ex.base; let s = &mut ex.store.stack; let x = vpop(s); put(s, b + op(instr), x); next!()',
    t='let b = ex.base; let s = &mut ex.store.stack; let x = *vtop(s); put(s, b + op(instr), x); next!()',
    b='let s = &mut ex.store.stack; let y = vpop(s); let t = vtop(s); *t = t.wrapping_add(y ^ @K); next!()',
    u='let s = &mut ex.store.stack; let t = vtop(s); *t = t.wrapping_add(@K); next!()',
    l='let st = &mut *ex.store; let t = vtop(&mut st.stack); *t = get(&st.heap, (*t as usize) & HMASK); next!()',
    w='let st = &mut *ex.store; let v = vpop(&mut st.stack); let a = vpop(&mut st.stack); '
      'put(&mut st.heap, (a as usize) & HMASK, v); next!()',
))

SP = dict(
    c='let s = &mut ex.store.slots; put(s, sp, (op(instr) as u32) ^ @K); next!(sp + 1)',
    g='let b = ex.base; let s = &mut ex.store.slots; let x = get(s, b + op(instr)); put(s, sp, x); next!(sp + 1)',
    s='let b = ex.base; let s = &mut ex.store.slots; let x = get(s, sp.wrapping_sub(1)); put(s, b + op(instr), x); '
      'next!(sp.wrapping_sub(1))',
    t='let b = ex.base; let s = &mut ex.store.slots; let x = get(s, sp.wrapping_sub(1)); put(s, b + op(instr), x); next!(sp)',
    b='let s = &mut ex.store.slots; let y = get(s, sp.wrapping_sub(1)); let a = get(s, sp.wrapping_sub(2)); '
      'put(s, sp.wrapping_sub(2), a.wrapping_add(y ^ @K)); next!(sp.wrapping_sub(1))',
    u='let s = &mut ex.store.slots; let t = get(s, sp.wrapping_sub(1)); put(s, sp.wrapping_sub(1), t.wrapping_add(@K)); next!(sp)',
    l='let st = &mut *ex.store; let a = get(&st.slots, sp.wrapping_sub(1)); let v = get(&st.heap, (a as usize) & HMASK); '
      'put(&mut st.slots, sp.wrapping_sub(1), v); next!(sp)',
    w='let st = &mut *ex.store; let v = get(&st.slots, sp.wrapping_sub(1)); let a = get(&st.slots, sp.wrapping_sub(2)); '
      'put(&mut st.heap, (a as usize) & HMASK, v); next!(sp.wrapping_sub(2))',
)
V['sp'] = dict(params=', sp: usize', init=', NLOC', fwd=', sp', abi='Rust', bodies=SP)
V['sp_spill'] = dict(params=', d0: u64, d1: u64, d2: u64, sp: usize', init=', 1, 2, 3, NLOC', fwd=', d0, d1, d2, sp',
                     abi='Rust', bodies={k: b.replace('next!(', 'next!(d0, d1, d2, ') for k, b in SP.items()})

# Write-through: the slice always holds every value, and `t` mirrors slots[sp - 1] whenever the
# stack is not empty. A pop reloads `t` from the slot below; at an empty stack that reads a local.
WT = dict(
    c='let s = &mut ex.store.slots; let v = (op(instr) as u32) ^ @K; put(s, sp, v); next!(sp + 1, v)',
    g='let b = ex.base; let s = &mut ex.store.slots; let x = get(s, b + op(instr)); put(s, sp, x); next!(sp + 1, x)',
    s='let b = ex.base; let s = &mut ex.store.slots; put(s, b + op(instr), t); let n = get(s, sp.wrapping_sub(2)); '
      'next!(sp.wrapping_sub(1), n)',
    t='let b = ex.base; let s = &mut ex.store.slots; put(s, b + op(instr), t); next!(sp, t)',
    b='let s = &mut ex.store.slots; let a = get(s, sp.wrapping_sub(2)); let r = a.wrapping_add(t ^ @K); '
      'put(s, sp.wrapping_sub(2), r); next!(sp.wrapping_sub(1), r)',
    u='let s = &mut ex.store.slots; let r = t.wrapping_add(@K); put(s, sp.wrapping_sub(1), r); next!(sp, r)',
    l='let st = &mut *ex.store; let v = get(&st.heap, (t as usize) & HMASK); put(&mut st.slots, sp.wrapping_sub(1), v); next!(sp, v)',
    w='let st = &mut *ex.store; let a = get(&st.slots, sp.wrapping_sub(2)); put(&mut st.heap, (a as usize) & HMASK, t); '
      'let n = get(&st.slots, sp.wrapping_sub(3)); next!(sp.wrapping_sub(2), n)',
)
V['sp_wt'] = dict(params=', sp: usize, t: u32', init=', NLOC, 0', fwd=', sp, t', abi='Rust', bodies=WT)
V['sp_wt_pn'] = dict(params=', sp: usize, t: u32', init=', NLOC, 0', fwd=', sp, t', abi='rust-preserve-none', bodies=WT)

# Write-back: `t` is the top, the slice holds the values below it, slots[sp - 1] is the one just
# below. A push stores the old top at slots[sp]; at an empty stack that is the scratch slot NLOC.
V['sp_wb'] = dict(params=', sp: usize, t: u32', init=', NLOC, 0', fwd=', sp, t', abi='Rust', bodies=dict(
    c='let s = &mut ex.store.slots; put(s, sp, t); next!(sp + 1, (op(instr) as u32) ^ @K)',
    g='let b = ex.base; let s = &mut ex.store.slots; let x = get(s, b + op(instr)); put(s, sp, t); next!(sp + 1, x)',
    s='let b = ex.base; let s = &mut ex.store.slots; put(s, b + op(instr), t); let n = get(s, sp.wrapping_sub(1)); '
      'next!(sp.wrapping_sub(1), n)',
    t='let b = ex.base; let s = &mut ex.store.slots; put(s, b + op(instr), t); next!(sp, t)',
    b='let s = &mut ex.store.slots; let a = get(s, sp.wrapping_sub(1)); next!(sp.wrapping_sub(1), a.wrapping_add(t ^ @K))',
    u='next!(sp, t.wrapping_add(@K))',
    l='let v = get(&ex.store.heap, (t as usize) & HMASK); next!(sp, v)',
    w='let st = &mut *ex.store; let a = get(&st.slots, sp.wrapping_sub(1)); put(&mut st.heap, (a as usize) & HMASK, t); '
      'let n = get(&st.slots, sp.wrapping_sub(2)); next!(sp.wrapping_sub(2), n)',
))

# A cached top that may or may not hold a value: every push and pop tests the flag.
V['tos_flag'] = dict(params=', sp: usize, t: u32, valid: bool', init=', NLOC, 0, false', fwd=', sp, t, valid',
                     abi='Rust', bodies=dict(
    c='let mut c = Cache { s: &mut ex.store.slots, sp, t, valid }; c.push((op(instr) as u32) ^ @K); let (sp, t, valid) = (c.sp, c.t, c.valid); next!(sp, t, valid)',
    g='let b = ex.base; let mut c = Cache { s: &mut ex.store.slots, sp, t, valid }; let x = get(c.s, b + op(instr)); '
      'c.push(x); let (sp, t, valid) = (c.sp, c.t, c.valid); next!(sp, t, valid)',
    s='let b = ex.base; let mut c = Cache { s: &mut ex.store.slots, sp, t, valid }; let x = c.pop(); '
      'put(c.s, b + op(instr), x); let (sp, t, valid) = (c.sp, c.t, c.valid); next!(sp, t, valid)',
    t='let b = ex.base; let c = Cache { s: &mut ex.store.slots, sp, t, valid }; let x = c.peek(); '
      'put(c.s, b + op(instr), x); let (sp, t, valid) = (c.sp, c.t, c.valid); next!(sp, t, valid)',
    b='let mut c = Cache { s: &mut ex.store.slots, sp, t, valid }; let y = c.pop(); let a = c.pop(); '
      'c.push(a.wrapping_add(y ^ @K)); let (sp, t, valid) = (c.sp, c.t, c.valid); next!(sp, t, valid)',
    u='let mut c = Cache { s: &mut ex.store.slots, sp, t, valid }; let x = c.pop(); c.push(x.wrapping_add(@K)); '
      'let (sp, t, valid) = (c.sp, c.t, c.valid); next!(sp, t, valid)',
    l='let st = &mut *ex.store; let mut c = Cache { s: &mut st.slots, sp, t, valid }; let a = c.pop(); '
      'let v = get(&st.heap, (a as usize) & HMASK); c.push(v); let (sp, t, valid) = (c.sp, c.t, c.valid); next!(sp, t, valid)',
    w='let st = &mut *ex.store; let mut c = Cache { s: &mut st.slots, sp, t, valid }; let v = c.pop(); let a = c.pop(); '
      'put(&mut st.heap, (a as usize) & HMASK, v); let (sp, t, valid) = (c.sp, c.t, c.valid); next!(sp, t, valid)',
))

# The Vec stack with the top value in `t`: the Vec holds the values below it (and one scratch
# value once anything was pushed at an empty stack).
V['vec_tos'] = dict(params=', t: u32', init=', 0', fwd=', t', abi='Rust', bodies=dict(
    c='let s = &mut ex.store.stack; vpush(s, t); next!((op(instr) as u32) ^ @K)',
    g='let b = ex.base; let s = &mut ex.store.stack; let x = get(s, b + op(instr)); vpush(s, t); next!(x)',
    s='let b = ex.base; let s = &mut ex.store.stack; put(s, b + op(instr), t); let n = vpop(s); next!(n)',
    t='let b = ex.base; let s = &mut ex.store.stack; put(s, b + op(instr), t); next!(t)',
    b='let s = &mut ex.store.stack; let a = vpop(s); next!(a.wrapping_add(t ^ @K))',
    u='next!(t.wrapping_add(@K))',
    l='let v = get(&ex.store.heap, (t as usize) & HMASK); next!(v)',
    w='let st = &mut *ex.store; let a = vpop(&mut st.stack); put(&mut st.heap, (a as usize) & HMASK, t); '
      'let n = vpop(&mut st.stack); next!(n)',
))

# Variants that know statically when a pop empties the stack: S and W skip the reload of `t`.
V['sp_wt_nr'] = dict(V['sp_wt'], bodies=dict(WT,
    S='let b = ex.base; let s = &mut ex.store.slots; put(s, b + op(instr), t); next!(sp.wrapping_sub(1), t)',
    W='let st = &mut *ex.store; let a = get(&st.slots, sp.wrapping_sub(2)); put(&mut st.heap, (a as usize) & HMASK, t); '
      'next!(sp.wrapping_sub(2), t)'))
V['sp_wb_nr'] = dict(V['sp_wb'], bodies=dict(V['sp_wb']['bodies'],
    S='let b = ex.base; let s = &mut ex.store.slots; put(s, b + op(instr), t); next!(sp.wrapping_sub(1), t)',
    W='let st = &mut *ex.store; let a = get(&st.slots, sp.wrapping_sub(1)); put(&mut st.heap, (a as usize) & HMASK, t); '
      'next!(sp.wrapping_sub(2), t)'))
# Everywhere else S and W are local.set and store.
for v in V.values():
    v['bodies'].setdefault('S', v['bodies']['s'])
    v['bodies'].setdefault('W', v['bodies']['w'])

# sp_wt_nr without the Instruction argument: every handler reloads its instruction from the slice,
# which frees a register (x86-64 System V has 6).
V['sp_wt_nr_ni'] = dict(V['sp_wt_nr'], noinstr=True)

# sp_wt_nr with the stack's slice itself as a handler argument (its pointer and length in two more
# registers), so no slot address waits for a load of the base pointer. Too many arguments for the
# Rust ABI, so rust-preserve-none. The slice is not part of the store (it cannot be borrowed twice).
def _slice(b):
    b = b.replace('let s = &mut ex.store.slots;', 'let s = &mut *sl;').replace('&mut st.slots', '&mut *sl').replace('&st.slots', '&*sl')
    return b.replace('next!(', 'next!(sl, ')
V['sp_wt_nr_sl'] = dict(params=', sl: &mut [u32], sp: usize, t: u32', init=', sl, NLOC, 0', fwd=', sl, sp, t',
                        abi='rust-preserve-none', slice=True,
                        bodies={k: _slice(b) for k, b in V['sp_wt_nr']['bodies'].items()})

# The slice without the Instruction argument: eight integer arguments, the Rust ABI's arm64 limit.
V['sp_wt_nr_sl_ni'] = dict(V['sp_wt_nr_sl'], abi='Rust', noinstr=True)

ORDER = ['vec', 'sp', 'sp_wt', 'sp_wb', 'tos_flag', 'vec_tos', 'sp_spill', 'sp_wt_pn', 'sp_wt_nr', 'sp_wb_nr', 'sp_wt_nr_ni', 'sp_wt_nr_sl', 'sp_wt_nr_sl_ni']

out = []
w = out.append
w('''// Generated by gen.py. Do not edit.
#![feature(explicit_tail_calls, abort_immediate, rust_preserve_none_cc)]
#![allow(incomplete_features, unused_variables, unused_mut, non_snake_case, non_camel_case_types, non_upper_case_globals, clippy::all)]
use core::process::abort_immediate as die;
use std::time::Instant;

pub const NLOC: usize = 16;
pub const HMASK: usize = 1023;
pub const MAXM: usize = %d;
pub const END: usize = 10 * MAXM;

pub struct Store { pub stack: Vec<u32>, pub slots: Vec<u32>, pub heap: Vec<u32> }
pub struct Exec<'a> { pub store: &'a mut Store, pub base: usize, pub iters: u64 }

#[inline(always)] fn op(instr: u64) -> usize { ((instr >> 16) & 0xffff) as usize }
#[inline(always)] fn get(s: &[u32], i: usize) -> u32 { match s.get(i) { Some(&x) => x, None => die() } }
#[inline(always)] fn put(s: &mut [u32], i: usize, v: u32) { match s.get_mut(i) { Some(x) => *x = v, None => die() } }
#[inline(always)] fn vpush(s: &mut Vec<u32>, v: u32) { if s.len() == s.capacity() { die() } s.push(v) }
#[inline(always)] fn vpop(s: &mut Vec<u32>) -> u32 { match s.pop() { Some(x) => x, None => die() } }
#[inline(always)] fn vtop(s: &mut Vec<u32>) -> &mut u32 { match s.last_mut() { Some(x) => x, None => die() } }

/// tos_flag's cache: the logical stack is s[NLOC..sp], plus t when valid.
struct Cache<'a> { s: &'a mut [u32], sp: usize, t: u32, valid: bool }
impl Cache<'_> {
    #[inline(always)] fn push(&mut self, v: u32) { if self.valid { put(self.s, self.sp, self.t); self.sp += 1; } self.t = v; self.valid = true; }
    #[inline(always)] fn pop(&mut self) -> u32 { if self.valid { self.valid = false; self.t } else { self.sp = self.sp.wrapping_sub(1); get(self.s, self.sp) } }
    #[inline(always)] fn peek(&self) -> u32 { if self.valid { self.t } else { get(self.s, self.sp.wrapping_sub(1)) } }
}

fn checksum(s: &[u32], heap: &[u32]) -> u64 {
    let mut h = 0u64;
    for &x in &s[..NLOC] { h = h.wrapping_mul(0x100000001b3).wrapping_add(x as u64); }
    for &x in heap { h = h.wrapping_mul(0x100000001b3).wrapping_add(x as u64); }
    h
}
''' % MAXM)

for vn in ORDER:
    v = V[vn]
    abi = v['abi']
    ni = v.get('noinstr', False)
    targ = '' if ni else ', u64'             # the Instruction argument's type in the handler type
    parg = '' if ni else ', instr: u64'      # ... in a handler's parameters
    fwd_i = '' if ni else ', instr'          # ... when passing it on
    reload = 'let instr = match prog.get(ip) { Some(&i) => i, None => die() }; ' if ni else ''
    ty = f'extern "{abi}" fn(&mut Exec<\'_>, &[u64], usize{targ}{v["params"]}) -> u64'
    w(f'type H_{vn} = {ty};')
    stack_field = 'stack' if vn.startswith('vec') else 'slots'
    fwd = v['fwd']
    w(f'''macro_rules! next_{vn} {{ ($ex:ident, $prog:ident, $ip:ident $(, $st:expr)*) => {{{{
    let ip = $ip + 1;
    let Some(&instr) = $prog.get(ip) else {{ die() }};
    become T_{vn}[(instr & 0xffff) as usize]($ex, $prog, ip{fwd_i} $(, $st)*)
}}}} }}''')
    for kind in KINDS:
        body = v['bodies'][kind].replace('next!(', f'next_{vn}!(ex, prog, ip, ').replace(f'next_{vn}!(ex, prog, ip, )', f'next_{vn}!(ex, prog, ip)')
        for i in range(MAXM):
            b = body.replace('@K', f'{2 * i + 1}u32')
            w(f'#[inline(never)] extern "{abi}" fn {vn}_{kind}{i}(ex: &mut Exec<\'_>, prog: &[u64], ip: usize{parg}{v["params"]}) -> u64 {{ {reload}{b} }}')
    # end of the program: count down the iterations, then restart at 0 or return the checksum
    w(f'''#[inline(never)] extern "{abi}" fn {vn}_end(ex: &mut Exec<'_>, prog: &[u64], ip: usize{parg}{v["params"]}) -> u64 {{
    if ex.iters == 0 {{ return checksum({'sl' if v.get('slice') else '&ex.store.' + stack_field}, &ex.store.heap); }}
    ex.iters -= 1;
    let Some(&instr) = prog.first() else {{ die() }};
    become T_{vn}[(instr & 0xffff) as usize](ex, prog, 0{fwd_i}{fwd})
}}''')
    names = [f'{vn}_{kind}{i}' for kind in KINDS for i in range(MAXM)] + [f'{vn}_end']
    w(f'static T_{vn}: [H_{vn}; END + 1] = [{", ".join(names)}];')
    sl_param = ', sl: &mut [u32]' if v.get('slice') else ''
    w(f'''fn run_{vn}(ex: &mut Exec<'_>, prog: &[u64]{sl_param}) -> u64 {{ let instr = prog[0]; T_{vn}[(instr & 0xffff) as usize](ex, prog, 0{fwd_i}{v["init"]}) }}''')

w('''
/// A program of `stmts` random statements over NLOC locals. Every handler picks one of `m` copies
/// of its kind, so the program uses up to 8 * m distinct handlers. The same seed gives the same
/// statements for every m.
fn program(m: usize, stmts: usize, seed: u64) -> (Vec<u64>, usize) {
    let mut s = seed | 1;
    let mut rnd = move || { s ^= s << 13; s ^= s >> 7; s ^= s << 17; s };
    let mut copies = { let mut c = seed.wrapping_mul(0x9E3779B97F4A7C15) | 1; move || { c ^= c << 13; c ^= c >> 7; c ^= c << 17; c } };
    let mut prog = Vec::new();
    let mut e = |kind: usize, operand: u64, copy: u64| prog.push((kind as u64 * MAXM as u64 + copy % m as u64) | (operand << 16));
    for _ in 0..stmts {
        let (a, b, c, x, y) = (rnd() % 16, rnd() % 16, rnd() % 16, rnd() % 16, rnd() % 16);
        let k = rnd() & 0xffff;
        // kinds: 0 const, 1 local.get, 2 local.set, 3 local.tee, 4 binop, 5 update, 6 load, 7 store,
        // 8 / 9 local.set / store that empty the stack
        let stmt: &[(usize, u64)] = match rnd() % 20 {
            0..=3 => &[(1, a), (1, b), (4, 0), (8, x)],                    // x = a op b
            4..=6 => &[(1, a), (0, k), (4, 0), (8, x)],                    // x = a op k
            7..=8 => &[(1, a), (5, 0), (8, x)],                            // x = a + k
            9..=11 => &[(1, a), (1, b), (4, 0), (3, x), (1, c), (4, 0), (8, y)], // y = (x = a op b) op c
            12..=13 => &[(1, a), (6, 0), (8, x)],                          // x = heap[a]
            14..=15 => &[(1, a), (1, b), (9, 0)],                          // heap[a] = b
            16..=17 => &[(1, a), (1, b), (4, 0), (5, 0), (1, c), (4, 0), (8, x)], // x = ((a op b) + k) op c
            _ => &[(1, a), (1, b), (1, c), (4, 0), (4, 0), (8, x)],       // x = a op (b op c)
        };
        for &(kind, operand) in stmt { let cp = copies(); e(kind, operand, cp); }
    }
    let n = prog.len();
    prog.push(END as u64);
    (prog, n + 1)
}

unsafe extern "C" {
    fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut u64) -> i32;
    fn getpid() -> i32;
}
fn counters() -> (u64, u64) {
    let mut b = [0u64; 64];
    unsafe { proc_pid_rusage(getpid(), 4, b.as_mut_ptr()) };
    (b[31], b[32])
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 { eprintln!("tosb <variant> <copies 1..=64> <iterations> [runs] [seed] [stmts]"); std::process::exit(2); }
    let variant = args[1].as_str();
    let m: usize = args[2].parse().unwrap();
    let iters: u64 = args[3].parse().unwrap();
    let runs: u32 = args.get(4).map(|s| s.parse().unwrap()).unwrap_or(1);
    let seed: u64 = args.get(5).map(|s| s.parse().unwrap()).unwrap_or(0x5eed);
    let stmts: usize = args.get(6).map(|s| s.parse().unwrap()).unwrap_or(64);
    assert!((1..=MAXM).contains(&m));
    let (prog, len) = program(m, stmts, seed);
    let mut distinct: Vec<u64> = prog.iter().map(|i| i & 0xffff).collect();
    distinct.sort(); distinct.dedup();
    let mut pairs: Vec<(u64, u64)> = prog.windows(2).map(|w| (w[0] & 0xffff, w[1] & 0xffff)).collect();
    pairs.push((END as u64, prog[0] & 0xffff));
    pairs.sort(); pairs.dedup();
    let mut r = 0u64;
    let (mut i0, mut c0) = (0, 0);
    let mut t = Instant::now();
    for run in 0..=runs {
        // run 0 warms up and is not counted
        if run == 1 { (i0, c0) = counters(); t = Instant::now(); }
        let mut store = Store {
            stack: { let mut v = Vec::with_capacity(4096); v.extend((0..NLOC as u32).map(|i| i * 7 + 1)); v },
            slots: { let mut v = vec![0u32; 4096]; for i in 0..NLOC { v[i] = i as u32 * 7 + 1; } v },
            heap: (0..=HMASK as u32).map(|j| j * 3 + 1).collect(),
        };
        let mut sl = store.slots.clone();
        let mut ex = Exec { store: &mut store, base: 0, iters: iters - 1 };
        let x = match variant {
''')
for vn in ORDER:
    w(f'            "{vn}" => run_{vn}(&mut ex, &prog{", &mut sl" if V[vn].get("slice") else ""}),')
w('''            _ => panic!("unknown variant {variant}"),
        };
        r = if run == 0 { x } else { assert_eq!(x, r, "runs disagree"); x };
    }
    let el = t.elapsed();
    let (i1, c1) = counters();
    let d = (len as f64) * iters as f64 * runs as f64;
    println!("{variant} m={m}: {:.3} ns/dispatch, {:.2} instr/dispatch, {:.3} cycles/dispatch, {} handlers, {} pairs, {} dispatches/iter, checksum {r:016x}",
        el.as_secs_f64() * 1e9 / d, (i1 - i0) as f64 / d, (c1 - c0) as f64 / d, distinct.len(), pairs.len(), len);
}''')

os.makedirs('src', exist_ok=True)
open('src/main.rs', 'w').write('\n'.join(out) + '\n')
print('wrote src/main.rs:', sum(len(x) for x in out) // 1024, 'KiB')
