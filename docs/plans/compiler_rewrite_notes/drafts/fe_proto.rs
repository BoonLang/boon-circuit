// Standalone calibration prototype for the proposed Boon front end (NOT repo code).
// Single-pass lexer (byte classes, TEXT raw mode, session interner) + newline-aware
// Pratt/recursive-descent parser into a flat SoA arena + per-definition fingerprints.
// Layout rule under test (indentation is validated, never used for structure):
//   A newline ends the current element unless the previous token is one of
//   ( [ { : , |> =>   or the next line starts with ) ] } , |> => or a binary
//   operator other than `-`.
use std::collections::HashMap;
use std::time::Instant;

// ---------------- interner ----------------
#[inline]
fn fxhash(bytes: &[u8]) -> u64 {
    const K: u64 = 0x517cc1b727220a95;
    let mut h: u64 = 0;
    let mut chunks = bytes.chunks_exact(8);
    for c in &mut chunks {
        let w = u64::from_le_bytes(c.try_into().unwrap());
        h = (h.rotate_left(5) ^ w).wrapping_mul(K);
    }
    let r = chunks.remainder();
    if !r.is_empty() {
        let mut b = [0u8; 8];
        b[..r.len()].copy_from_slice(r);
        h = (h.rotate_left(5) ^ u64::from_le_bytes(b)).wrapping_mul(K);
    }
    (h.rotate_left(5) ^ bytes.len() as u64).wrapping_mul(K)
}
struct Interner { table: Vec<u32>, hashes: Vec<u64>, starts: Vec<u32>, arena: Vec<u8>, mask: usize }
impl Interner {
    fn new() -> Self { let cap = 1 << 14; Interner { table: vec![u32::MAX; cap], hashes: vec![], starts: vec![0], arena: vec![], mask: cap - 1 } }
    #[inline]
    fn intern(&mut self, s: &[u8]) -> u32 {
        let h = fxhash(s);
        let mut i = (h as usize) & self.mask;
        loop {
            let e = self.table[i];
            if e == u32::MAX {
                let id = self.hashes.len() as u32;
                self.hashes.push(h); self.arena.extend_from_slice(s); self.starts.push(self.arena.len() as u32);
                self.table[i] = id;
                if self.hashes.len() * 2 > self.table.len() { self.grow(); }
                return id;
            }
            let e = e as usize;
            if self.hashes[e] == h && &self.arena[self.starts[e] as usize..self.starts[e + 1] as usize] == s { return e as u32; }
            i = (i + 1) & self.mask;
        }
    }
    fn grow(&mut self) {
        let cap = self.table.len() * 2; self.table = vec![u32::MAX; cap]; self.mask = cap - 1;
        for (id, h) in self.hashes.iter().enumerate() { let mut i = (*h as usize) & self.mask; while self.table[i] != u32::MAX { i = (i + 1) & self.mask; } self.table[i] = id as u32; }
    }
    fn get(&self, id: u32) -> &str { std::str::from_utf8(&self.arena[self.starts[id as usize] as usize..self.starts[id as usize + 1] as usize]).unwrap() }
}

// ---------------- tokens ----------------
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
enum T {
    Ident, Upper, Kw, Num, Str, LParen, RParen, LBrack, RBrack, LBrace, RBrace, Colon, Comma, Dot, Ellipsis,
    Pipe, Arrow, Plus, Minus, Star, Slash, Percent, EqEq, NotEq, Lt, Gt, Le, Ge,
    TextOpen, TextChunk, Interp, TextClose, Err, Eof,
}
// keywords (payload = kw id)
const KW: &[&str] = &["FUNCTION", "LIST", "MAP", "SET", "BYTES", "BITS", "HOLD", "THEN", "WHEN", "WHILE", "LATEST", "BLOCK", "SOURCE", "SKIP", "FLUSH", "PASS", "PASSED", "OUT", "DRAIN", "DRAINING", "TEXT"];
const K_FUNCTION: u32 = 0; const K_LIST: u32 = 1; const K_MAP: u32 = 2; const K_SET: u32 = 3; const K_BYTES: u32 = 4; const K_BITS: u32 = 5;
const K_HOLD: u32 = 6; const K_THEN: u32 = 7; const K_WHEN: u32 = 8; const K_WHILE: u32 = 9; const K_LATEST: u32 = 10; const K_BLOCK: u32 = 11;
const K_SOURCE: u32 = 12; const K_SKIP: u32 = 13; const K_FLUSH: u32 = 14; const K_PASS: u32 = 15; const K_PASSED: u32 = 16; const K_OUT: u32 = 17;
const K_DRAIN: u32 = 18; const K_DRAINING: u32 = 19;

const NL: u8 = 1; // first token on its line

#[derive(Default)]
struct Toks { kind: Vec<T>, start: Vec<u32>, end: Vec<u32>, pay: Vec<u32>, flags: Vec<u8>, indent: Vec<u16>, comments: u32, line_starts: Vec<u32> }

fn kw_of(s: &[u8]) -> Option<u32> {
    if s.len() < 3 || !s[0].is_ascii_uppercase() || !s[1].is_ascii_uppercase() { return None; }
    KW.iter().position(|k| k.as_bytes() == s).map(|p| p as u32)
}

fn lex(src: &[u8], int: &mut Interner, t: &mut Toks) {
    let n = src.len();
    let mut i = 0usize;
    let mut at_line_start = true;
    let mut indent: u16 = 0;
    t.line_starts.push(0);
    macro_rules! push { ($k:expr, $s:expr, $e:expr, $p:expr) => {{
        t.kind.push($k); t.start.push($s as u32); t.end.push($e as u32); t.pay.push($p);
        t.flags.push(if at_line_start { NL } else { 0 }); t.indent.push(indent); at_line_start = false;
    }}}
    while i < n {
        let b = src[i];
        match b {
            b' ' | b'\t' | b'\r' => { i += 1; if at_line_start { indent += 1; } }
            b'\n' => { i += 1; at_line_start = true; indent = 0; t.line_starts.push(i as u32); }
            b'-' if i + 1 < n && src[i + 1] == b'-' => { t.comments += 1; while i < n && src[i] != b'\n' { i += 1; } }
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                let s = i; i += 1;
                while i < n && (src[i].is_ascii_alphanumeric() || src[i] == b'_' || src[i] == b'-' || src[i] == b'/') { i += 1; }
                let w = &src[s..i];
                if let Some(k) = kw_of(w) {
                    if k == 20 { // TEXT
                        let mut j = i; while j < n && (src[j] == b' ' || src[j] == b'\t') { j += 1; }
                        if j < n && src[j] == b'{' {
                            push!(T::TextOpen, s, j + 1, 0);
                            i = j + 1;
                            // raw mode
                            let mut chunk = i; let mut depth = 0i32;
                            loop {
                                if i >= n { push!(T::Err, chunk, n, 0); break; }
                                let c = src[i];
                                if c == b'\n' { t.line_starts.push(i as u32 + 1); }
                                if c == b'{' {
                                    if i > chunk { push!(T::TextChunk, chunk, i, 0); }
                                    // interpolation: $?path until '}'
                                    let is = i + 1; let mut j2 = is;
                                    while j2 < n && src[j2] != b'}' && src[j2] != b'\n' { j2 += 1; }
                                    let raw = trim(&src[is..j2]);
                                    let sym = int.intern(raw);
                                    push!(T::Interp, is, j2, sym);
                                    i = if j2 < n { j2 + 1 } else { j2 }; chunk = i; let _ = depth; depth = 0; continue;
                                }
                                if c == b'}' {
                                    if i > chunk { push!(T::TextChunk, chunk, i, 0); }
                                    push!(T::TextClose, i, i + 1, 0); i += 1; break;
                                }
                                i += 1;
                            }
                            continue;
                        }
                    }
                    push!(T::Kw, s, i, k);
                } else if w[0].is_ascii_uppercase() && !w.contains(&b'/') {
                    let sym = int.intern(w); push!(T::Upper, s, i, sym);
                } else { let sym = int.intern(w); push!(T::Ident, s, i, sym); }
            }
            b'0'..=b'9' => {
                let s = i; while i < n && src[i].is_ascii_digit() { i += 1; }
                if i + 1 < n && src[i] == b'.' && src[i + 1].is_ascii_digit() { i += 1; while i < n && src[i].is_ascii_digit() { i += 1; } }
                // radix / byte / bits suffix glued: 16uFF, 8u7, 2u0101
                if i < n && (src[i] == b'u' || src[i] == b's' || src[i] == b'b') && i + 1 < n && src[i + 1].is_ascii_alphanumeric() {
                    while i < n && (src[i].is_ascii_alphanumeric() || src[i] == b'_') { i += 1; }
                }
                push!(T::Num, s, i, 0);
            }
            b'"' => {
                let s = i; i += 1;
                while i < n && src[i] != b'"' { if src[i] == b'\\' { i += 1; } if i < n && src[i] == b'\n' { t.line_starts.push(i as u32 + 1); } i += 1; }
                i = (i + 1).min(n); push!(T::Str, s, i, 0);
            }
            _ => {
                let s = i;
                let two = if i + 1 < n { src[i + 1] } else { 0 };
                let (k, len) = match (b, two) {
                    (b'|', b'>') => (T::Pipe, 2), (b'=', b'>') => (T::Arrow, 2), (b'=', b'=') => (T::EqEq, 2),
                    (b'!', b'=') => (T::NotEq, 2), (b'<', b'=') => (T::Le, 2), (b'>', b'=') => (T::Ge, 2),
                    (b'.', b'.') if i + 2 < n && src[i + 2] == b'.' => (T::Ellipsis, 3),
                    (b'(', _) => (T::LParen, 1), (b')', _) => (T::RParen, 1), (b'[', _) => (T::LBrack, 1), (b']', _) => (T::RBrack, 1),
                    (b'{', _) => (T::LBrace, 1), (b'}', _) => (T::RBrace, 1), (b':', _) => (T::Colon, 1), (b',', _) => (T::Comma, 1),
                    (b'.', _) => (T::Dot, 1), (b'+', _) => (T::Plus, 1), (b'-', _) => (T::Minus, 1), (b'*', _) => (T::Star, 1),
                    (b'/', _) => (T::Slash, 1), (b'%', _) => (T::Percent, 1), (b'<', _) => (T::Lt, 1), (b'>', _) => (T::Gt, 1),
                    _ => (T::Err, if b < 0x80 { 1 } else { utf8_len(b) }),
                };
                i += len; push!(k, s, i, 0);
            }
        }
    }
    push!(T::Eof, n, n, 0);
    t.flags.last_mut().map(|f| *f |= NL);
}
fn utf8_len(b: u8) -> usize { if b >= 0xF0 { 4 } else if b >= 0xE0 { 3 } else { 2 } }
fn trim(s: &[u8]) -> &[u8] { let mut a = 0; let mut b = s.len(); while a < b && (s[a] == b' ' || s[a] == b'\t') { a += 1; } while b > a && (s[b - 1] == b' ' || s[b - 1] == b'\t') { b -= 1; } &s[a..b] }

// ---------------- nodes ----------------
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
enum N {
    Ident, Tag, Path, Num, Str, Text, TextTpl, TextPart, Interp, Source, Skip, Flush,
    Record, TaggedRecord, Field, Spread, List, Map, Set, Bytes, Bits, MapEntry, Latest, Block, Binding,
    Call, Arg, PassArg, PipeCall, When, While, Arm, Then, Hold, Draining, PipeField, FieldAccess,
    Binary, Neg, Function, Param, Item, Wildcard, Error, Passed,
}
#[derive(Default)]
struct Ast { kind: Vec<N>, a: Vec<u32>, b: Vec<u32>, start: Vec<u32>, end: Vec<u32>, extra: Vec<u32> }
impl Ast {
    #[inline]
    fn push(&mut self, k: N, a: u32, b: u32, s: u32, e: u32) -> u32 {
        let id = self.kind.len() as u32; self.kind.push(k); self.a.push(a); self.b.push(b); self.start.push(s); self.end.push(e); id
    }
}

struct Diag { at: u32, msg: &'static str }
struct P<'a> { t: &'a Toks, i: usize, ast: Ast, diags: Vec<Diag>, list: Vec<u32>, items: Vec<(u32, u32, u32)>, ctx: u8, lints: Vec<(u32, &'static str)> }

#[inline] fn is_binop(k: T) -> bool { matches!(k, T::Plus | T::Minus | T::Star | T::Slash | T::Percent | T::EqEq | T::NotEq | T::Lt | T::Gt | T::Le | T::Ge) }
#[inline] fn prec(k: T) -> u8 { match k { T::Star | T::Slash | T::Percent => 30, T::Plus | T::Minus => 20, T::EqEq | T::NotEq | T::Lt | T::Gt | T::Le | T::Ge => 10, _ => 0 } }
// newline before `k` does not end the expression
#[inline] fn continues_line(k: T) -> bool { matches!(k, T::Pipe | T::Arrow | T::Comma | T::RParen | T::RBrack | T::RBrace) || (is_binop(k) && k != T::Minus) }
// newline after `k` does not end the expression
#[inline] fn open_end(k: T) -> bool { matches!(k, T::LParen | T::LBrack | T::LBrace | T::Colon | T::Comma | T::Pipe | T::Arrow) }

impl<'a> P<'a> {
    #[inline] fn k(&self) -> T { self.t.kind[self.i] }
    #[inline] fn kn(&self, d: usize) -> T { self.t.kind[(self.i + d).min(self.t.kind.len() - 1)] }
    #[inline] fn nl(&self) -> bool { self.t.flags[self.i] & NL != 0 }
    #[inline] fn s(&self) -> u32 { self.t.start[self.i] }
    #[inline] fn pe(&self) -> u32 { if self.i == 0 { 0 } else { self.t.end[self.i - 1] } }
    #[inline] fn bump(&mut self) -> usize { let i = self.i; if self.t.kind[i] != T::Eof { self.i += 1; } i }
    #[inline] fn kw(&self, k: u32) -> bool { self.k() == T::Kw && self.t.pay[self.i] == k }
    fn err(&mut self, msg: &'static str) -> u32 {
        self.diags.push(Diag { at: self.s(), msg });
        let s = self.s(); self.ast.push(N::Error, 0, 0, s, s)
    }
    // does a separator (newline) stand between the previous token and the current one?
    #[inline] fn newline_sep(&self) -> bool { self.nl() && !continues_line(self.k()) && !(self.i > 0 && open_end(self.t.kind[self.i - 1])) }
    fn expect(&mut self, k: T, msg: &'static str) -> bool { if self.k() == k { self.bump(); true } else { self.err(msg); false } }

    // recovery: skip to next separator at depth 0 or the closer
    fn recover(&mut self, closer: T) {
        let mut depth = 0i32;
        loop {
            let k = self.k();
            if k == T::Eof { return; }
            if depth == 0 && (k == closer || k == T::Comma || (self.nl() && self.i > 0)) { return; }
            match k { T::LParen | T::LBrack | T::LBrace | T::TextOpen => depth += 1, T::RParen | T::RBrack | T::RBrace | T::TextClose => { depth -= 1; if depth < 0 { return; } } _ => {} }
            self.bump();
        }
    }

    // generic element list; returns (first extra index, count)
    fn elements(&mut self, closer: T, f: fn(&mut Self) -> u32) -> (u32, u32) {
        let my_ctx = self.ctx; self.ctx = 0;
        let base = self.list.len();
        loop {
            let k = self.k();
            if k == closer { self.bump(); break; }
            if k == T::Eof { self.err("unclosed bracket"); break; }
            if k == T::Comma { self.bump(); continue; }
            let before = self.i;
            let e = f(self);
            self.list.push(e);
            let k = self.k();
            if k == closer || k == T::Comma { continue; }
            if self.nl() { continue; }
            self.err("expected `,`, newline or closing bracket");
            self.recover(closer);
            if self.i == before { self.bump(); }
        }
        // lints (context-dependent)
        let ctx = my_ctx;
        let mut first_indent: Option<u16> = None;
        let elems: Vec<u32> = self.list[base..].to_vec();
        for (ix, &e) in elems.iter().enumerate() {
            let st = self.ast.start[e as usize];
            let ti = match self.t.start.binary_search(&st) { Ok(i) => i, Err(i) => i.min(self.t.kind.len()-1) };
            if self.t.flags[ti] & NL != 0 {
                let ind = self.t.indent[ti];
                match first_indent { None => first_indent = Some(ind), Some(f) if f != ind => self.lints.push((st, "inconsistent element indentation")), _ => {} }
            }
            let bare = self.ast.kind[e as usize] == N::Binding && self.ast.a[e as usize] == u32::MAX;
            if ctx == 1 && bare { self.lints.push((st, "bare expression in record")); }
            if ctx == 2 && bare && ix + 1 != elems.len() { self.lints.push((st, "bare expression before end of body")); }
            if ctx == 2 && !bare && ix + 1 == elems.len() && self.ast.kind[e as usize] != N::Function { self.lints.push((st, "body without result expression")); }
        }
        let start = self.ast.extra.len() as u32;
        let cnt = (self.list.len() - base) as u32;
        self.ast.extra.extend_from_slice(&self.list[base..]);
        self.list.truncate(base);
        (start, cnt)
    }

    // ---- top level ----
    fn unit(&mut self) {
        while self.k() != T::Eof {
            let ts = self.i as u32;
            let before = self.i;
            if !self.nl() || self.t.indent[self.i] != 0 { self.err("top-level item must start at column 0"); }
            let n = self.named_element(true);
            self.items.push((n, ts, self.i as u32));
            if self.k() != T::Eof && !self.nl() { self.err("expected newline after item"); self.recover(T::Eof); }
            if self.i == before { self.bump(); }
        }
    }
    // `name: expr` | FUNCTION ... | ...spread | bare expr (reported by caller context)
    fn named_element(&mut self, top: bool) -> u32 {
        let s = self.s();
        if self.kw(K_FUNCTION) { return self.function(); }
        if self.k() == T::Ellipsis { self.bump(); let v = self.expr(0); return self.ast.push(N::Spread, v, 0, s, self.pe()); }
        if matches!(self.k(), T::Ident | T::Upper | T::Kw) && self.kn(1) == T::Colon {
            let name = if self.k() == T::Kw { 1_000_000 + self.t.pay[self.i] } else { self.t.pay[self.i] };
            let head_indent = self.t.indent[self.i];
            self.bump(); self.bump();
            if self.nl() && self.t.indent[self.i] <= head_indent && !top { self.diags.push(Diag { at: self.s(), msg: "value on next line must be indented" }); }
            if self.nl() && top && self.t.indent[self.i] == 0 { self.diags.push(Diag { at: self.s(), msg: "value on next line must be indented" }); }
            let v = self.expr(0);
            return self.ast.push(N::Field, name, v, s, self.pe());
        }
        if top { let e = self.err("expected `name:` or FUNCTION"); self.recover(T::Eof); return e; }
        let v = self.expr(0);
        self.ast.push(N::Binding, u32::MAX, v, s, self.pe()) // bare expression element
    }
    fn function(&mut self) -> u32 {
        let s = self.s(); self.bump();
        let name = if matches!(self.k(), T::Ident) { let p = self.t.pay[self.i]; self.bump(); p } else { self.err("expected function name"); 0 };
        self.expect(T::LParen, "expected `(`");
        let params = self.elements(T::RParen, |p| { let s = p.s(); if p.k() == T::Ident { let n = p.t.pay[p.i]; p.bump(); if p.k() == T::Colon { p.bump(); if p.kw(K_OUT) { p.bump(); } else { p.err("expected OUT"); } } p.ast.push(N::Param, n, 0, s, p.pe()) } else { let e = p.err("expected parameter"); p.bump(); e } });
        let _ = params;
        self.expect(T::LBrace, "expected `{`");
        self.ctx = 2; let body = self.elements(T::RBrace, |p| p.named_element(false)); self.ctx = 0;
                let f = self.ast.push(N::Function, name, body.0, s, self.pe()); let _ = body.1; f
    }

    // ---- expressions ----
    fn expr(&mut self, min: u8) -> u32 {
        let s = self.s();
        let head_indent = self.t.indent[self.i];
        let mut lhs = self.unary();
        loop {
            let k = self.k();
            if self.nl() && !continues_line(k) { break; }
            if self.nl() && (k == T::Pipe || is_binop(k)) && self.t.indent[self.i] < head_indent { self.lints.push((self.s(), "continuation line is less indented than the expression it continues")); }
            if k == T::Pipe {
                if min > 5 { break; }
                self.bump();
                lhs = self.pipe_rhs(lhs, s);
                continue;
            }
            let p = prec(k);
            if p == 0 || p <= min { break; }
            self.bump();
            if self.nl() { self.err("operand must be on the same line as the operator"); }
            let rhs = self.expr(p);
            lhs = self.ast.push(N::Binary, lhs, rhs, s, self.pe());
            if p == 10 && prec(self.k()) == 10 && !self.nl() { self.err("comparison operators do not chain"); }
        }
        lhs
    }
    fn unary(&mut self) -> u32 {
        let s = self.s();
        if self.k() == T::Minus { self.bump(); let v = self.postfix(); return self.ast.push(N::Neg, v, 0, s, self.pe()); }
        self.postfix()
    }
    fn postfix(&mut self) -> u32 {
        let s = self.s();
        let mut e = self.primary();
        loop {
            if self.nl() { break; }
            match self.k() {
                T::Dot if matches!(self.kn(1), T::Ident | T::Upper | T::Kw) => { self.bump(); let f = self.t.pay[self.i]; self.bump(); e = self.ast.push(N::FieldAccess, e, f, s, self.pe()); }
                _ => break,
            }
        }
        e
    }
    fn args(&mut self) -> (u32, u32) {
        self.elements(T::RParen, |p| {
            let s = p.s();
            if p.kw(K_PASS) && p.kn(1) == T::Colon { p.bump(); p.bump(); let v = p.expr(0); return p.ast.push(N::PassArg, v, 0, s, p.pe()); }
            if matches!(p.k(), T::Ident | T::Upper | T::Kw) && p.kn(1) == T::Colon { let n = p.t.pay[p.i]; p.bump(); p.bump(); let v = p.expr(0); return p.ast.push(N::Arg, n, v, s, p.pe()); }
            let v = p.expr(0); p.ast.push(N::Arg, u32::MAX, v, s, p.pe())
        })
    }
    fn braced_expr(&mut self) -> u32 {
        // `{ expr }` with optional leading bindings (THEN/HOLD bodies)
        if !self.expect(T::LBrace, "expected `{`") { return 0; }
        self.ctx = 2; let (a, c) = self.elements(T::RBrace, |p| p.named_element(false)); self.ctx = 0;
        if c == 0 { 0 } else { self.ast.extra[(a + c - 1) as usize] }
    }
    fn arms(&mut self) -> (u32, u32) {
        if !self.expect(T::LBrace, "expected `{`") { return (0, 0); }
        self.elements(T::RBrace, |p| {
            let s = p.s();
            let pat = p.pattern();
            if !p.expect(T::Arrow, "expected `=>`") { p.recover(T::RBrace); return pat; }
            let v = p.expr(0);
            p.ast.push(N::Arm, pat, v, s, p.pe())
        })
    }
    fn pattern(&mut self) -> u32 {
        let s = self.s();
        match self.k() {
            T::Ident if self.t.pay[self.i] == 0 => { self.bump(); self.ast.push(N::Wildcard, 0, 0, s, self.pe()) }
            T::Upper if self.kn(1) == T::LBrack && !self.t.flags[self.i + 1] & NL != 0 => { let tag = self.t.pay[self.i]; self.bump(); self.bump(); let f = self.elements(T::RBrack, |p| { let s = p.s(); if matches!(p.k(), T::Ident|T::Upper|T::Kw) && p.kn(1) == T::Colon { p.bump(); p.bump(); let v = p.pattern(); p.ast.push(N::Field, 0, v, s, p.pe()) } else { p.pattern() } }); let _ = f; self.ast.push(N::TaggedRecord, tag, 0, s, self.pe()) }
            _ => self.unary(),
        }
    }
    fn pipe_rhs(&mut self, input: u32, s: u32) -> u32 {
        let k = self.k();
        match k {
            T::Kw => {
                let kw = self.t.pay[self.i];
                match kw {
                    K_WHEN | K_WHILE => { self.bump(); let (a, c) = self.arms(); let _ = (a, c); self.ast.push(if kw == K_WHEN { N::When } else { N::While }, input, a, s, self.pe()) }
                    K_THEN => { self.bump(); let b = self.braced_expr(); self.ast.push(N::Then, input, b, s, self.pe()) }
                    K_HOLD => { self.bump(); if self.k() == T::Ident { self.bump(); } else { self.err("expected HOLD state name"); } if !self.expect(T::LBrace, "expected `{`") { return 0; } let (a, _c) = self.elements(T::RBrace, |p| p.expr(0)); self.ast.push(N::Hold, input, a, s, self.pe()) }
                    K_DRAINING => { self.bump(); self.ast.push(N::Draining, input, 0, s, self.pe()) }
                    K_LATEST => { let l = self.primary(); self.ast.push(N::PipeCall, input, l, s, self.pe()) }
                    K_FLUSH | K_SKIP => { self.bump(); self.ast.push(N::PipeCall, input, 0, s, self.pe()) }
                    _ => { let e = self.err("unexpected keyword after `|>`"); self.bump(); e }
                }
            }
            T::Ident | T::Upper => {
                let f = self.t.pay[self.i]; self.bump();
                if self.k() == T::LParen && !self.nl() { self.bump(); let (a, _c) = self.args(); let _ = a; self.ast.push(N::PipeCall, input, f, s, self.pe()) }
                else { self.err("expected `(` after pipe target") }
            }
            T::Dot => { self.bump(); let f = self.t.pay[self.i]; self.bump(); self.ast.push(N::PipeField, input, f, s, self.pe()) }
            _ => self.err("expected call, WHEN, WHILE, THEN, HOLD or `.field` after `|>`"),
        }
    }
    fn primary(&mut self) -> u32 {
        let s = self.s();
        match self.k() {
            T::Num => { self.bump(); self.ast.push(N::Num, 0, 0, s, self.pe()) }
            T::Str => { self.bump(); self.ast.push(N::Str, 0, 0, s, self.pe()) }
            T::TextOpen => {
                self.bump(); let mut parts = 0u32;
                loop {
                    match self.k() {
                        T::TextChunk => { self.bump(); parts += 1; }
                        T::Interp => { let sy = self.t.pay[self.i]; let ps = self.s(); self.bump(); self.ast.push(N::Interp, sy, 0, ps, self.pe()); parts += 1; }
                        T::TextClose => { self.bump(); break; }
                        _ => { self.err("unterminated TEXT"); break; }
                    }
                }
                self.ast.push(N::Text, parts, 0, s, self.pe())
            }
            T::Ident => {
                let sy = self.t.pay[self.i]; self.bump();
                if self.k() == T::LParen && !self.nl() { self.bump(); let (a, c) = self.args(); let _ = c; return self.ast.push(N::Call, sy, a, s, self.pe()); }
                let mut e = self.ast.push(N::Ident, sy, 0, s, self.pe());
                while self.k() == T::Dot && !self.nl() && matches!(self.kn(1), T::Ident | T::Upper | T::Kw) { self.bump(); let f = self.t.pay[self.i]; self.bump(); e = self.ast.push(N::Path, e, f, s, self.pe()); }
                if self.k() == T::LParen && !self.nl() { self.bump(); let (a, _c) = self.args(); return self.ast.push(N::Call, e, a, s, self.pe()); }
                e
            }
            T::Upper => {
                let sy = self.t.pay[self.i]; self.bump();
                if self.k() == T::LBrack && !self.nl() { self.bump(); self.ctx = 1; let (a, _c) = self.elements(T::RBrack, |p| p.named_element(false)); self.ctx = 0; return self.ast.push(N::TaggedRecord, sy, a, s, self.pe()); }
                if self.k() == T::LParen && !self.nl() { self.bump(); let (a, _c) = self.args(); return self.ast.push(N::Call, sy, a, s, self.pe()); }
                self.ast.push(N::Tag, sy, 0, s, self.pe())
            }
            T::LBrack => { self.bump(); self.ctx = 1; let (a, c) = self.elements(T::RBrack, |p| p.named_element(false)); self.ctx = 0; self.ast.push(N::Record, a, c, s, self.pe()) }
            T::LParen => { self.bump(); let e = self.expr(0); self.expect(T::RParen, "expected `)`"); e }
            T::Kw => {
                let kw = self.t.pay[self.i];
                match kw {
                    K_LIST | K_SET | K_LATEST => {
                        self.bump();
                        if self.k() == T::LBrack { self.bump(); let _ = self.expr(0); self.expect(T::RBrack, "expected `]`"); }
                        if !self.expect(T::LBrace, "expected `{`") { return 0; }
                        let (a, c) = self.elements(T::RBrace, |p| { let s = p.s(); if p.k() == T::Ellipsis { p.bump(); let v = p.expr(0); return p.ast.push(N::Spread, v, 0, s, p.pe()); } p.expr(0) });
                        self.ast.push(match kw { K_LIST => N::List, K_SET => N::Set, _ => N::Latest }, a, c, s, self.pe())
                    }
                    K_MAP => { self.bump(); let (a, c) = self.arms(); self.ast.push(N::Map, a, c, s, self.pe()) }
                    K_BYTES | K_BITS => {
                        self.bump();
                        if self.k() == T::LBrack { self.bump(); let _ = self.expr(0); self.expect(T::RBrack, "expected `]`"); }
                        if self.k() == T::LBrace { self.bump(); let (a, c) = self.elements(T::RBrace, |p| p.expr(0)); return self.ast.push(N::Bytes, a, c, s, self.pe()); }
                        self.ast.push(N::Bits, 0, 0, s, self.pe())
                    }
                    K_BLOCK => { self.bump(); if !self.expect(T::LBrace, "expected `{`") { return 0; } self.ctx = 2; let (a, c) = self.elements(T::RBrace, |p| p.named_element(false)); self.ctx = 0; self.ast.push(N::Block, a, c, s, self.pe()) }
                    K_SOURCE => { self.bump(); self.ast.push(N::Source, 0, 0, s, self.pe()) }
                    K_SKIP => { self.bump(); self.ast.push(N::Skip, 0, 0, s, self.pe()) }
                    K_FLUSH => { self.bump(); let v = if self.k() == T::LBrace && !self.nl() { self.braced_expr() } else { 0 }; self.ast.push(N::Flush, v, 0, s, self.pe()) }
                    K_PASSED => {
                        self.bump(); let mut e = self.ast.push(N::Passed, 0, 0, s, self.pe());
                        while self.k() == T::Dot && !self.nl() { self.bump(); let f = self.t.pay[self.i]; self.bump(); e = self.ast.push(N::Path, e, f, s, self.pe()); }
                        e
                    }
                    K_DRAIN => { self.bump(); let b = self.braced_expr(); self.ast.push(N::Draining, b, 0, s, self.pe()) }
                    K_THEN | K_WHEN | K_WHILE | K_HOLD => { let e = self.err("operator needs a `|>` input"); self.bump(); e }
                    _ => { let e = self.err("unexpected keyword"); self.bump(); e }
                }
            }
            T::Eof => self.err("unexpected end of file"),
            _ => { let e = self.err("expected expression"); self.bump(); e }
        }
    }
}

// ---------------- definitions & fingerprints ----------------
// Definition = top-level item, or a field of a record literal whose parent is a definition (root field paths).
// Fingerprints hash the definition's own tokens (child definitions contribute only their names), with
// interned symbol ids (append-only session interner) and no positions => item-relative by construction.
struct Def { path_hash: u64, tok_start: u32, tok_end: u32, fp_shape: u64, fp_lit: u64 }

fn tok_at(t: &Toks, off: u32) -> u32 { match t.start.binary_search(&off) { Ok(i) => i as u32, Err(i) => i as u32 } }

fn collect_defs(ast: &Ast, t: &Toks, src: &[u8], items: &[(u32, u32, u32)], out: &mut Vec<Def>) {
    fn walk(ast: &Ast, t: &Toks, src: &[u8], node: u32, parent: u64, out: &mut Vec<Def>) -> Option<(u32, u32)> {
        let k = ast.kind[node as usize];
        let (name, value) = match k { N::Field => (ast.a[node as usize], ast.b[node as usize]), N::Function => (ast.a[node as usize] ^ 0xF00D_0000, u32::MAX), _ => return None };
        let path_hash = (parent.rotate_left(7) ^ fxhash(&name.to_le_bytes())).wrapping_mul(0x9E3779B97F4A7C15);
        let ts = tok_at(t, ast.start[node as usize]);
        let te = tok_at(t, ast.end[node as usize]);
        let me = out.len(); out.push(Def { path_hash, tok_start: ts, tok_end: te, fp_shape: 0, fp_lit: 0 });
        let mut holes: Vec<(u32, u32)> = vec![];
        if value != u32::MAX && ast.kind[value as usize] == N::Record {
            let (a, c) = (ast.a[value as usize], ast.b[value as usize]);
            for j in a..a + c { if let Some(r) = walk(ast, t, src, ast.extra[j as usize], path_hash, out) { holes.push(r); } }
        }
        let mut shape: u64 = 0xcbf29ce484222325; let mut lit: u64 = 0x84222325cbf29ce4;
        let mut i = ts; let mut h = 0usize;
        while i < te {
            if h < holes.len() && i == holes[h].0 { shape = (shape ^ 0xABCD).wrapping_mul(0x100000001b3); i = holes[h].1.max(i + 1); h += 1; continue; }
            let k = t.kind[i as usize];
            shape = (shape ^ (k as u64) ^ ((t.pay[i as usize] as u64) << 8) ^ ((t.flags[i as usize] as u64) << 40)).wrapping_mul(0x100000001b3);
            if matches!(k, T::Num | T::Str | T::TextChunk) {
                lit = (lit ^ fxhash(&src[t.start[i as usize] as usize..t.end[i as usize] as usize])).wrapping_mul(0x100000001b3);
            }
            i += 1;
        }
        out[me].fp_shape = shape; out[me].fp_lit = lit;
        Some((ts, te))
    }
    for &(n, _, _) in items { walk(ast, t, src, n, 0, out); }
}

fn parse_unit(src: &[u8], int: &mut Interner) -> (Toks, Ast, Vec<Diag>, Vec<(u32, u32, u32)>) {
    let mut t = Toks::default();
    t.kind.reserve(src.len() / 5); t.start.reserve(src.len() / 5); t.end.reserve(src.len() / 5); t.pay.reserve(src.len() / 5); t.flags.reserve(src.len() / 5); t.indent.reserve(src.len() / 5);
    lex(src, int, &mut t);
    let (ast, diags, items) = {
        let mut p = P { t: &t, i: 0, ast: Ast::default(), diags: vec![], list: Vec::with_capacity(256), items: vec![], ctx: 0, lints: vec![] };
        let cap = t.kind.len();
        p.ast.kind.reserve(cap); p.ast.a.reserve(cap); p.ast.b.reserve(cap); p.ast.start.reserve(cap); p.ast.end.reserve(cap);
        p.unit();
        for (at, m) in p.lints.drain(..) { p.diags.push(Diag { at, msg: m }); }
        (p.ast, p.diags, p.items)
    };
    (t, ast, diags, items)
}

fn line_col(t: &Toks, off: u32) -> (usize, usize) {
    let l = match t.line_starts.binary_search(&off) { Ok(i) => i, Err(i) => i - 1 };
    (l + 1, (off - t.line_starts[l]) as usize + 1)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = args[0].as_str();
    let files: Vec<String> = args[1..].to_vec();
    let srcs: Vec<Vec<u8>> = files.iter().map(|f| std::fs::read(f).unwrap()).collect();
    let mut int = Interner::new();
    let wildcard = int.intern(b"__"); assert_eq!(wildcard, 0);
    // fixups: `__` must map to u32::MAX payload for the wildcard check; simpler: treat sym 0 as wildcard
    if mode == "check" {
        let mut bad = 0;
        for (f, s) in files.iter().zip(&srcs) {
            let (t, ast, diags, items) = parse_unit(s, &mut int);
            let mut defs = vec![]; collect_defs(&ast, &t, s, &items, &mut defs);
            if !diags.is_empty() {
                bad += 1;
                for d in diags.iter().take(3) { let (l, c) = line_col(&t, d.at); println!("{f}:{l}:{c}: {}", d.msg); }
                if diags.len() > 3 { println!("{f}: ... {} more", diags.len() - 3); }
            }
            let _ = (ast.kind.len(), defs.len());
        }
        println!("files={} with_errors={}", files.len(), bad);
        return;
    }
    // bench
    let total: usize = srcs.iter().map(|s| s.len()).sum();
    let mut best_lex = f64::MAX; let mut best_all = f64::MAX; let mut ntok = 0; let mut nnode = 0; let mut ndefs = 0; let mut ndiag = 0;
    for _ in 0..30 {
        let mut int2 = Interner::new();
        let t0 = Instant::now();
        let mut toks = 0;
        for s in &srcs { let mut t = Toks::default(); lex(s, &mut int2, &mut t); toks += t.kind.len(); }
        best_lex = best_lex.min(t0.elapsed().as_secs_f64()); ntok = toks;
        let mut int3 = Interner::new();
        let t1 = Instant::now();
        let mut nodes = 0; let mut defs_n = 0; let mut dn = 0;
        for s in &srcs { let (t, ast, diags, items) = parse_unit(s, &mut int3); let mut defs = vec![]; collect_defs(&ast, &t, s, &items, &mut defs); nodes += ast.kind.len(); defs_n += defs.len(); dn += diags.len(); }
        best_all = best_all.min(t1.elapsed().as_secs_f64()); nnode = nodes; ndefs = defs_n; ndiag = dn;
    }
    println!("files={} bytes={} tokens={} nodes={} defs={} diags={} lex_ms={:.3} lex+parse+defs_ms={:.3} MB/s={:.0} ns/token={:.1}",
        files.len(), total, ntok, nnode, ndefs, ndiag, best_lex * 1e3, best_all * 1e3, total as f64 / best_all / 1e6, best_all * 1e9 / ntok as f64);
    // warm: edit a char in the largest file, reparse that unit, diff definitions
    let bi = (0..srcs.len()).max_by_key(|&i| srcs[i].len()).unwrap();
    let big = &srcs[bi];
    let (t0k, a0, _, it0) = parse_unit(big, &mut int);
    let mut d0 = vec![]; collect_defs(&a0, &t0k, big, &it0, &mut d0);
    // find a TEXT literal near the middle and insert a char
    let mid = big.len() / 2;
    let pos = big[mid..].windows(6).position(|w| w == b"TEXT {").map(|p| mid + p + 7).unwrap_or(mid);
    let mut edited = big.clone();
    match std::env::var("EDIT").as_deref() {
        Ok("top") => { for (j, c) in b"-- moved\n\n".iter().enumerate() { edited.insert(j, *c); } }
        Ok("rename") => { // rename a local token inside one definition: change first occurrence of ' item' after mid
            let p2 = big[mid..].windows(5).position(|w| w == b" item").map(|p| mid + p + 1).unwrap(); edited[p2] = b'j'; }
        Ok("field") => { // add a new record field line after the first `store: [` line
            let p2 = big.windows(9).position(|w| w == b"store: [\n").map(|p| p + 9).unwrap();
            for (j, c) in b"    zz_new_field: 1\n".iter().enumerate() { edited.insert(p2 + j, *c); } }
        _ => { edited.insert(pos, b'x'); }
    }
    let mut best_warm = f64::MAX; let mut changed = 0usize; let mut lit_only = 0usize;
    for _ in 0..30 {
        let t = Instant::now();
        let (tk, a1, _, it1) = parse_unit(&edited, &mut int);
        let mut d1 = vec![]; collect_defs(&a1, &tk, &edited, &it1, &mut d1);
        let old: HashMap<u64, (u64, u64)> = d0.iter().map(|d| (d.path_hash, (d.fp_shape, d.fp_lit))).collect();
        let mut ch = 0; let mut lo = 0;
        for d in &d1 { match old.get(&d.path_hash) { Some(&(s, l)) if s == d.fp_shape && l == d.fp_lit => {}, Some(&(s, _)) if s == d.fp_shape => { lo += 1; ch += 1; } _ => ch += 1 } }
        best_warm = best_warm.min(t.elapsed().as_secs_f64()); changed = ch; lit_only = lo;
    }
    println!("warm: reparse {} ({} bytes) + fingerprint + diff {} defs: best_ms={:.3} changed_defs={} (literal-only {})", files[bi], big.len(), d0.len(), best_warm * 1e3, changed, lit_only);
    let _ = int.get(0);
}
