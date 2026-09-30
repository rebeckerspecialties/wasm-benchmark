#!/usr/bin/env python3
"""Where each variant's handler state lives on a target: for copy 0 of some handler kinds of every
variant in an assembly listing of tosb (cargo rustc ... -- --emit asm), the instruction count, the
callee-saved registers it saves, and its stack-slot accesses (stack-passed arguments and spills).

    asm_regs.py <tosb .s file> [kinds]      kinds default: bgS (binop, local.get, emptying local.set)

x86-64 when the path names an x86_64 target, arm64 otherwise.
"""
import re, sys, collections
path = sys.argv[1]
kinds = sys.argv[2] if len(sys.argv) > 2 else 'bgS'
x86 = 'x86_64' in path

def handler_name(label):
    """The generated fn name inside a mangled label: identifiers are length-prefixed."""
    for m in re.finditer(r'(?<!\d)(\d+)', label):
        n = int(m.group(1))
        ident = label[m.end():m.end() + n]
        if len(ident) == n and re.fullmatch(r'(vec|sp|tos)\w*_[A-Za-z]0', ident):
            return ident[:-1]
    return None

funcs, cur = {}, None
for line in open(path):
    m = re.match(r'^(\S+):\s*(#.*|;.*|//.*)?$', line)
    if m and not line.startswith(('.', 'L', ' ', '\t')):
        cur = handler_name(m.group(1))
        if cur: funcs[cur] = []
        continue
    if cur and re.match(r'^\s*\.(cfi_endproc|seh_endproc)|^\.?Lfunc_end', line):
        cur = None; continue
    if cur and re.match(r'^\s+[a-z]', line) and not re.match(r'^\s+\.', line):
        funcs[cur].append(line.strip())

rows = collections.OrderedDict()
for name, ins in funcs.items():
    variant, kind = name.rsplit('_', 1)
    if kind not in kinds:
        continue
    if x86:
        saved = sum(1 for l in ins if l.startswith('push'))
        stack = sum(1 for l in ins if re.search(r'\[rsp\b[^\]]*\]', l) and not l.startswith(('push', 'pop')))
    else:
        saved = sum(2 if l.startswith('stp') else 1 for l in ins if re.match(r'(stp|str)\s+x(19|2[0-9]|30)\b', l))
        stack = sum(1 for l in ins if re.search(r'\[(sp|x29)\b', l) and not re.match(r'(stp|ldp|str|ldr)\s+x(19|2[0-9]|30)\b', l))
    rows.setdefault(variant, {})[kind] = (len(ins), saved, stack)
print(f'{"variant":14}' + ''.join(f'  {k}: instrs/saved/stack' for k in kinds))
for v, d in rows.items():
    print(f'{v:14}' + ''.join(f'  {"/".join(map(str, d[k])) if k in d else "-":>21}' for k in kinds))
