#!/usr/bin/env python3
"""Breakage census for BOON_COMPILER_REWRITE_PLAN.md section 5.1 (review M1, 2026-09-30).

Token-level scanner (not a parser). Corpus:
  * every tracked *.bn file (git ls-files "*.bn")
  * Boon sources embedded as "..." strings inside tracked .bn files (persons_pro templates)
  * Boon sources embedded as string literals in crates/**/*.rs (raw and escaped strings
    containing at least two Boon markers)
Usage: python3 census.py [--out DIR] [--samples N]
Writes census_results.json (all sites) and prints a summary.
"""
import collections, json, os, re, subprocess, sys, random

ROOT = '/home/martinkavik/repos/boon-circuit'
OUT = sys.argv[sys.argv.index('--out') + 1] if '--out' in sys.argv else os.path.dirname(os.path.abspath(__file__))

BIN_OPS = {'+', '-', '*', '/', '==', '!=', '<', '>', '<=', '>='}
CMP_OPS = {'==', '!=', '<', '>', '<=', '>='}
CONT_START = {'|>', '=>', '.'} | BIN_OPS
CONT_END = {'|>', '=>', ':', ',', '...'} | BIN_OPS
OPEN = {'(': ')', '[': ']', '{': '}'}
EVENT_SEGS = {'events', 'event', 'press', 'click', 'key_down', 'key_up', 'change', 'blur', 'focus',
              'double_click', 'submit', 'message', 'tick', 'released', 'pressed_event', 'input_event',
              'pointer_down', 'pointer_up', 'wheel', 'scroll', 'drop', 'drag', 'mouse_down', 'mouse_up',
              'hover_event', 'selected_event', 'resize', 'connected', 'disconnected', 'received'}
EFFECT_MODS = {'File', 'Http', 'Wellen', 'Timer', 'Random', 'Clock', 'Directory', 'Secret',
               'DevelopmentPasskey', 'Shell', 'Build', 'Publish', 'Server', 'Client', 'Session',
               'Host', 'Stremio', 'Dependency', 'Stream', 'Crypto', 'Bell'}
COMMANDS = {('File', 'write_bytes'), ('File', 'write_text'), ('File', 'write'), ('Http', 'request'),
            ('Clock', 'wall'), ('Random', 'bytes'), ('Shell', 'run'), ('Publish', 'artifact')}
STATEFUL_BUILTINS = {('Bool', 'toggle'), ('Timer', 'interval'), ('Number', 'counter')}
LIST_BINDER_FNS = {'map', 'retain', 'remove', 'any', 'all', 'count', 'find', 'filter', 'sort_by',
                   'every', 'sum', 'find_map', 'key_by', 'group_by', 'flat_map', 'fold', 'each', 'take_while'}

# ---------------------------------------------------------------- corpus
def git_bn():
    out = subprocess.run(['git', 'ls-files', '*.bn'], cwd=ROOT, capture_output=True, text=True).stdout
    return [l for l in out.splitlines() if l]

def rust_files():
    out = subprocess.run(['git', 'ls-files', 'crates/**/*.rs', 'xtask/**/*.rs'], cwd=ROOT, capture_output=True, text=True).stdout
    return [l for l in out.splitlines() if l]

MARKERS = ['|> HOLD', '|> WHEN', '|> THEN', '|> WHILE', 'LATEST {', 'FUNCTION ', ': SOURCE', 'document:', 'store:', 'Element/', 'Document/new']

def boonish(s):
    return len(s) > 30 and sum(1 for m in MARKERS if m in s) >= 2

def unescape(s):
    return (s.replace('\\n', '\n').replace('\\t', '\t').replace('\\"', '"').replace('\\\\', '\\'))

def extract_rust_strings(path):
    src = open(os.path.join(ROOT, path), encoding='utf-8', errors='replace').read()
    res = []
    i, n = 0, len(src)
    while i < n:
        c = src[i]
        if src.startswith('//', i):
            j = src.find('\n', i); i = n if j < 0 else j; continue
        m = re.match(r'r(#*)"', src[i:i + 12])
        if m and (i == 0 or not (src[i - 1].isalnum() or src[i - 1] == '_')):
            hashes = m.group(1); start = i + len(m.group(0)); end = src.find('"' + hashes, start)
            if end < 0: break
            body = src[start:end]
            if boonish(body): res.append((src.count('\n', 0, i) + 1, body))
            i = end + 1 + len(hashes); continue
        if c == '"':
            j = i + 1
            while j < n and src[j] != '"':
                j += 2 if src[j] == '\\' else 1
            body = src[i + 1:j]
            # line continuations "\<newline>   "
            body = re.sub(r'\\\n\s*', '', body)
            body = unescape(body)
            if boonish(body): res.append((src.count('\n', 0, i) + 1, body))
            i = j + 1; continue
        if c == "'" and i + 2 < n and (src[i + 2] == "'" or (src[i + 1] == '\\' and src[i + 3:i + 4] == "'")):
            i += 3; continue
        i += 1
    return res

# ---------------------------------------------------------------- lexer
class Tok:
    __slots__ = ('t', 'k', 'line', 'col', 'interp', 'first')
    def __init__(s, t, k, line, col, interp=()):
        s.t, s.k, s.line, s.col, s.interp, s.first = t, k, line, col, interp, False
    def __repr__(s): return s.t

ID_RE = re.compile(r'[A-Za-z_][A-Za-z0-9_]*')
NUM_RE = re.compile(r'\d[\d_]*(?:\.\d+)?(?:[a-z]\w*)?')

def lex(src):
    toks = []; i = 0; line = 1; n = len(src); linestart = 0
    while i < n:
        c = src[i]
        if c == '\n': line += 1; i += 1; linestart = i; continue
        if c in ' \t\r': i += 1; continue
        if src.startswith('--', i):
            j = src.find('\n', i); i = n if j < 0 else j; continue
        if src.startswith('TEXT', i) and (i + 4 >= n or not (src[i + 4].isalnum() or src[i + 4] == '_')):
            j = i + 4
            while j < n and src[j] in ' \t': j += 1
            if j < n and src[j] == '{':
                d = 0; k = j
                while k < n:
                    if src[k] == '{': d += 1
                    elif src[k] == '}':
                        d -= 1
                        if d == 0: break
                    k += 1
                content = src[j + 1:k]
                interp = tuple(re.findall(r'\{\s*([A-Za-z_][A-Za-z0-9_.]*)\s*\}', content))
                toks.append(Tok('TEXT{}', 'text', line, i - linestart, interp))
                line += content.count('\n'); i = k + 1
                if '\n' in content: linestart = src.rfind('\n', 0, i) + 1
                continue
        if c == '"':
            j = i + 1
            while j < n and src[j] != '"':
                j += 2 if src[j] == '\\' else 1
            toks.append(Tok('"str"', 'str', line, i - linestart, (src[i + 1:j],)))
            line += src.count('\n', i, j); i = j + 1; continue
        m = ID_RE.match(src, i)
        if m:
            toks.append(Tok(m.group(0), 'id', line, i - linestart)); i = m.end(); continue
        m = NUM_RE.match(src, i)
        if m:
            toks.append(Tok(m.group(0), 'num', line, i - linestart)); i = m.end(); continue
        for op in ('...', '|>', '=>', '==', '!=', '>=', '<='):
            if src.startswith(op, i):
                toks.append(Tok(op, 'op', line, i - linestart)); i += len(op); break
        else:
            toks.append(Tok(c, 'op', line, i - linestart)); i += 1
    prev = -1
    for t in toks:
        if t.line != prev: t.first = True; prev = t.line
    return toks

# ---------------------------------------------------------------- structure
class Unit:
    def __init__(self, name, src, example, origin):
        self.name, self.src, self.example, self.origin = name, src, example, origin
        self.toks = lex(src)
        n = len(self.toks)
        self.match = [-1] * n; self.parent = [-1] * n
        st = []
        for i, t in enumerate(self.toks):
            self.parent[i] = st[-1] if st else -1
            if t.t in OPEN: st.append(i)
            elif t.t in (')', ']', '}'):
                if st:
                    o = st.pop(); self.match[o] = i; self.match[i] = o
                    self.parent[i] = self.parent[o]
        self.unbalanced = len(st)
        self._items = {}
        self.funcs = []  # (name, params, open_idx, close_idx, head_idx)
        for i, t in enumerate(self.toks):
            if t.t == 'FUNCTION' and i + 2 < n and self.toks[i + 1].k == 'id' and self.toks[i + 2].t == '(':
                p = i + 2; q = self.match[p]
                params = [self.toks[k].t for k in range(p + 1, q if q > 0 else p + 1)
                          if self.toks[k].k == 'id' and self.parent[k] == p and self.toks[k + 1].t in (',', ')', ':')]
                b = q + 1 if q > 0 else -1
                if 0 <= b < n and self.toks[b].t == '{':
                    self.funcs.append((self.toks[i + 1].t, params, b, self.match[b], i))

    def tt(self, i): return self.toks[i].t if 0 <= i < len(self.toks) else None

    def kind_of_group(self, o):
        """Kind of the bracket group opened at o."""
        t = self.toks[o].t
        if o < 0: return 'root'
        if t == '(': return 'call' if o > 0 and self.toks[o - 1].k == 'id' else 'paren'
        if t == '[':
            return 'tagged' if o > 0 and self.toks[o - 1].k == 'id' and self.toks[o - 1].t[0].isupper() and self.toks[o-1].line == self.toks[o].line else 'record'
        p = self.tt(o - 1); p2 = self.tt(o - 2)
        if p in ('WHEN', 'WHILE', 'LATEST', 'BLOCK', 'THEN', 'LIST', 'SET', 'MAP'): return p
        if p2 == 'HOLD': return 'HOLD'
        if p2 == 'FUNCTION' or (p == ')' and self.tt(self.match[o - 1] - 2) == 'FUNCTION'): return 'FUNC'
        return '{'

    def toplevel(self, o):
        """Top-level token indices inside group opened at o (o=-1: file root)."""
        if o in self._items: return self._items[o]
        s, e = (0, len(self.toks)) if o < 0 else (o + 1, self.match[o])
        if e < 0: e = len(self.toks)
        out = []; k = s
        while k < e:
            out.append(k)
            if self.toks[k].t in OPEN and self.match[k] > k: k = self.match[k] + 1
            else: k += 1
        self._items[o] = out
        return out

    def items(self, o):
        """Split group into items: list of lists of top-level token indices."""
        tl = self.toplevel(o); items = []; cur = []; prev_end_line = None; prev_t = None
        for k in tl:
            t = self.toks[k]
            if t.t == ',':
                if cur: items.append(cur)
                cur = []; prev_t = ','; continue
            if cur and t.line > prev_end_line and t.t not in CONT_START and prev_t not in CONT_END:
                items.append(cur); cur = []
            cur.append(k)
            prev_end_line = self.toks[self.match[k]].line if t.t in OPEN and self.match[k] > k else t.line
            prev_t = self.toks[self.match[k]].t if t.t in OPEN and self.match[k] > k else t.t
        if cur: items.append(cur)
        return items

    def span_end(self, k):
        return self.match[k] if self.toks[k].t in OPEN and self.match[k] > k else k

    def item_expr_start(self, item):
        """Index into item where the expression starts (after `key:` / `pattern =>`)."""
        for j, k in enumerate(item):
            if self.toks[k].t == '=>': return j + 1
        if len(item) >= 2 and self.toks[item[0]].k == 'id' and self.toks[item[1]].t == ':': return 2
        if item and self.toks[item[0]].t == '...': return 1
        return 0

    def containing_item(self, i):
        o = self.parent[i]
        for it in self.items(o):
            if it[0] <= i <= self.span_end(it[-1]): return o, it
        return o, []

    def enclosing(self, i, kinds):
        o = self.parent[i]
        while o >= 0:
            if self.kind_of_group(o) in kinds: return o
            o = self.parent[o]
        return -1

    def func_of(self, i):
        best = None
        for f in self.funcs:
            if f[2] < i < f[3]:
                if best is None or f[2] > best[2]: best = f
        return best

    def toplevel_field_of(self, i):
        """Name of the module-level field containing token i."""
        o = i
        while self.parent[o] >= 0: o = self.parent[o]
        for it in self.items(-1):
            if it[0] <= o <= self.span_end(it[-1]):
                if len(it) >= 2 and self.toks[it[1]].t == ':': return self.toks[it[0]].t
                if self.toks[it[0]].t == 'FUNCTION': return 'FUNCTION ' + self.toks[it[1]].t
        return None

    def field_key_of(self, i):
        """Innermost `key:` whose initializer contains i."""
        o = self.parent[i]; cur = i
        while True:
            for it in self.items(o):
                if it[0] <= cur <= self.span_end(it[-1]):
                    if len(it) >= 2 and self.toks[it[0]].k == 'id' and self.toks[it[1]].t == ':' and self.kind_of_group(o) in ('record', 'BLOCK', 'root', 'tagged'):
                        return self.toks[it[0]].t, it
            if o < 0: return None, None
            cur = o; o = self.parent[o]

    def path_at(self, k):
        """Read dotted path starting at id token k. Returns (path_str, last_index)."""
        parts = [self.toks[k].t]; j = k
        while self.tt(j + 1) == '.' and j + 2 < len(self.toks) and self.toks[j + 2].k == 'id':
            parts.append(self.toks[j + 2].t); j += 2
        return '.'.join(parts), j

    def text(self, a, b, maxlen=160):
        s = ' '.join(self.toks[k].t for k in range(a, b + 1))
        return s if len(s) <= maxlen else s[:maxlen] + ' ...'

    def keys_defined(self, o):
        """Keys defined directly in record/BLOCK/root group o."""
        ks = set()
        for it in self.items(o):
            if len(it) >= 2 and self.toks[it[0]].k == 'id' and self.toks[it[1]].t == ':': ks.add(self.toks[it[0]].t)
        return ks

# ---------------------------------------------------------------- reads inside a range
def pattern_binders(u, pat_idx):
    """Binders introduced by a WHEN/WHILE pattern (token index list)."""
    b = []
    if not pat_idx: return b
    first = u.toks[pat_idx[0]]
    if len(pat_idx) == 1 and first.k == 'id' and (first.t[0].islower() or (first.t[0] == '_' and first.t != '__')):
        b.append(first.t)
    for k in pat_idx:
        if u.toks[k].t == '[' and u.match[k] > k:
            for q in range(k + 1, u.match[k]):
                if u.toks[q].k == 'id' and u.toks[q].t[0].islower() and u.parent[q] == k and u.tt(q + 1) != ':' and u.tt(q - 1) != ':':
                    b.append(u.toks[q].t)
                elif u.toks[q].k == 'id' and u.parent[q] == k and u.tt(q - 1) == ':' and u.toks[q].t[0].islower():
                    b.append(u.toks[q].t)
    return b

def local_binders_in(u, a, b):
    """Binders introduced inside token range [a,b]: HOLD x, List/f(x, ...), nested pattern binders."""
    loc = set()
    b = min(b, len(u.toks) - 1)
    for k in range(a, b + 1):
        t = u.toks[k].t
        if t == 'HOLD' and k + 1 <= b and u.toks[k + 1].k == 'id': loc.add(u.toks[k + 1].t)
        if t == '(' and u.tt(k - 2) == '/' and u.tt(k - 1) in LIST_BINDER_FNS and k + 2 <= b and u.toks[k + 1].k == 'id' and u.tt(k + 2) == ',':
            loc.add(u.toks[k + 1].t)
        if t == '=>':
            o, it = u.containing_item(k)
            j = it.index(k) if k in it else -1
            if j > 0: loc.update(pattern_binders(u, it[:j]))
    return loc

def reads_in(u, a, b, extra_bound=()):
    """Free reads (dotted paths) and calls in token range [a, b]."""
    b = min(b, len(u.toks) - 1)
    bound = set(extra_bound) | local_binders_in(u, a, b)
    reads = []; calls = []
    k = a
    while k <= b:
        t = u.toks[k]
        if t.k == 'text':
            for p in t.interp:
                if p.split('.')[0] not in bound: reads.append((p, k))
            k += 1; continue
        if t.k == 'id':
            prev = u.tt(k - 1); nxt = u.tt(k + 1)
            if prev == '/' and nxt == '(':
                calls.append((u.tt(k - 2), t.t, k)); k += 1; continue
            if prev == '.' or prev == '/' or nxt == '/':
                k += 1; continue
            if nxt == '(':
                if u.tt(k - 1) != 'FUNCTION': calls.append((None, t.t, k))
                k += 1; continue
            if nxt == ':' and u.kind_of_group(u.parent[k]) in ('record', 'BLOCK', 'call', 'tagged', 'root', 'paren'):
                k += 1; continue
            if t.t[0].isupper() and t.t not in ('PASSED',):
                k += 1; continue
            if t.t in ('__', 'HOLD', 'SKIP'): k += 1; continue
            if prev == 'HOLD': k += 1; continue
            path, j = u.path_at(k)
            root = t.t
            if root not in bound:
                # record-literal / BLOCK siblings inside the range are local
                o = u.parent[k]; local = False
                while o >= a - 1 and o >= 0 and o >= a:
                    if u.kind_of_group(o) in ('record', 'BLOCK', 'tagged') and root in u.keys_defined(o): local = True; break
                    o = u.parent[o]
                if not local: reads.append((path, k))
            k = j + 1; continue
        k += 1
    return reads, calls

# ---------------------------------------------------------------- example grouping
def example_of(path):
    p = path.split('/')
    if p[0] == 'examples':
        if len(p) == 2: return p[1][:-3] if p[1].endswith('.bn') else p[1]
        if p[1] == 'migrations': return 'migrations/' + p[2]
        return p[1]
    if p[0] == 'testdata': return 'testdata'
    if p[0] == 'crates': return 'crates/' + p[1]
    return p[0]

VIEW_FUNCS = set()
def role_of(u, i):
    base = os.path.basename(u.name)
    if re.search(r'theme', u.name, re.I) or base in ('Classic.bn', 'Professional.bn', 'Glassmorphism.bn', 'Neobrutalism.bn', 'Neumorphism.bn'):
        return 'theme'
    f = u.func_of(i)
    if f and (u.name, f[0]) in VIEW_FUNCS: return 'view'
    if f:
        body = [u.toks[k].t for k in range(f[2], f[3])]
    else:
        fld = u.toplevel_field_of(i)
        if fld in ('document', 'scene', 'view', 'ui', 'root'): return 'view'
        o = i
        while u.parent[o] >= 0: o = u.parent[o]
        body = []
        for it in u.items(-1):
            if it[0] <= o <= u.span_end(it[-1]):
                body = [u.toks[k].t for k in range(it[0], u.span_end(it[-1]) + 1)]
    if 'Element' in body or 'Scene' in body or 'Document' in body: return 'view'
    return 'state'

# ---------------------------------------------------------------- main census
def main():
    units = []
    for p in git_bn():
        src = open(os.path.join(ROOT, p), encoding='utf-8').read()
        u = Unit(p, src, example_of(p), 'bn')
        units.append(u)
        for t in u.toks:
            if t.k == 'str' and boonish(unescape(t.interp[0])):
                units.append(Unit(f'{p}@str:{t.line}', unescape(t.interp[0]), example_of(p), 'bn-string'))
    rust_hits = collections.Counter()
    for p in rust_files():
        for line, body in extract_rust_strings(p):
            units.append(Unit(f'{p}@{line}', body, 'rust:' + p.split('/')[1], 'rust'))
            rust_hits[p] += 1

    # module registry (user modules by file basename, per example)
    user_modules = collections.defaultdict(set)
    funcs_by_example = collections.defaultdict(dict)
    for u in units:
        base = os.path.basename(u.name.split('@')[0])[:-3]
        user_modules[u.example].add(base)
        for f in u.funcs:
            funcs_by_example[u.example].setdefault(f[0], []).append((u, f, base))
    # transitive "reads PASSED / module-level state" per function
    reads_passed = {}
    def fkey(u, f): return (u.name, f[0], f[2])
    for u in units:
        for f in u.funcs:
            reads_passed[fkey(u, f)] = any(u.toks[k].t == 'PASSED' for k in range(f[2], f[3]))
    changed = True
    while changed:
        changed = False
        for u in units:
            for f in u.funcs:
                if reads_passed[fkey(u, f)]: continue
                _, calls = reads_in(u, f[2] + 1, f[3] - 1)
                for mod, name, _k in calls:
                    for (cu, cf, cbase) in funcs_by_example[u.example].get(name, []):
                        if (mod is None and cu.name.split('@')[0] == u.name.split('@')[0]) or (mod is not None and mod == cbase) or (mod is None and cu.example == u.example):
                            if reads_passed[fkey(cu, cf)]:
                                reads_passed[fkey(u, f)] = True; changed = True; break
                    if reads_passed[fkey(u, f)]: break
    def impure_call(u, mod, name):
        if mod is not None and mod not in user_modules[u.example]: return False
        for (cu, cf, cbase) in funcs_by_example[u.example].get(name, []):
            if mod is not None and mod != cbase: continue
            if reads_passed[fkey(cu, cf)]: return True
        return False
    # stateful FUNCTIONs (contain HOLD / SOURCE / LATEST with state / collection updates), transitive
    stateful = {}
    for u in units:
        for f in u.funcs:
            ts = [u.toks[k].t for k in range(f[2], f[3])]
            stateful[fkey(u, f)] = ('HOLD' in ts or 'SOURCE' in ts)
    changed = True
    while changed:
        changed = False
        for u in units:
            for f in u.funcs:
                if stateful[fkey(u, f)]: continue
                _, calls = reads_in(u, f[2] + 1, f[3] - 1)
                for mod, name, _k in calls:
                    for (cu, cf, cbase) in funcs_by_example[u.example].get(name, []):
                        if (mod is None or mod == cbase) and stateful[fkey(cu, cf)]:
                            stateful[fkey(u, f)] = True; changed = True; break
                    if stateful[fkey(u, f)]: break

    # view functions: contain Element/Scene/Document, plus everything they call (closure), minus stateful ones
    frontier = []
    for u in units:
        for f in u.funcs:
            ts = {u.toks[k].t for k in range(f[2], f[3])}
            if ts & {'Element', 'Scene', 'Document'}: VIEW_FUNCS.add((u.name, f[0])); frontier.append((u, f))
    while frontier:
        u, f = frontier.pop()
        for mod, name, _k in reads_in(u, f[2] + 1, f[3] - 1)[1]:
            for (cu, cf, cb) in funcs_by_example[u.example].get(name, []):
                if (mod is None or mod == cb) and (cu.name, cf[0]) not in VIEW_FUNCS and not stateful[fkey(cu, cf)] and not re.search('theme', cu.name, re.I):
                    VIEW_FUNCS.add((cu.name, cf[0])); frontier.append((cu, cf))
    # argument literalness per (example, function, label)
    arg_lit = collections.defaultdict(collections.Counter)
    for u in units:
        for k in range(len(u.toks) - 1):
            if u.toks[k].k != 'id' or u.tt(k + 1) != '(' or u.tt(k - 1) == 'FUNCTION': continue
            for arg in u.items(k + 1):
                if len(arg) >= 3 and u.tt(arg[1]) == ':':
                    v = [u.toks[q] for q in range(arg[2], u.span_end(arg[-1]) + 1)]
                    lit = all(x.k in ('num', 'text') or (x.k == 'id' and x.t[0].isupper() and x.t != 'PASSED') or x.t in ('[', ']', ',', ':', '-') or (x.k == 'id' and u.tt(arg[2]) != x.t and False) for x in v)
                    arg_lit[(u.example, u.toks[k].t, u.toks[arg[0]].t)]['lit' if lit else 'nonlit'] += 1
    R = collections.defaultdict(list)   # class -> list of site dicts
    C = collections.Counter()
    per_example = collections.defaultdict(collections.Counter)

    def site(u, k, **kw):
        d = dict(file=u.name, line=u.toks[k].line, example=u.example, origin=u.origin)
        d.update(kw); return d

    for u in units:
        toks = u.toks; n = len(toks)
        C['units_' + u.origin] += 1
        C['lines_' + u.origin] += u.src.count('\n') + 1
        source_keys = set()
        hold_fields = set(); latest_const_fields = set()
        for k in range(n - 2):
            if toks[k].k == 'id' and toks[k + 1].t == ':' and toks[k + 2].t == 'SOURCE': source_keys.add(toks[k].t)
        # field -> initializer kind (for THEN-over-state)
        for k in range(n - 1):
            if toks[k].k == 'id' and toks[k + 1].t == ':' and toks[k].t[0].islower():
                o, it = u.containing_item(k)
                if it and it[0] == k and len(it) > 2:
                    ts = [toks[q].t for q in it[2:]]
                    if 'HOLD' in ts: hold_fields.add(toks[k].t)
        # ------------------------------------------------ WHEN / WHILE
        for k in range(1, n - 1):
            t = toks[k].t
            if t not in ('WHEN', 'WHILE') or toks[k + 1].t != '{' or toks[k - 1].t != '|>': continue
            C[t] += 1; per_example[u.example][t] += 1
            o, it = u.containing_item(k - 1)
            if not it: continue
            s = it[u.item_expr_start(it)] if u.item_expr_start(it) < len(it) else it[0]
            sel_a, sel_b = s, k - 2
            sel_ts = [toks[q] for q in range(sel_a, sel_b + 1)] if sel_b >= sel_a else []
            sel_ids = [x.t for x in sel_ts if x.k == 'id']
            sel_path = None
            if sel_ts and sel_ts[0].k == 'id':
                sel_path, j = u.path_at(sel_a)
                if j != sel_b: sel_path_simple = False
            sel_simple = bool(sel_ts) and sel_ts[0].k == 'id' and u.path_at(sel_a)[1] == sel_b
            segs = set()
            for x in sel_ts:
                if x.k == 'id': segs.add(x.t)
            sel_mods = {toks[q - 1].t for q in range(sel_a, sel_b + 1) if toks[q].t == '/' and q - 1 >= 0}
            is_event = ('THEN' in segs) or bool(segs & EVENT_SEGS) or bool(segs & source_keys)
            is_effect = bool(sel_mods & EFFECT_MODS) or any(re.search(r'(_result|_response|_reply|_completion|_outcome|_page|_event|_events)$', x) for x in sel_ids)
            is_literal = bool(sel_ts) and all(x.k in ('num', 'text') or (x.k == 'id' and x.t[0].isupper()) for x in sel_ts)
            sel_kind = 'event' if is_event else ('effect_result' if is_effect else ('literal' if is_literal else 'value'))
            # copy context: inside a THEN body or inside an arm of an event WHEN
            copy_ctx = None; q = u.parent[k]
            while q >= 0:
                g = u.kind_of_group(q)
                if g == 'THEN': copy_ctx = 'THEN body'; break
                if g in ('WHEN',) and u.tt(q - 2) == '|>':
                    # which selector?
                    oo, iit = u.containing_item(q - 2)
                    if iit:
                        ss = iit[u.item_expr_start(iit)] if u.item_expr_start(iit) < len(iit) else iit[0]
                        segs2 = {toks[z].t for z in range(ss, q - 1) if toks[z].k == 'id'}
                        if ('THEN' in segs2) or (segs2 & EVENT_SEGS) or (segs2 & source_keys):
                            copy_ctx = 'event WHEN arm'; break
                q = u.parent[q]
            sel_reads = [r for r, _ in reads_in(u, sel_a, sel_b)[0]] if sel_b >= sel_a else []
            sel_calls = [c for c in reads_in(u, sel_a, sel_b)[1]] if sel_b >= sel_a else []
            def covered(r):
                return any(r == sr or r.startswith(sr + '.') for sr in sel_reads)
            sel_param = None
            fdef = u.func_of(k)
            if sel_simple and fdef and sel_path.split('.')[0] in fdef[1] and '.' not in sel_path: sel_param = (fdef[0], sel_path)
            arms = []
            for arm in u.items(k + 1):
                j = next((z for z, x in enumerate(arm) if toks[x].t == '=>'), None)
                if j is None: continue
                pat = arm[:j]; body_a = arm[j + 1] if j + 1 < len(arm) else None
                if body_a is None: continue
                body_b = u.span_end(arm[-1])
                binders = pattern_binders(u, pat)
                reads, calls = reads_in(u, body_a, body_b, binders)
                subj = [r for r, _ in reads if sel_simple and (r == sel_path or r.startswith(sel_path + '.'))]
                indep = [r for r, _ in reads if not (sel_simple and (r == sel_path or r.startswith(sel_path + '.')))]
                impure = [f'{m}/{nm}' if m else nm for m, nm, _ in calls if impure_call(u, m, nm)]
                bare_tag = len(pat) == 1 and toks[pat[0]].k == 'id' and toks[pat[0]].t[0].isupper()
                used = {r.split('.')[0] for r, _ in reads} | {x for x in u.text(body_a, body_b, 10**9).split()}
                unused_b = [b for b in binders if not any(toks[z].t == b or (toks[z].k == 'text' and any(p.split('.')[0] == b for p in toks[z].interp)) for z in range(body_a, body_b + 1))]
                indep_unc = [r for r in indep if not covered(r)]
                arms.append(dict(pat=u.text(pat[0], u.span_end(pat[-1]), 60), line=toks[pat[0]].line, binders=binders, indep_unc=sorted(set(indep_unc)),
                                 subj=subj, indep=sorted(set(indep)), impure=sorted(set(impure)), bare_tag=bare_tag,
                                 unused_binders=unused_b,
                                 body=u.text(body_a, body_b, 120)))
                for ub in unused_b:
                    R['unused_pattern_binder'].append(site(u, pat[0], binder=ub, pat=u.text(pat[0], u.span_end(pat[-1]), 60)))
                if subj and t == 'WHEN':
                    R['when_subject_read_arm'].append(site(u, pat[0], sel=sel_path, pat=arms[-1]['pat'], bare_tag=bare_tag, reads=sorted(set(subj))[:4]))
            indep_all = sorted({r for a in arms for r in a['indep']})
            indep_unc_all = sorted({r for a in arms for r in a['indep_unc']})
            impure_all = sorted({r for a in arms for r in a['impure']})
            role = role_of(u, k)
            rec = site(u, k, kw=t, sel=u.text(sel_a, sel_b, 100) if sel_b >= sel_a else '', sel_kind=sel_kind,
                       copy_ctx=copy_ctx, role=role, n_arms=len(arms), indep=indep_all[:12], impure=impure_all[:6],
                       func=(u.func_of(k) or (None,))[0], field=u.toplevel_field_of(k), indep_unc=indep_unc_all[:12], sel_param=sel_param,
                       commands_in_arms=[f'{m}/{nm}' for a in [None] for (m, nm, _z) in reads_in(u, k + 2, max(k + 2, u.match[k + 1] - 1))[1] if (m, nm) in COMMANDS],
                       arms=[dict(pat=a['pat'], body=a['body']) for a in arms][:6])
            if t == 'WHEN':
                R['when_all'].append(rec)
                cand = (sel_kind in ('value',)) and copy_ctx is None and (indep_all or impure_all)
                cand_eff = (sel_kind == 'effect_result') and copy_ctx is None and (indep_all or impure_all)
                if cand: R['when_while_candidate'].append(rec)
                if cand and (indep_unc_all or impure_all): R['when_while_candidate_refined'].append(rec)
                if sel_kind == 'value' and copy_ctx is None and not cand: R['when_value_pure_mapping'].append(rec)
                elif cand_eff: R['when_while_candidate_effect_selector'].append(rec)
                C[f'WHEN_sel_{sel_kind}'] += 1
            else:
                R['while_all'].append(rec)
        # ------------------------------------------------ THEN
        for k in range(1, n - 1):
            if toks[k].t != 'THEN' or toks[k + 1].t != '{' or toks[k - 1].t != '|>': continue
            C['THEN'] += 1
            o, it = u.containing_item(k - 1)
            if not it: continue
            s = it[u.item_expr_start(it)] if u.item_expr_start(it) < len(it) else it[0]
            sel_ts = [toks[q] for q in range(s, k - 1)]
            ids = [x.t for x in sel_ts if x.k == 'id']
            segs = set(ids)
            last = None
            if sel_ts and sel_ts[0].k == 'id':
                path, j = u.path_at(s)
                if j == k - 2: last = path.split('.')[-1]
            is_event = bool(segs & EVENT_SEGS) or bool(segs & source_keys) or 'THEN' in segs
            lit = bool(sel_ts) and all(x.k in ('num', 'text') or (x.k == 'id' and x.t[0].isupper()) for x in sel_ts)
            if lit: R['then_over_constant'].append(site(u, k, sel=u.text(s, k - 2)))
            if last and last in hold_fields and not is_event:
                R['then_over_hold_same_file'].append(site(u, k, sel=u.text(s, k - 2)))
            if not is_event and not lit:
                R['then_over_non_event'].append(site(u, k, sel=u.text(s, k - 2, 80), last=last, hold=bool(last and last in hold_fields)))
        # ------------------------------------------------ LATEST
        for k in range(n - 1):
            if toks[k].t != 'LATEST' or toks[k + 1].t != '{': continue
            C['LATEST'] += 1
            arms = u.items(k + 1)
            key, kit = u.field_key_of(k)
            kinds = []
            for arm in arms:
                a, b = arm[0], u.span_end(arm[-1])
                ts = [toks[q] for q in range(a, b + 1)]
                tset = {x.t for x in ts}
                if 'THEN' in tset or 'WHEN' in tset and (tset & EVENT_SEGS): kinds.append('event')
                elif all(x.k in ('num', 'text') or (x.k == 'id' and x.t[0].isupper()) or x.t in ('(', ')', '[', ']', ',', ':', '/', '{', '}') or (x.k == 'id' and toks[max(0, a)].k == 'id') and False for x in ts):
                    kinds.append('constant')
                elif (tset & EVENT_SEGS) or (tset & source_keys): kinds.append('event_path')
                elif len(ts) >= 3 and ts[0].k == 'id' and ts[0].t[0].isupper() and ts[1].t == '/' and all(
                        x.k in ('num', 'text', 'op') or (x.k == 'id' and x.t[0].isupper()) or (x.k == 'id' and toks[max(0, a)].t == x.t) or x.t == ts[2].t for x in ts):
                    kinds.append('constant')  # e.g. Text/empty()
                else: kinds.append('value')
            d = site(u, k, field=key, n_arms=len(arms), kinds=kinds, text=u.text(k, u.match[k + 1] if u.match[k + 1] > 0 else k, 140))
            R['latest_all'].append(d)
            if len(arms) == 1: R['latest_one_input'].append(d)
            if 'constant' in kinds: R['latest_constant_arm'].append(d)
            starting = sum(1 for x in kinds if x in ('constant', 'value'))
            if starting >= 2: R['latest_two_starting_arms'].append(d)
            if 'value' in kinds: R['latest_bare_value_arm'].append(d)
            # self reference: bare `key` or `*.key` path read inside, not through a HOLD binder
            if key:
                inner = range(k + 2, u.match[k + 1] if u.match[k + 1] > 0 else k + 2)
                binders = local_binders_in(u, k + 2, max(k + 2, u.match[k + 1] - 1))
                hits = []
                for q in inner:
                    if toks[q].t == key and toks[q].k == 'id' and u.tt(q + 1) not in (':', '(') and key not in binders:
                        if u.tt(q - 1) != '.' or (u.tt(q - 1) == '.' and u.tt(q - 2) in ('store',)):
                            hits.append(toks[q].line)
                        elif u.tt(q - 1) == '.':
                            hits.append(toks[q].line)
                # enclosing HOLD with the same binder name?
                hb = False; qq = u.parent[k]
                while qq >= 0:
                    if u.kind_of_group(qq) == 'HOLD' and u.tt(qq - 1) == key: hb = True
                    qq = u.parent[qq]
                if hits and not hb:
                    R['latest_self_reference'].append(dict(d, hits=hits))
            # two arms with the same trigger path
            trig = []
            for arm in arms:
                a = arm[0]
                if toks[a].k == 'id':
                    p, j = u.path_at(a)
                    if u.tt(j + 1) == '|>' and u.tt(j + 2) == 'THEN': trig.append(p)
            dup = [p for p, c in collections.Counter(trig).items() if c > 1]
            if dup: R['latest_same_trigger_arms'].append(dict(d, dup=dup))
        # ------------------------------------------------ HOLD
        for k in range(1, n - 2):
            if toks[k].t != 'HOLD' or toks[k - 1].t != '|>': continue
            C['HOLD'] += 1
            o, it = u.containing_item(k - 1)
            s = it[u.item_expr_start(it)] if it and u.item_expr_start(it) < len(it) else k - 1
            init = [toks[q].t for q in range(s, k - 1)]
            body_o = k + 2 if u.tt(k + 2) == '{' else -1
            body = [toks[q].t for q in range(body_o, u.match[body_o])] if body_o > 0 and u.match[body_o] > 0 else []
            coll = [x for x in ('LIST', 'SET', 'MAP') if x in init] + (['List/'] if 'List' in init else []) + (['Map/'] if 'Map' in init else []) + (['Set/'] if 'Set' in init else [])
            body_coll = [x for x in ('LIST', 'SET', 'MAP') if x in body] + (['List/'] if 'List' in body else [])
            if coll or body_coll:
                R['collection_in_hold'].append(site(u, k, init=' '.join(init)[:100], init_coll=coll, body_coll=body_coll, binder=u.tt(k + 1)))
            bset = set(body)
            if bset & {'blur', 'focus', 'focused', 'hover', 'hovered', 'pointer_enter', 'pointer_leave', 'mouse_enter', 'mouse_leave', 'pressed', 'pointer'}:
                R['host_value_mirrored_in_hold'].append(site(u, k, key=u.field_key_of(k)[0], hits=sorted(bset & {'blur', 'focus', 'focused', 'hover', 'hovered', 'pointer_enter', 'pointer_leave', 'mouse_enter', 'mouse_leave', 'pressed', 'pointer'})))
        # ------------------------------------------------ state inside copy contexts (D30 placement)
        for k in range(n):
            t = toks[k].t
            is_state = (t == 'HOLD' and u.tt(k - 1) == '|>') or (t == 'SOURCE') or (t == 'toggle' and u.tt(k - 2) == 'Bool')
            is_state_call = False; callee = None
            if toks[k].k == 'id' and u.tt(k + 1) == '(' and u.tt(k - 1) != 'FUNCTION':
                mod = u.tt(k - 2) if u.tt(k - 1) == '/' else None
                for (cu, cf, cbase) in funcs_by_example[u.example].get(t, []):
                    if (mod is None or mod == cbase) and stateful[fkey(cu, cf)]: is_state_call = True; callee = t
            if not (is_state or is_state_call): continue
            q = u.parent[k]; ctx = None
            while q >= 0:
                g = u.kind_of_group(q)
                if g == 'THEN': ctx = 'THEN'; break
                if g == 'WHEN': ctx = 'WHEN arm'; break
                if g == 'FUNC': break
                q = u.parent[q]
            if ctx:
                R['state_in_copy_context'].append(site(u, k, what=('call ' + callee) if is_state_call and not is_state else t, ctx=ctx, role=role_of(u, k)))
        # ------------------------------------------------ commands outside copy contexts (D31)
        for k in range(2, n):
            if toks[k].t == '(' and u.tt(k - 2) == '/' and (u.tt(k - 3), u.tt(k - 1)) in COMMANDS:
                q = u.parent[k]; ctx = None
                while q >= 0:
                    g = u.kind_of_group(q)
                    if g in ('THEN', 'WHEN'): ctx = g; break
                    q = u.parent[q]
                R['command_calls'].append(site(u, k, call=f'{u.tt(k-3)}/{u.tt(k-1)}', ctx=ctx or 'live'))
        # ------------------------------------------------ misc greps
        for k in range(n):
            t = toks[k].t
            if t == 'catch_cycle' and u.tt(k - 2) == 'Dependency': R['dependency_catch_cycle'].append(site(u, k))
            if t == 'request' and u.tt(k - 2) == 'Http' and u.tt(k - 1) == '/': R['http_request'].append(site(u, k))
            if t == 'latest' and u.tt(k - 2) == 'List' and u.tt(k - 1) == '/':
                o, it = u.containing_item(k)
                txt = u.text(it[0], u.span_end(it[-1]), 10**9).split() if it else []
                R['list_latest'].append(site(u, k, after_map=('map' in txt and 'new' in txt)))
            if t == 'FUNCTION' and u.parent[k] >= 0: R['nested_function'].append(site(u, k, name=u.tt(k + 1)))
            if toks[k].k == 'id' and u.tt(k + 1) == ':' and u.kind_of_group(u.parent[k]) in ('record', 'tagged', 'root', 'BLOCK', 'call'):
                if t in ('event', 'events', 'hovered', 'focused', 'pressed'): C[f'key_{t}'] += 1; per_example[u.example][f'key_{t}'] += 1
            if t == 'Theme' and u.tt(k + 1) == '/' and u.example == 'todo_mvc_physical': C['todo_theme_calls'] += 1; per_example[u.name]['Theme/'] += 1
            if t == 'NovyTheme' and u.tt(k + 1) == '/':
                C[f'NovyTheme/{u.tt(k+2)}'] += 1
        # ------------------------------------------------ operator mixing (D11) and comparison chains (D18)
        seen = set()
        for o in [-1] + [i for i in range(n) if toks[i].t in OPEN]:
            if u.match[o] < 0 and o >= 0: continue
            for it in u.items(o):
                st = u.item_expr_start(it)
                # split the item further at `|>` (structural) and `=>`
                seg = []; segs_ = []
                for q in it[st:]:
                    if toks[q].t in ('|>', '=>', ':'):
                        segs_.append(seg); seg = []
                    else: seg.append(q)
                segs_.append(seg)
                for sg in segs_:
                    ops = []
                    for z, q in enumerate(sg):
                        if toks[q].t in BIN_OPS:
                            if toks[q].t == '-' and (z == 0 or toks[sg[z - 1]].t in BIN_OPS): continue  # unary
                            if toks[q].t == '/' and (z > 0 and toks[sg[z - 1]].k == 'id' and toks[sg[z - 1]].t[0].isupper()): continue  # Module/fn
                            if toks[q].t == '/' and z + 1 < len(sg) and toks[sg[z + 1]].k == 'id' and toks[sg[z + 1]].t[0].isupper(): continue
                            ops.append(toks[q].t)
                    if len(set(ops)) >= 2 and sg[0] not in seen:
                        seen.add(sg[0])
                        R['mixed_operators'].append(site(u, sg[0], ops=ops, text=u.text(sg[0], u.span_end(sg[-1]), 100)))
                    if sum(1 for x in ops if x in CMP_OPS) >= 2:
                        R['comparison_chain'].append(site(u, sg[0], ops=ops, text=u.text(sg[0], u.span_end(sg[-1]), 100)))
                # binary op directly followed by |> in same item (a + b |> f): informational
                ops_before_pipe = False
                for q in it[st:]:
                    if toks[q].t in CMP_OPS or toks[q].t in ('+', '*'): ops_before_pipe = True
                    if toks[q].t == '|>' and ops_before_pipe:
                        R['binop_then_pipe'].append(site(u, it[st], text=u.text(it[st], u.span_end(it[-1]), 100))); break
        # ------------------------------------------------ unused FUNCTION params (D24)
        for (name, params, b, e, h) in u.funcs:
            body_ids = set()
            for q in range(b, e):
                if toks[q].k == 'id': body_ids.add(toks[q].t)
                if toks[q].k == 'text':
                    for p in toks[q].interp: body_ids.add(p.split('.')[0])
            for p in params:
                if p not in body_ids: R['unused_param'].append(site(u, h, func=name, param=p))
        # ------------------------------------------------ [x: x] own-name copies (D14)
        for k in range(n - 3):
            if toks[k].k == 'id' and toks[k + 1].t == ':' and toks[k + 2].t == toks[k].t and u.tt(k + 3) in (',', ']', ')') and u.kind_of_group(u.parent[k]) in ('record', 'tagged'):
                R['own_name_copy'].append(site(u, k, name=toks[k].t))
        # ------------------------------------------------ D23: user FUNCTION call sites passing records
        for k in range(n - 1):
            if toks[k].k != 'id' or u.tt(k + 1) != '(' or u.tt(k - 1) == 'FUNCTION': continue
            mod = u.tt(k - 2) if u.tt(k - 1) == '/' else None
            if mod is not None and mod not in user_modules[u.example]: continue
            cands = [(cu, cf, cb) for (cu, cf, cb) in funcs_by_example[u.example].get(toks[k].t, []) if mod is None or mod == cb]
            if not cands: continue
            cu, cf, cb = cands[0]
            p = k + 1
            for arg in u.items(p):
                if len(arg) < 3 or u.tt(arg[1]) != ':': continue
                label = toks[arg[0]].t
                if label not in cf[1]: continue
                # how is the param used in the callee?
                fields_read = set(); forwarded = False; used_bare = False
                for q in range(cf[2], cf[3]):
                    if cu.toks[q].t == label and cu.toks[q].k == 'id' and cu.tt(q - 1) != '.' and cu.tt(q + 1) != ':':
                        if cu.tt(q + 1) == '.' and q + 2 < len(cu.toks): fields_read.add(cu.toks[q + 2].t)
                        else: used_bare = True
                    if cu.toks[q].k == 'text':
                        for pp in cu.toks[q].interp:
                            if pp.split('.')[0] == label and '.' in pp: fields_read.add(pp.split('.')[1])
                if not fields_read: continue  # not used as a record (or only forwarded/bare)
                v = arg[2:]
                vt = toks[v[0]]
                if vt.t == '[' and len(v) == 1:
                    keys = u.keys_defined(v[0])
                    extra = sorted(keys - fields_read)
                    spreads = any(toks[q].t == '...' and u.parent[q] == v[0] for q in range(v[0], u.match[v[0]]))
                    R['d23_literal_record_arg'].append(site(u, k, func=toks[k].t, param=label, keys=sorted(keys), read=sorted(fields_read),
                                                            extra=extra, callee_forwards_bare=used_bare, spread=spreads))
                elif vt.k == 'id' and vt.t[0].islower() and u.path_at(v[0])[1] == u.span_end(v[-1]):
                    R['d23_variable_record_arg'].append(site(u, k, func=toks[k].t, param=label, value=u.path_at(v[0])[0], read=sorted(fields_read), callee_forwards_bare=used_bare))
        # ------------------------------------------------ D18.4: tag used bare and as tagged object in one unit
        bare_tags = collections.Counter(); tagged_tags = collections.Counter()
        for k in range(n):
            if toks[k].k == 'id' and toks[k].t[0].isupper() and u.tt(k + 1) != '/' and u.tt(k - 1) != '/':
                if u.tt(k + 1) == '[' and toks[k + 1].line == toks[k].line: tagged_tags[toks[k].t] += 1
                elif toks[k].t not in ('True', 'False', 'SKIP', 'TEXT{}', 'LIST', 'SET', 'MAP', 'BLOCK', 'LATEST', 'HOLD', 'WHEN', 'WHILE', 'THEN', 'FUNCTION', 'SOURCE', 'PASSED', 'PASS', 'FLUSH', 'Oklch', 'BITS', 'BYTES', 'DRAIN', 'DRAINING', 'OUT', 'TEXT'):
                    bare_tags[toks[k].t] += 1
        for tg in set(bare_tags) & set(tagged_tags):
            R['tag_bare_and_tagged_same_unit'].append(dict(file=u.name, example=u.example, tag=tg, bare=bare_tags[tg], tagged=tagged_tags[tg]))
        # ------------------------------------------------ suffix-resolved bare names (D14), RUN.bn and entry files only
        if u.origin == 'bn':
            defined_anywhere = collections.defaultdict(int)
            for k in range(n - 1):
                if toks[k].k == 'id' and toks[k + 1].t == ':': defined_anywhere[toks[k].t] += 1
            module_keys = u.keys_defined(-1)
            fnames = {f[0] for f in u.funcs}
            for k in range(n):
                tk = toks[k]
                if tk.k != 'id' or not (tk.t[0].islower() or tk.t == '_x') or u.tt(k - 1) in ('.', '/', 'FUNCTION', 'HOLD') or u.tt(k + 1) in ('/', '('):
                    continue
                if u.tt(k + 1) == ':' and u.kind_of_group(u.parent[k]) in ('record', 'tagged', 'root', 'BLOCK', 'call', 'paren'): continue
                name = tk.t
                if u.kind_of_group(u.parent[k]) == 'paren' and u.tt(u.parent[k] - 2) == 'FUNCTION': continue
                if u.kind_of_group(u.parent[k]) == 'call' and u.tt(u.parent[k] - 2) == 'FUNCTION': continue
                if name in module_keys or name in fnames: continue
                # lexical scopes
                found = False; q = u.parent[k]; child = k
                while q >= 0 and not found:
                    g = u.kind_of_group(q)
                    own = None
                    for _it in u.items(q):
                        if _it[0] <= child <= u.span_end(_it[-1]) and len(_it) >= 2 and u.tt(_it[1]) == ':': own = u.tt(_it[0])
                    if g in ('record', 'tagged', 'BLOCK') and name in u.keys_defined(q) and name != own: found = True
                    if g == 'HOLD' and u.tt(q - 1) == name: found = True
                    if g == 'call' and u.tt(q - 2) == '/' and u.tt(q - 1) in LIST_BINDER_FNS and u.tt(q + 1) == name: found = True
                    if g == 'FUNC':
                        f = u.func_of(k)
                        if f and name in f[1]: found = True
                    if g == 'call' and name in u.keys_defined(q) and name != own and not found:
                        R['call_sibling_read'].append(site(u, k, name=name, call=u.tt(q - 1), role=role_of(u, k))); found = True; break
                    if g in ('WHEN', 'WHILE'):
                        for arm in u.items(q):
                            j = next((z for z, x in enumerate(arm) if toks[x].t == '=>'), None)
                            if j is not None and arm[0] <= k <= u.span_end(arm[-1]) and name in pattern_binders(u, arm[:j]): found = True
                    child = q; q = u.parent[q]
                if found: continue
                # not lexically visible: is it a pattern binder itself / list binder being declared?
                o, it = u.containing_item(k)
                if it and any(toks[x].t == '=>' for x in it) and k < next(x for x in it if toks[x].t == '=>'): continue
                if u.kind_of_group(u.parent[k]) == 'call' and u.tt(u.parent[k] - 2) == '/' and u.tt(u.parent[k] + 1) == name: continue
                if u.kind_of_group(u.parent[k]) in ('tagged',) and u.tt(k + 1) in (',', ']') and any(toks[x].t == '=>' for x in u.containing_item(u.parent[k])[1] or []):
                    continue
                if defined_anywhere[name]:
                    R['suffix_resolved_name'].append(site(u, k, name=name, defs=defined_anywhere[name], role=role_of(u, k)))
                else:
                    R['unresolved_name'].append(site(u, k, name=name))

    # ---------------------------------------------------------------- summarize
    summ = collections.OrderedDict()
    summ['corpus'] = {k: v for k, v in C.items() if k.startswith(('units_', 'lines_'))}
    summ['rust_files_with_boon'] = len(rust_hits)
    summ['rust_files'] = dict(rust_hits.most_common())
    for key in ('WHEN', 'WHILE', 'THEN', 'LATEST', 'HOLD'):
        summ[key] = C[key]
    for key in sorted(k for k in C if k.startswith(('WHEN_sel_', 'key_', 'NovyTheme/', 'todo_theme'))):
        summ[key] = C[key]
    for cls, sites in R.items():
        by_origin = collections.Counter(s.get('origin','bn') for s in sites)
        by_ex = collections.Counter(s['example'] for s in sites)
        summ['class:' + cls] = dict(total=len(sites), by_origin=dict(by_origin), top_examples=dict(by_ex.most_common(12)))
    # WHEN->WHILE detail
    wc = R['when_while_candidate']
    summ['when_candidate_by_role'] = dict(collections.Counter(s['role'] for s in wc))
    summ['when_candidate_by_example_role'] = {ex: dict(collections.Counter(s['role'] for s in wc if s['example'] == ex)) for ex in sorted({s['example'] for s in wc})}
    summ['when_all_by_example'] = dict(collections.Counter(s['example'] for s in R['when_all']).most_common())
    summ['when_by_sel_kind_and_ctx'] = dict(collections.Counter(f"{s['sel_kind']}|{s['copy_ctx']}" for s in R['when_all']))
    summ['when_candidate_reason'] = dict(collections.Counter(('indep' if s['indep'] else '') + ('+impure' if s['impure'] else '') for s in wc))
    summ['when_subject_read_bare_tag_arms'] = sum(1 for s in R['when_subject_read_arm'] if s['bare_tag'])
    summ['list_latest_after_map'] = sum(1 for s in R['list_latest'] if s['after_map'])
    summ['d23_literal_with_extra'] = sum(1 for s in R['d23_literal_record_arg'] if s['extra'] and not s['callee_forwards_bare'])
    summ['d23_literal_with_extra_or_forwarded'] = sum(1 for s in R['d23_literal_record_arg'] if s['extra'])
    summ['d23_variable_args'] = len(R['d23_variable_record_arg'])
    summ['commands_live'] = sum(1 for s in R['command_calls'] if s['ctx'] == 'live')
    ps = collections.Counter()
    for rec in R['when_all']:
        if rec.get('sel_param') and rec['sel_kind'] == 'value':
            c = arg_lit[(rec['example'], rec['sel_param'][0], rec['sel_param'][1])]
            kind = 'no_calls' if not c else ('all_literal' if c['nonlit'] == 0 else ('all_nonliteral' if c['lit'] == 0 else 'mixed'))
            rec['param_arg_literalness'] = dict(c)
            ps[kind] += 1
            if kind == 'all_literal': R['when_over_constant_bound_param'].append(rec)
    summ['when_param_selector_call_literalness'] = dict(ps)
    cc = collections.Counter()
    for rec in R['when_all']:
        if rec.get('commands_in_arms'): cc[f"{rec['sel_kind']}|{rec['copy_ctx']}"] += 1
    summ['when_with_command_in_arms'] = dict(cc)
    json.dump(dict(summary=summ, sites=R), open(os.path.join(OUT, 'census_results.json'), 'w'), indent=1, default=str)
    print(json.dumps(summ, indent=1, default=str))

if __name__ == '__main__':
    main()
