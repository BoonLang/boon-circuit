// Standalone calibration benchmark for the proposed Boon lexer design.
// Byte-class table, SoA tokens, session interner (FxHash + open addressing),
// TEXT { ... } raw mode with {interpolation}. Not repo code.
use std::time::Instant;

const WS: u8 = 0; const NL: u8 = 1; const ALPHA: u8 = 2; const DIGIT: u8 = 3;
const QUOTE: u8 = 4; const MINUS: u8 = 5; const OP: u8 = 6; const SYM: u8 = 7; const OTHER: u8 = 8;

fn classes() -> [u8; 256] {
    let mut t = [OTHER; 256];
    t[b' ' as usize] = WS; t[b'\t' as usize] = WS; t[b'\r' as usize] = WS; t[b'\n' as usize] = NL;
    for c in b'a'..=b'z' { t[c as usize] = ALPHA; }
    for c in b'A'..=b'Z' { t[c as usize] = ALPHA; }
    t[b'_' as usize] = ALPHA;
    for c in b'0'..=b'9' { t[c as usize] = DIGIT; }
    t[b'"' as usize] = QUOTE; t[b'-' as usize] = MINUS;
    for c in b"><=|+%*/!".iter() { t[*c as usize] = OP; }
    for c in b"[]{}():,.$#".iter() { t[*c as usize] = SYM; }
    t
}

fn ident_cont() -> [bool; 256] {
    let mut t = [false; 256];
    for c in b'a'..=b'z' { t[c as usize] = true; }
    for c in b'A'..=b'Z' { t[c as usize] = true; }
    for c in b'0'..=b'9' { t[c as usize] = true; }
    t[b'_' as usize] = true; t[b'-' as usize] = true; t[b'/' as usize] = true;
    t
}

#[inline]
fn fxhash(bytes: &[u8]) -> u64 {
    const K: u64 = 0x517cc1b727220a95;
    let mut h: u64 = 0;
    let mut chunks = bytes.chunks_exact(8);
    for c in &mut chunks { let w = u64::from_le_bytes(c.try_into().unwrap()); h = (h.rotate_left(5) ^ w).wrapping_mul(K); }
    let r = chunks.remainder();
    if !r.is_empty() { let mut b = [0u8; 8]; b[..r.len()].copy_from_slice(r); h = (h.rotate_left(5) ^ u64::from_le_bytes(b)).wrapping_mul(K); }
    (h.rotate_left(5) ^ bytes.len() as u64).wrapping_mul(K)
}

struct Interner { table: Vec<u32>, hashes: Vec<u64>, starts: Vec<u32>, arena: Vec<u8>, mask: usize }
impl Interner {
    fn new() -> Self { let cap = 1 << 16; Interner { table: vec![u32::MAX; cap], hashes: vec![], starts: vec![0], arena: vec![], mask: cap - 1 } }
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
}

#[derive(Default)]
struct Tokens { kind: Vec<u8>, start: Vec<u32>, payload: Vec<u32>, line_starts: Vec<u32> }

// token kinds
const T_IDENT: u8 = 1; const T_NUM: u8 = 2; const T_STR: u8 = 3; const T_COMMENT: u8 = 4; const T_OP: u8 = 5;
const T_SYM: u8 = 6; const T_NL: u8 = 7; const T_TEXT_OPEN: u8 = 8; const T_TEXT_CHUNK: u8 = 9; const T_INTERP: u8 = 10;
const T_TEXT_CLOSE: u8 = 11; const T_ERR: u8 = 12;

fn lex(src: &[u8], cls: &[u8; 256], ic: &[bool; 256], int: &mut Interner, text_sym: u32, out: &mut Tokens) {
    let n = src.len();
    let mut i = 0usize;
    out.line_starts.push(0);
    while i < n {
        let b = src[i];
        match cls[b as usize] {
            WS => { i += 1; }
            NL => { out.kind.push(T_NL); out.start.push(i as u32); out.payload.push(0); i += 1; out.line_starts.push(i as u32); }
            ALPHA => {
                let s = i; i += 1;
                while i < n && ic[src[i] as usize] { i += 1; }
                let sym = int.intern(&src[s..i]);
                out.kind.push(T_IDENT); out.start.push(s as u32); out.payload.push(sym);
                if sym == text_sym {
                    let mut j = i; while j < n && (src[j] == b' ' || src[j] == b'\t') { j += 1; }
                    if j < n && src[j] == b'{' {
                        out.kind.push(T_TEXT_OPEN); out.start.push(j as u32); out.payload.push(0);
                        i = j + 1;
                        let mut chunk = i;
                        loop {
                            if i >= n { out.kind.push(T_ERR); out.start.push(i as u32); out.payload.push(0); break; }
                            let c = src[i];
                            if c == b'\n' { out.line_starts.push(i as u32 + 1); i += 1; continue; }
                            if c == b'}' {
                                if i > chunk { out.kind.push(T_TEXT_CHUNK); out.start.push(chunk as u32); out.payload.push((i - chunk) as u32); }
                                out.kind.push(T_TEXT_CLOSE); out.start.push(i as u32); out.payload.push(0); i += 1; break;
                            }
                            if c == b'{' {
                                if i > chunk { out.kind.push(T_TEXT_CHUNK); out.start.push(chunk as u32); out.payload.push((i - chunk) as u32); }
                                let s = i + 1; let mut k = s; while k < n && src[k] != b'}' && src[k] != b'\n' { k += 1; }
                                let mut a = s; while a < k && src[a] == b' ' { a += 1; }
                                let mut z = k; while z > a && src[z - 1] == b' ' { z -= 1; }
                                let sym = int.intern(&src[a..z]);
                                out.kind.push(T_INTERP); out.start.push(a as u32); out.payload.push(sym);
                                i = if k < n { k + 1 } else { k }; chunk = i; continue;
                            }
                            i += 1;
                        }
                    }
                }
            }
            DIGIT => { let s = i; i += 1; while i < n && cls[src[i] as usize] == DIGIT { i += 1; } out.kind.push(T_NUM); out.start.push(s as u32); out.payload.push(0); }
            QUOTE => {
                let s = i; i += 1;
                while i < n && src[i] != b'"' { if src[i] == b'\\' { i += 1; } i += 1; }
                i = (i + 1).min(n);
                out.kind.push(T_STR); out.start.push(s as u32); out.payload.push(0);
            }
            MINUS => {
                if i + 1 < n && src[i + 1] == b'-' { let s = i; while i < n && src[i] != b'\n' { i += 1; } out.kind.push(T_COMMENT); out.start.push(s as u32); out.payload.push(0); }
                else { out.kind.push(T_OP); out.start.push(i as u32); out.payload.push(b'-' as u32); i += 1; }
            }
            OP => {
                let s = i; let two = i + 1 < n && matches!((b, src[i + 1]), (b'=', b'>') | (b'|', b'>') | (b'=', b'=') | (b'>', b'=') | (b'<', b'=') | (b'!', b'='));
                i += if two { 2 } else { 1 }; out.kind.push(T_OP); out.start.push(s as u32); out.payload.push(b as u32);
            }
            SYM => { out.kind.push(T_SYM); out.start.push(i as u32); out.payload.push(b as u32); i += 1; }
            _ => { let s = i; i += 1; while i < n && (src[i] & 0xC0) == 0x80 { i += 1; } out.kind.push(T_ERR); out.start.push(s as u32); out.payload.push(0); }
        }
    }
}

// Rolling structural fingerprint over tokens (identity by token kind + symbol, ignoring spans),
// split per top-level line group, to estimate the cost of per-definition fingerprinting in the same pass.
fn fingerprint(t: &Tokens) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for k in 0..t.kind.len() { h = (h.rotate_left(5) ^ ((t.kind[k] as u64) << 32 | t.payload[k] as u64)).wrapping_mul(0x517cc1b727220a95); }
    h
}

fn main() {
    let files: Vec<String> = std::env::args().skip(1).collect();
    let srcs: Vec<Vec<u8>> = files.iter().map(|f| std::fs::read(f).unwrap()).collect();
    let total: usize = srcs.iter().map(|s| s.len()).sum();
    let cls = classes(); let ic = ident_cont();
    let reps = 200;
    let mut best = f64::MAX; let mut ntok = 0; let mut nsym = 0; let mut fp = 0u64;
    for r in 0..reps {
        let mut int = Interner::new();
        for kw in ["FUNCTION","LIST","MAP","SET","HOLD","THEN","WHEN","WHILE","LATEST","BLOCK","SOURCE","SKIP","FLUSH","PASS","PASSED","TEXT","BYTES","BITS","DRAIN","OUT"] { int.intern(kw.as_bytes()); }
        let text_sym = int.intern(b"TEXT");
        let t0 = Instant::now();
        let mut toks = 0; fp = 0;
        for s in &srcs {
            let mut t = Tokens::default();
            t.kind.reserve(s.len() / 5); t.start.reserve(s.len() / 5); t.payload.reserve(s.len() / 5);
            lex(s, &cls, &ic, &mut int, text_sym, &mut t);
            toks += t.kind.len();
            fp ^= fingerprint(&t);
        }
        let dt = t0.elapsed().as_secs_f64();
        if r > 5 && dt < best { best = dt; }
        ntok = toks; nsym = int.hashes.len();
    }
    // warm re-lex of the largest single file with a pre-filled interner (the warm-edit case)
    let (bi, big) = srcs.iter().enumerate().max_by_key(|(_, s)| s.len()).unwrap();
    let mut int = Interner::new(); let text_sym = int.intern(b"TEXT");
    let mut t = Tokens::default(); lex(big, &cls, &ic, &mut int, text_sym, &mut t);
    let mut best_big = f64::MAX;
    for _ in 0..200 {
        let t0 = Instant::now();
        let mut t = Tokens::default(); t.kind.reserve(big.len() / 5); t.start.reserve(big.len() / 5); t.payload.reserve(big.len() / 5);
        lex(big, &cls, &ic, &mut int, text_sym, &mut t);
        let f = fingerprint(&t);
        let dt = t0.elapsed().as_secs_f64();
        if dt < best_big { best_big = dt; }
        std::hint::black_box(f);
    }
    println!("files={} bytes={} tokens={} symbols={} best_ms={:.3} MB/s={:.0} ns/token={:.1} fp={:x}",
        srcs.len(), total, ntok, nsym, best * 1e3, total as f64 / best / 1e6, best * 1e9 / ntok as f64, fp);
    println!("warm relex+fingerprint of {} ({} bytes, interner prefilled): best_ms={:.3}", files[bi], big.len(), best_big * 1e3);
}
