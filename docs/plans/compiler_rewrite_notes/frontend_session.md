> **Design-panel input, 2026-09-29. Not authority.** Written by the design round
> that fed `docs/plans/BOON_COMPILER_REWRITE_PLAN.md`, followed by its adversarial
> review. Where this note disagrees with the plan's decision table (D1-D13) or
> defaults, the plan wins. Delete this folder once the P0 spec and contract exist.
> File:line references point at the tree as of 2026-09-29.

# Front end (lexer, parser, definitions, discovery), session / warm path (snapshots, fingerprints, early cutoff, lanes, cancellation), and integration (every compiler API consumer, new public API)

## area
Front end (lexer, parser, definitions, discovery), session / warm path (snapshots, fingerprints, early cutoff, lanes, cancellation), and integration (every compiler API consumer, new public API)

## summary
I measured instead of estimating. A standalone prototype (design/fe_proto.rs, compiled with rustc outside the repo) has a byte-class lexer with TEXT raw mode, a session interner, a newline-aware Pratt parser writing into a flat arena, and per-definition header/shape/literal fingerprints. It parses NovyWave (433 KB, 8 units) in 2.6 ms (today 65 ms), TodoMVC in 0.64 ms (today 18 ms), counter in 0.021 ms (today 3.6 ms) and the whole 1.08 MB example corpus in 6.9 ms. Under one uniform layout rule, all 141 current example files parse with zero diagnostics. Warm reparse is cheap: re-parsing the largest unit whole (NovyWave RUN.bn, 208 KB), fingerprinting its 841 definitions and diffing them takes 1.24 ms. The diff is exact: inserting lines at the top of a file changes 0 definitions, a literal edit changes 1 (literal-only), a local rename 1, and adding a field 2. So the design needs no incremental parser. Element-level splicing is kept only as error containment. The layout rule is: a newline ends an element unless the next line starts with `|>`, `=>`, `,`, a closing bracket or a binary operator other than `-`. Tokens that need a follow-up carry over to the next line. Indentation is checked but never decides structure. The probes also found current parser behavior that needs owner sign-off. Arithmetic is flat and right-associative (`2 * 3 + 1` parses as `2 * (3 + 1)`), and this changes the meaning of exact_number_value_algebra.bn. Some input is silently dropped (`value: 1` followed by an indented `2` yields 2). `--` inside TEXT starts a comment, and a column-0 `|>` continuation breaks BUILD.bn. File discovery needs no manifest: following `Module/` references reproduces the manifest file lists for every entry, except two `Generated/Assets.bn` units that nothing references. The session keeps an append-only interner and a DefKey-to-DefId table. Each revision is an Arc-shared snapshot of per-definition syntax, resolution, check and lowering records. Checking goes in dependency order over strongly connected groups of definitions and stops propagating where an interface fingerprint is unchanged. Literal-only edits skip type checking, which is sound now that types never depend on values. Diagnostics and the plan are published as two separate lanes. Cancellation is polled per definition and every 4096 solver steps. The public API shrinks to Session::{new, edit, check, build}, one-shot compile/check calls, a DistributedSession, and syntax/project/debug modules. The consumer matrix maps all 24 current callers onto it, from the playground, editor, program runtime and CLI down to xtask and the tests. The new compile service also becomes the warm benchmark driver.

## design
# Front end, session (warm path) and integration

Scope: source bytes to a per-definition dependency graph (front end), the retained per-revision orchestration around the checker and lowering (session), and every caller of today's compiler API (integration). Checker internals, lowering and the plan format belong to sibling panels; section 2.11 fixes the contracts this area needs from them.

## 0. What was measured for this design

**Prototype** `scratchpad/design/fe_proto.rs` (not repo code; `rustc -O -C target-cpu=native`, i7-9700K, single thread, best of 30). It has a byte-class lexer with TEXT raw mode and an append-only FxHash interner, a newline-aware recursive-descent + Pratt parser into a flat struct-of-arrays arena with element-level recovery, a definition table with 3 fingerprints, and a diff.

| input | bytes | tokens | nodes | defs | lex | lex+parse+defs | today (parse phase) |
|---|---:|---:|---:|---:|---:|---:|---:|
| NovyWave (8 units) | 433,371 | 56,855 | 29,737 | 1,301 | 1.46-1.50 ms | **2.61-2.64 ms** (164 MB/s, 46 ns/token) | 65 ms |
| TodoMVC physical (7 units) | 102,167 | 13,694 | 7,634 | 174 | 0.37 ms | **0.64 ms** | 18 ms |
| counter | 3,352 | 552 | 272 | 22 | 0.013 ms | **0.021 ms** | 3.6 ms (2.7 ms of it is manifest loading) |
| all 140 current example files | 1,082,095 (30,114 lines) | 147,351 | 75,353 | 3,377 | 4.0 ms | **6.9 ms** (≈4.4 M lines/s) | — |

Warm edits: re-parse the whole unit, fingerprint every definition, diff by DefKey.

| unit | bytes | defs | time | edit results |
|---|---:|---:|---:|---|
| novywave/RUN.bn | 208,274 | 841 | 1.24 ms | literal: 1 changed (literal-only); 2 lines inserted at file top: **0 changed**; local rename: 1 (shape); new store field: 2 (new def + parent header) |
| novywave/View/NovyView.bn | 188,540 | 379 | 1.33 ms | literal: 1 |
| todo_mvc_physical/RUN.bn | 34,661 | 97 | 0.23 ms | same pattern |

For comparison, today a warm re-parse costs 41.7-49.1 ms on NovyWave RUN.bn and 6.2-8.5 ms on TodoMVC (a_frontend, a_integration_warm).

The prototype accepts all 141 current-surface `examples/**/*.bn`, including both BUILD.bn files, with zero diagnostics under the rules in §1.3, including the strict indentation checks. It rejects `language_surface/future/where_contracts.bn`. The earlier lexbench (interning included) runs at 352 MB/s.

**Corpus layout facts** (python scan of examples):
- 533 lines start with `|>`: 473 at the previous line's indentation, 60 deeper, 0 shallower.
- 2 lines start with `==` (novywave/RUN.bn:204, :210). No line starts with any other operator.
- No line ends with a binary operator, `,` or `|>`. 67 end with `=>`, 949 with `:`, 1,226 with `(`.
- No identifier contains `-`. No tab indentation.
- 19 sites use two or more unparenthesized infix operators.

## 1. Front end

### 1.1 Sources and project discovery

The compiler API never touches the filesystem. It takes `Source { path: Arc<str>, text: Arc<str> }`. Paths are project-relative with `/` separators and are normalized lexically, never canonicalized.

Module naming is unchanged (boon_parser/src/lib.rs:3987-3999): the entry has no module, an uppercase file stem is a module, and a lowercase stem merges into the root namespace.

Discovery is a separate function, `boon_compiler::project::discover(entry)`. Only by-path tools use it: CLI, phase0 runner, host migration-scenario runner.

1. **Project root.** For `RUN.bn`, the entry's directory; if that directory is a role directory (`Client`/`Server`/`Session`), its parent. For `X.bn`, the sibling directory `X/` if it exists, otherwise a single-file project.
2. **Root-namespace units.** Every lowercase-stem `*.bn` in the namespace directory (non-recursive): the entry directory for `RUN.bn`, `X/` for `X.bn`. `RUN.bn` and `BUILD.bn` are excluded.
3. **Module index.** Every uppercase-stem `*.bn` under the root, found in one directory walk. Two files with the same stem is an error naming both.
4. **Closure.** Lex each included unit; every qualified name `Stem/...` whose Stem is in the index adds that file. Repeat until nothing is added. The lexing is reused as the first parse.
5. `BUILD.bn` is its own program and never a unit of RUN.

Result of this rule against examples/manifest.toml: exact match for all 22 entries, including the 3 fjordpulse roles and cells/`cells/*.bn`. The only differences are `novywave/Generated/Assets.bn` and `todo_mvc_physical/Generated/Assets.bn`, which nothing references.

What this removes:
- the manifest lookup and full validation on every by-path compile (boon_compiler/src/lib.rs:1876-1901; ~1,300 readlink calls, 2.7 ms);
- the `CARGO_MANIFEST_DIR`-dependent logical paths (lib.rs:1816-1825).

A qualified reference to a module the host did not supply is a positioned diagnostic `unknown module X`. `examples/manifest.toml` stays a fixture registry for xtask and the playground catalog; `validate_at` runs only in xtask.

### 1.2 Lexer

**Output** is struct-of-arrays:

```rust
struct Tokens {
    kind: Vec<Tok>,          // u8 enum
    start: Vec<u32>,         // byte offset; the end comes from the lexer, stored as u16 len or the next start
    sym: Vec<Symbol>,        // Symbol(u32) for identifiers, tags, qualified names, interpolation paths
    flags: Vec<u8>,          // FIRST_ON_LINE
    indent: Vec<u16>,        // indentation of the token's line
    partner: Vec<u32>,       // matching bracket index (u32::MAX if unmatched)
}
struct LineTable { starts: Vec<u32> }
struct Trivia { comments: Vec<(u32, u32)> }   // tooling mode only
```

There is one pass with a 256-entry byte-class table and an identifier-continue table. Newlines and comments are not tokens: they set `FIRST_ON_LINE` and `indent` on the next token. Today's lexer allocates a heap `String` per token and re-walks the bytes (boon_parser lib.rs:5393, 5405).

**Lexical rules** (normative; unchanged from today unless flagged):
- Whitespace: space, tab, `\r`.
- Comment: `--` to end of line, outside strings and TEXT bodies (flag: today `--` inside TEXT is a comment, see §1.6).
- Identifier: `[A-Za-z_][A-Za-z0-9_\-/]*`, unchanged (lib.rs:5275-5283), so `x-1` is one identifier.
  - An ALL-CAPS word in the fixed keyword set is a keyword token (FUNCTION LIST MAP SET BYTES BITS HOLD THEN WHEN WHILE LATEST BLOCK SOURCE SKIP FLUSH PASS PASSED OUT DRAIN DRAINING TEXT).
  - An uppercase-initial word without `/` is a Tag.
  - A word containing `/` is a qualified name (`Module/fn`, `Role/value`, `Scene/Element/stripe`).
  - `__` is the wildcard.
- Number: `[0-9]+(\.[0-9]+)?` as a single token (today three tokens re-joined by ast_number_literal, lib.rs:6371). Radix/sized literals written glued, such as `16uFF`, and BITS forms are one token. `-` is always an operator token.
- String: `"..."` with today's escapes (lib.rs:7280-7300).
- TEXT: `TEXT`, optional spaces, then `{` enters raw mode. Content runs to the matching `}` (braces nest). `{ $?name(.name)* }` or `{ Qualified/name }` is an interpolation, lexed as ordinary tokens. The value is the raw content trimmed at both ends, with interior whitespace and newlines verbatim (matches lib.rs:7153-7177; verified with dump-ir: `"line one\n        indented "`). Raw-mode newlines do not set layout flags.
- Punctuation: `( ) [ ] { } : , . ...`. Operators: `|> => == != <= >= < > + - * / %`.
- Error tokens with today's messages, emitted inline (replacing validate_source_syntax_with_work, lib.rs:7503-7660): `#`, `$` outside TEXT, single `=` and `|`, `EXAMPLE`, `LINK`, planned-feature spellings, unterminated string or TEXT. The path-keyed example-style lint (`bg`/`fill`/`true`/`false` when the path contains `/examples/`, lib.rs:7509-7558) moves to xtask.

**Interner.** One per session, append-only, never renumbered. FxHash open addressing over a byte arena. All builtin qualified names are pre-interned in a fixed order at session start, so `sym.0 < BUILTIN_COUNT` means builtin index `sym.0`. Keywords are token kinds, not symbols. Identifiers from intermediate keystrokes are kept; a session that exceeds 1 M junk symbols is rebuilt cold (~30-75 ms, rare).

**Tooling mode.** The same lexer, with trivia and TEXT-chunk classes, serves highlighting. It replaces `boon_parser::lex_source` in boon_editor language.rs:144, which today tokenizes TEXT bodies as code.

### 1.3 Layout and grammar (normative)

Today's parser splits lines into items, builds a tree from indentation (lib.rs:5786-5815), then runs 3 line-merge passes (only `(`, BYTES and DRAIN merge, lib.rs:4391-4397) and 4 repair passes (lib.rs:4427-4430). Its behavior differs per construct. One rule replaces all of it; it is validated on every example.

**Layout rules**
- **L1 (separator).** A line break ends the current element (acts like `,`) unless the next token is one of `) ] } , |> =>` or a binary operator other than `-`. Tokens that require a follow-up (`( [ { : , |> =>`) simply continue onto the next line. This is the rule today's parenthesis merge already uses (lib.rs:5621-5630), applied uniformly.
- **L2.** A line starting with `-` always begins a new element (unary minus). This matches today's LIST behavior: `LIST {\n 5\n -7\n}` is `[5, -7]` (dump-ir).
- **L3.** A binary operator other than `|>` must have its right operand on the same line. `1 +\n 2` is an error, as today.
- **L4 (indentation is checked, never structural).**
  - (a) A continuation line (starting with `|>` or an operator) must not be indented less than the line where the continued expression starts.
  - (b) A value on the line after `name:` or `=>` must be indented more than that line. For top level this means value indent > 0; `value:\n1` is an error, as today.
  - (c) Within one bracket, elements that start a line all start at the same column.
  - (d) Top-level items start at column 0.
  - (e) Tabs in indentation are rejected (owner question Q5).
- **L5.** A unit is a sequence of top-level items separated per L1.

**Expression precedence** (low to high):
- `|>`, left-associative. Its right-hand side is a pipe form, followed only by another `|>` or the end of the expression.
- Comparisons `== != < > <= >=`, non-associative.
- `+ -`, left-associative.
- `* / %`, left-associative.
- Unary `-`.
- Postfix `.field`; call `name(args)`.

`=>` is not an operator. It exists only in arm and entry contexts. Precedence is owner question Q2. Today every infix operator has the same precedence and is right-associative (split_infix takes the first top-level operator, lib.rs:6981-6999; dump-ir confirms `2*3+1` is `2*(3+1)` and `10-3-2` is `10-(3-2)`).

**Pipe forms:** `f(args)` or `Q/f(args)`, `WHEN {arms}`, `WHILE {arms}`, `THEN {body}`, `HOLD name {exprs}`, `DRAINING`, `.field`, `LATEST {...}`. Tokens after a pipe form that are not `|>` are an error. Today they are silently dropped: ast_pipe_expr_kind reads only up to the first `(...)`, lib.rs:6603, so `x |> f() + 1` loses `+ 1`.

**Primaries:**
- number, string, TEXT
- name, Tag, `Tag[elements]`, `[elements]`, `(expr)`
- `a.b.c`, `PASSED.a.b`
- `LIST[n]? {...}`, `MAP {k => v}`, `SET {...}`, `BYTES[..]? {...}`, BITS forms
- `BLOCK {body}`, `LATEST {exprs}`
- `SOURCE`, `SKIP`, `FLUSH`, `FLUSH {expr}`, `DRAIN {...}`

**Element contexts**

| context | elements |
|---|---|
| unit | `name: expr`, `FUNCTION name(params) { body }` |
| `[...]`, `Tag[...]` | `name: expr`, `...expr` |
| body (FUNCTION, BLOCK, THEN) | `name: expr` bindings, then exactly one result expression last |
| `HOLD name { }` | one or more update expressions (corpus: flush_error_propagation.bn:69-75, persons_pro/RUN.bn:139-151) |
| `LATEST/LIST/SET { }` | `expr`, `...expr` |
| `WHEN/WHILE { }` | `pattern => expr` |
| `MAP { }` | `expr => expr` |
| call args | `name: expr`, `PASS: expr`, `expr` (a lone name is a binder/shorthand) |
| params | `name`, `name: OUT` |

Patterns: literal, TEXT, Tag, `Tag[binders]`, binder name, `__`, negative number.

### 1.4 Parser, arena, spans, recovery

The parser is hand-written recursive descent with Pratt for binary operators. It consumes token indices once, with no string windows (today's `span_for_tokens` is quadratic and mislocates repeated sub-expressions, lib.rs:5945-5959).

**Nodes are final at birth.** They are pushed in post-order and never patched: no `linked_input`, no `Delimiter` placeholders, no `consume_arrow_*`, no `"Field/x"` op strings (lib.rs:6129).

```rust
#[repr(C)] struct Node { kind: NodeKind, flags: u8, _p: u16, a: u32, b: u32 } // 12 bytes; a/b = child local id | Symbol | extra index
struct DefSyntax {                  // one per definition; Arc-shared across revisions while fingerprints match
    kind: DefKind, name: Symbol, key: DefKey,
    nodes: Box<[Node]>, span: Box<[(u32, u32)]>,   // DEF-RELATIVE byte (start, len)
    extra: Box<[u32]>,                              // element lists, args, arms
    children: Box<[(Symbol, DefKey)]>,              // nested definitions (record-literal fields)
    header_fp: u128, shape_fp: u128, lit_fp: u128,
    literals: Box<[(u32 /*local node*/, LitClass)]>,
    diags: Box<[LocalDiag]>,
}
struct UnitSyntax { path: Arc<str>, text: Arc<str>, module: Option<Symbol>, lines: LineTable,
                    defs: Box<[(DefId, u32 /*absolute start*/)]>, diags: Box<[Diag]> }
```

**Per-definition arenas.** The parser writes into a scratch stack. Starting a nested definition pushes a level, and the parent gets a `DefRef` node. At the definition's end the level is copied into boxed slices (~0.1 ms of memcpy for NovyWave). Local node ids and spans are therefore relative to the definition from birth. Tokens are not retained after parsing.

**Recovery.** An element error skips to the next separator at the same bracket depth, using the lexer's partner table. An unclosed opener is closed implicitly when a line at indentation ≤ the opener line's indentation starts a new element at an enclosing level; this is reported once. Column-0 items always resync. Every unit reports all its errors. Today `ParseError` is a single message and the first one aborts the whole project request (session.rs:862-870, boon_parser lib.rs:2101). Probe `two_errors.bn`: 2 positioned errors, other items intact.

**Fenced reparse (error containment).** If a warm reparse produces errors *and* changes definitions outside the edited byte range (for example an unclosed `TEXT {` or `[` that swallows later siblings), re-parse only the edited definition's old range, shifted by the edit, as a single element in its parent context. Every other definition keeps its old `Arc<DefSyntax>`. This costs one extra parse, only on this error path.

Incremental splice parsing is otherwise not needed: whole-unit reparse is ≤ 1.33 ms for the largest units. Revisit only if a unit exceeds ~1 MB.

### 1.5 Definitions, identity, fingerprints

**Definition** = a top-level field, a FUNCTION, or a field of a record literal that is itself the direct value of a definition (recursively). This is the kernel's root-field-path granularity; C9 found 155 on TodoMVC and 1,389 on NovyWave. The prototype finds 174 and 1,301 with a close predicate. The checker panel owns the exact predicate, but it must stay syntactic.

**Identity.**
- `DefKey = hash128(role, module-or-root, [path Symbols], kind, duplicate ordinal)`. A duplicate is also a diagnostic.
- `DefKey → DefId(u32)` through a session-lifetime append-only table; deleted definitions are tombstoned. Dense per-revision indices never appear in keys. Today's M4 key failed exactly on renumbering (a_integration_warm).
- Moving a definition between lowercase files keeps its key.

**Fingerprints** are 128-bit and computed during the parse from the definition's own tokens. A child definition contributes only a marker. Symbol ids are session-stable. No positions are included, so the fingerprints are item-relative by construction.
- `header_fp`: kind, name, parameter names and OUT flags in order; for record-literal definitions, the list of child definition names.
- `shape_fp`: token kinds, symbols and FIRST_ON_LINE flags. Literal bytes are masked, but literal kinds are kept. Type-relevant literal positions (BITS/BYTES widths and sizes, LIST capacity, sized-literal suffixes) are hashed into shape.
- `lit_fp`: the remaining literal bytes, in order.

**Diff classes per DefKey:** Same (positions only), LiteralOnly, Body (shape), Header, Added, Removed.

### 1.6 Divergences from today's parser (owner questions Q1-Q3)

| input | today (measured) | proposed |
|---|---|---|
| `0.1 + 0.2 == 0.3`, `1 / 3 * 3 == 1` (language_surface/current/exact_number_value_algebra.bn) | `0.1 + (0.2 == 0.3)` | conventional. The other 17 multi-operator sites give the same result either way. |
| `LIST {\n 1,\n 2\n}`, `[a: 1,\n b: 2]`, `WHEN {\n A => 1, B => 2\n}` | rejected | accepted (L1) |
| `value: 1\n    2` | value = 2 (1 silently dropped) | error |
| `value:\n  5\n  - 7` | value = -7 (5 dropped) | error |
| `x \|> f() + 1` | `+ 1` dropped | error |
| continuation less indented than its head | binds to the enclosing structure | error (L4a) |
| top-level `name: a\n\|> f()` at column 0 (BUILD.bn) | internal kernel error "owner has no public declaration" | continuation |
| `TEXT { a -- b }` (BUILD.bn uses this) | "unclosed `{`" | literal text |

### 1.7 Tooling on the same syntax crate

- `syntax::highlight(text)` replaces lex_source (boon_editor).
- `syntax::format(path, text)` ports `format_source_unit` (boon_parser lib.rs:4140-4360, used at dev.rs:1045) onto tokens + trivia.
- The language-surface probe (xtask language_surface.rs:474-485 runs `cargo run -p boon_parser --example language_surface_probe`) moves to the new crate.

## 2. Session (warm path)

### 2.1 State

```rust
pub struct Session {
    config: Arc<Config>, interner: Interner,
    units: Vec<UnitState>, unit_by_path: FxHashMap<Arc<str>, UnitId>,
    def_ids: FxHashMap<DefKey, DefId>,                 // append-only
    defs: Vec<DefSlot>,                                // current Arc<DefSyntax>, unit, abs_start, class-of-last-change
    names: NameIndex,                                  // leaf Symbol -> SmallVec<DefId>; (module, fn) -> DefId
    name_users: FxHashMap<Symbol, SmallVec<DefId>>,    // who looked a bare name up (hits AND misses)
    resolved: Vec<Option<Arc<DefResolved>>>,           // refs per local node -> Local|Def|Builtin|Passed|Role|Unresolved
    graph: Graph,                                      // CSR edges + SCCs in topological order
    checks: Vec<Option<Arc<DefCheck>>>, check_memo: SmallLru<(DefId, InputKey), Arc<DefCheck>>,
    frags: Vec<Option<Arc<Fragment>>>, plan: Option<(PlanKey, Arc<MachinePlan>)>, last_good_plan: Option<Arc<MachinePlan>>,
    rev: u64,
}
struct Snapshot { rev: u64, units: Arc<[Arc<UnitSyntax>]>, defs: Arc<[DefSlot]>, graph: Arc<Graph>,
                  checks: Arc<[Option<Arc<DefCheck>>]> }   // immutable; built per revision by copying ~1.4k Arcs (µs)
```

The session is single-threaded: `Send`, not `Sync`, owned by the compile thread. Published `Checked` values hold an `Arc<Snapshot>` and are immutable, so they can cross threads.

### 2.2 Edit to revision

`edit()` replaces unit texts (full text or a splice), marks those units dirty and bumps `rev`. It does no compiling. Today apply_updates discards everything after parse (session.rs:485-486).

Lazily, at the next `check`:
1. Re-parse each dirty unit whole (≈6 ns/byte).
2. Diff definitions by DefKey.
3. Keep the old `Arc<DefSyntax>` for Same definitions and update `abs_start`.

There is no whole-bundle SHA-256 anywhere. Today there are ≥7 passes per keystroke across playground and session: preview.rs:1252, compile.rs:416, 498, session.rs:851-852, boon_parser lib.rs:1558.

### 2.3 Resolution, name index, graph

Resolution runs per definition with class Body/Header/Added, and per definition listed in `name_users[s]` for any leaf name `s` whose `NameIndex` entry changed (a definition added, removed or renamed).
- One pass over the definition's nodes, with a scope stack for params, BLOCK bindings, pattern binders and builtin binder args. Which args are binders comes from the static builtin table.
- Order: local scope, then own-module function, then `Module/fn`, then builtin, then the unique-suffix fallback over the NameIndex. The fallback reproduces boon_typecheck lib.rs:14385-14393 (C9), and NovyWave's only function-containing cycle depends on it.
- Misses are recorded in `name_users` too, so adding `foo` re-resolves every definition that failed to find it.

The graph is rebuilt from per-definition edge lists only when edges changed: CSR plus iterative Tarjan over ≤1.5k nodes and ~3.3k edges, ≈20-50 µs.

### 2.4 Check orchestration: early cutoff and backdating

```text
dirty := {Body, Header, Added} ∪ {defs whose resolution changed}
iface_changed := {}
for group in graph.sccs_topological():          // dependencies first; ~1.4k iterations of a cheap test
    poll(cancel)?                                // per group
    if group ∩ dirty = ∅ and no external dep of group ∈ iface_changed: continue   // reuse
    key := (shape/header fps of members, interface_fp of each external dep)
    if let Some(hit) = check_memo[key]: reuse    // undo, and revisits after cancellation
    else results := checker.check_group(group, poll)?     // checker panel
    for d in group:
        if results[d].interface_fp != old.interface_fp: iface_changed += d
        // else: backdate — dependents' inputs are unchanged, so they are not rechecked
        store checks[d]
```

The reuse key is the exact input tuple (syntax fingerprints plus dependency interface fingerprints). Comparing a freshly recomputed interface to decide the dependents' inputs is Salsa backdating. AGENTS.md's "never final result-type equality" must be reworded to allow this explicitly.

`interface_fp` = hash-consed interface identity (field order ignored, per decision 7) **plus** the display-order hash. Reordering fields then re-renders the dependents' hints without breaking type identity.

Results from a cancelled revision stay in the memo, keyed by content, and are reused if the inputs match.

### 2.5 Literal-only fast path

This is sound because types never depend on values (decision 4); type-relevant literal positions are already in `shape_fp`. For a LiteralOnly definition:
1. Skip resolution and checking; reuse its `DefCheck`.
2. Re-run only its literal validations: duplicate literal arms, numeric range checks for sized literals.
3. Preview: `Lowerer::patch_literals(plan, [(def, local node, value)])`. This needs the plan's constant pool to be a separate `Arc` table (backend contract), so the patch costs O(changed literals) and does not rebuild the plan.

### 2.6 Lanes, scheduling, cancellation

A single compile thread is enough, and simpler than two:

```rust
loop {
    let job = mailbox.take_latest();                      // depth-1 latest-wins, as compile.rs:142-165 today
    session.edit(job.edits);
    let checked = match session.check(&job.cancel) { Ok(c) => c, Err(Canceled) => continue };
    send(LanguageReady { rev, snapshot: project(&checked) });        // lane 1, published immediately
    match session.build(&job.cancel) {                               // lane 2
        Ok(Build::Plan(p)) => send(PlanReady { rev, plan: p }),
        Ok(Build::Errors(_)) => send(PlanFailed { rev }),            // last good plan stays mounted
        Err(Canceled) => continue,
    }
}
```

Today one `CompileOutcome` carries both results after the full verified compile (compile.rs:373-393, preview.rs:1493-1508). That adds ~340 ms (TodoMVC) or ~1.1 s (NovyWave) to edit-to-diagnostics.

**Poll points** (`Poll::tick` checks an `Arc<AtomicBool>` every 4096 steps): per unit in front, per group in check, every 4096 checker steps inside a group, per definition in lowering, every 256 fragments in link. Target stop latency is ≤1 ms p95. Today polls exist only between phases, and the kernel and semantic phases have none (C5).

### 2.7 Last-good policy and containment

- **Syntax errors** in a definition: its diagnostics are published, and dependents keep using its last good interface, so there is no cascade. Its hints are dropped; all other definitions keep theirs.
- **Preview** is whole-plan: a plan is published only for an error-free revision; otherwise the last good plan stays live (as today, session.rs:487-490). Mixing stale fragments into a plan is never allowed.
- **Dev process**: keep the previous language snapshot instead of clearing it on every keystroke (dev.rs:353), shifting positions by the local edit delta until the new revision's snapshot arrives.

### 2.8 Publication

`Checked` exposes:
- diagnostics: DefId + definition-relative span, with absolute file/line/column computed lazily from `abs_start` and the unit line table;
- hints per file: from `DefCheck.node_types` on hintable nodes, rendered with first-appearance field order;
- semantic occurrences: declaration/reference/call/OUT/PASS, from `DefResolved`.

The editor projection keeps a per-definition block cache and sends only changed blocks, each carrying (DefId, version). Delta publication is SE5, and is needed once full snapshots exceed ~64 KB. Estimate: NovyWave's ~20k semantic items at ~150 B is ~3 MB per keystroke if sent whole.

Snapshot matching uses the revision number and drops `SourceBundleDigestV1` (today boon_editor language.rs:117-128 SHA-256s the whole bundle on the dev side).

### 2.9 Consistency guard

Debug builds and CI: after every keystroke of the edit corpus, run a cold compile of the same texts on a scratch session and require identical diagnostics and an identical canonical plan (from-scratch consistency). Release builds skip it.

### 2.10 Memory

Retained per session:
- syntax: 12 B/node plus spans and extra, ≈1 MB for NovyWave;
- texts: 0.43 MB;
- resolutions and checks: ~4-8 B/node plus diagnostics;
- fragments and plan: backend.

Target ≤64 MiB resident for NovyWave and ≤24 MiB for TodoMVC. Today: 383-414 MB and 166-199 MB.

### 2.11 Contracts with the checker and lowering panels

```rust
pub trait Checker { fn check_group(&mut self, cx: &GroupCx<'_>, poll: &mut Poll) -> Result<Vec<DefCheck>, Canceled>; }
pub struct GroupCx<'a> { pub defs: &'a [DefId], pub syntax: &'a [&'a DefSyntax], pub resolved: &'a [&'a DefResolved],
                         pub dep: &'a dyn Fn(DefId) -> InterfaceRef, pub interner: &'a Interner }
pub struct DefCheck { pub interface: InterfaceRef, pub interface_fp: u128, pub node_types: Box<[TypeId]>,
                      pub diags: Box<[LocalDiag]>, pub literal_checks: Box<[LitCheck]> }
pub trait Lowerer {
    fn lower_def(&mut self, d: DefId, s: &DefSyntax, c: &DefCheck, deps: &dyn Fn(DefId) -> &DefCheck, poll: &mut Poll)
        -> Result<(Arc<Fragment>, SmallVec<[DefId; 8]> /*lowering deps read*/), Canceled>;
    fn link(&mut self, frags: &[Arc<Fragment>], cfg: &Config, poll: &mut Poll) -> Result<Arc<MachinePlan>, Canceled>;
    fn patch_literals(&mut self, plan: &Arc<MachinePlan>, changes: &[LiteralChange]) -> Option<Arc<MachinePlan>>;
}
```

The plan must contain no absolute source positions; today's DebugMap has labels only (boon_plan lib.rs:9952-9967). Position-only edits then return the same `Arc<MachinePlan>`, and the playground skips the runtime swap.

## 3. Public API (facade crate `boon_compiler`)

```rust
pub use boon_plan::{MachinePlan, TargetProfile, ProgramRole, ApplicationIdentity, MigrationPredecessorBinding};
#[derive(Clone)] pub struct Source { pub path: Arc<str>, pub text: Arc<str> }
#[derive(Clone)] pub struct Config { pub entry: Arc<str>, pub target: TargetProfile, pub role: ProgramRole,
    pub app: ApplicationIdentity, pub schema_version: u64, pub predecessors: Arc<[MigrationPredecessorBinding]> }
pub enum Edit { Set { path: Arc<str>, text: Arc<str> }, Splice { path: Arc<str>, range: Range<u32>, text: Box<str> },
                Remove { path: Arc<str> }, Rename { from: Arc<str>, to: Arc<str> } }
#[derive(Clone, Default)] pub struct Cancel(Arc<AtomicBool>);   pub struct Canceled;

pub struct Session;   // one program, retained, single-threaded owner
impl Session {
    pub fn new(config: Config, sources: Vec<Source>) -> Session;
    pub fn revision(&self) -> u64;
    pub fn edit(&mut self, edits: impl IntoIterator<Item = Edit>) -> u64;
    pub fn set_config(&mut self, config: Config) -> u64;
    pub fn check(&mut self, cancel: &Cancel) -> Result<Arc<Checked>, Canceled>;   // diagnostics lane
    pub fn build(&mut self, cancel: &Cancel) -> Result<Build, Canceled>;          // preview lane (implies check)
    pub fn last_good_plan(&self) -> Option<Arc<MachinePlan>>;
}
pub enum Build { Plan(Arc<MachinePlan>), Errors(Arc<Checked>) }
impl Checked { pub fn revision(&self) -> u64; pub fn has_errors(&self) -> bool; pub fn files(&self) -> &[Arc<str>];
    pub fn diagnostics(&self) -> impl Iterator<Item = Diagnostic<'_>>;   // severity, code, file, byte span, line, col, message
    pub fn hints(&self, file: u32) -> impl Iterator<Item = Hint<'_>>;
    pub fn occurrences(&self) -> impl Iterator<Item = Occurrence>; }
pub fn check(config: &Config, sources: &[Source]) -> Arc<Checked>;                          // one-shot
pub fn compile(config: &Config, sources: &[Source]) -> Result<Arc<MachinePlan>, Arc<Checked>>; // one-shot
pub struct DistributedSession;   // roles: Vec<(Config, Vec<Source>)>; same edit/check/build; build -> Vec<(ProgramRole, Arc<MachinePlan>)>; checked(role)
pub mod display { DeclId, TypeDisplayNode, Severity }   // small serde types for editor/IPC
pub mod syntax  { highlight, format }
pub mod project { discover }
pub mod debug   { dump_lowered, PhaseTimings, WorkCounters }
```

**Distributed.** One union definition graph over the roles, with role-qualified DefKeys. Cross-role `Role/value` references are ordinary edges, so no fixed point is needed. Today `distributed_compiler.rs` runs 6 + 3·P ≥ 9 legacy checks (C7).

**Plan verification** stays in boon_plan (`verify_plan`). It is called only when loading untrusted artifacts (program_core.rs:446-460), by `boon_cli check`, and via `debug_assert` in test builds.

**Builtin tables** are process-static `LazyLock`, which the small-program targets need.

## 4. Consumer port matrix (every caller)

| # | consumer | uses today | needs from the new API |
|---|---|---|---|
| 1 | playground compile.rs PreviewCompiler (97-120, 488-715) | CompilerSession open/apply_updates/request(EditorDiagnostics, VerifiedPreview), CheckedSourceSyntax, downcast ParseError (546) | boon_compile_service over Session::edit/check/build; parse errors are ordinary diagnostics |
| 2 | compile.rs migration (744-912; fresh CompilerSession per stage per keystroke, 848) | compile_source_bundle | `compile()` for predecessor stages, cached by (stage path, per-unit hash, schema_version) and turned into MigrationPredecessorBinding; target stage = the retained Session with `Config.predecessors` |
| 3 | compile.rs child-program worker (223-345), preview.rs:4568, 4595, 5614 | compile_program_artifact | via program_runtime → `compile()` (SoftwareBounded) |
| 4 | distributed_program.rs:16-44 | compile_distributed_program_bundle_with_client_projection + project_checked_language(ParsedProgram) | DistributedSession; `checked(Client)` for the projection |
| 5 | preview.rs | preview_project_key SHA (1252); Wake::Compiled forwarding (1493-1508); migration Preview/Activate compile synchronously and Activate recompiles (3332, 3356); schema evidence (4144-4145) | revision + unit-hash keys; two outcome messages; cached stage plans |
| 6 | readiness.rs:175-178 | Arc<MachinePlan> into prepare_machine_plan_build | unchanged |
| 7 | dev.rs | full bundle per keystroke (354-358), clear_language (353), format_source_unit (1045), materialize_file | optional Splice deltas; keep stale snapshot; `syntax::format`; `syntax::highlight` |
| 8 | catalog.rs:157, 247 | compiler_source_units_for_manifest_source | plain manifest reader (fixture registry) or `project::discover` |
| 9 | protocol.rs:8, 165 | boon_checked TypeDisplayNode/DeclId/DiagnosticSeverity | `boon_compiler::display` |
| 10 | verify.rs:4879-4909 bounded-starter-source-compile (persons_pro budget 4/8 ms) | child compile timing | small-program target (§5) |
| 11 | boon_editor language.rs | project_checked_language/_unit_native/_parse_error (271-458), boon_typecheck::project_type_hints* (358-365), lex_source (144), digest matching (117-128) | one `project_language(rev, &Checked)` with per-definition blocks; hints from `Checked::hints`; revision matching |
| 12 | boon_program_runtime program_core.rs | compile_sealed_machine_plan (517), diagnose_runtime_source_units re-run on failure (526), compile_distributed_* (629-639), COMPILER_ID, plan_hash / source_bundle_digest_v1 / profile (694-731) | `compile()`, whose Err carries diagnostics (no second pass); DistributedSession; the artifact layer computes BLAKE3 over the canonical plan encoding at encode; `verify_plan` stays at from_content_artifact |
| 13 | boon_app_package build.rs:162, 420-438 | trusted distributed bundle via program_runtime | same via #12; package keeps its own content hash |
| 14 | boon_app_server, boon_app_cli | load packages only | artifact format only |
| 15 | boon_runtime lib.rs:1-5, 320-520, 2281-2312 | LiveRuntime::from_source*/from_project* compile inside the runtime; manifest source helpers | drop the compiler dependency; callers (boon_cli run, phase0) do `compile()` then `LiveRuntime::from_shared_machine_plan`. The web host (wasm) then stops linking the compiler. |
| 16 | boon_host_runtime migration_scenario.rs:2-5, 576-600 | compile_machine_plan per stage + manifest units | `compile()` + reader |
| 17 | boon_cli lib.rs | check (118-162 VerifiedCheck + plan.verification()), dump-plan (164-177), dump-ir (179-191), run (96-116); compiler_sample.rs (2,015 lines) | `compile()` + explicit `verify_plan`; `debug::dump_lowered`; `compile-bench` (cold) and `compile-bench --warm`, which drives boon_compile_service over budgets/compiler_edits.toml |
| 18 | boon_behavior_harness tests/artifact_oracle.rs (test-flat-oracle) | compile_artifact_oracle_pair | delete; scenarios run on `compile()` |
| 19 | boon_phase0_baseline fixtures.rs, runner.rs | compile_machine_plan, compiler_source_units_for_path | `compile()` + discover |
| 20 | boon_plan_executor tests (foundations_vertical, map_set_runtime, map_set_cross_target, fjordpulse_server_search) + machine.rs:36634 test module | compile_machine_plan(source_text), diagnose_runtime_source_units | `compile()` / `check()` |
| 21 | boon_web_host (wasm32 dev-dep tests/startup.rs) | boon_compiler in tests | new crates must build for wasm32 (std only, no required threads) |
| 22 | boon_server_runtime tests (5), boon_http_runtime tests/loopback.rs, boon_host_runtime tests (3), boon_wellen_host tests/waveform.rs | via program_runtime | no direct change |
| 23 | xtask | compiler_performance/allocator/interactions/work_sample/producer spawn `boon_cli compiler-sample` (971-976, 680-700, 1234-1281); language_surface.rs:474-485; architecture.rs, dependency_classifier.rs, verify_phase0.rs, packed_site_inventory.rs (gates naming old crates, types and the receipt spine); gates.rs verify-all (handoff manifest includes verify-architecture) | new bench JSON; probe in the new syntax crate; delete the compiler gates; update verify-architecture |
| 24 | boon_compiler tests/*.rs (6 files), boon_editor tests (check_program) | old API | diagnostics corpus (`*.bn` + `*.expect`) and scenario tests |

## 5. Performance model and warm targets

**Diagnostics.** `T_diag = F·bytes(edited unit) + r·nodes(R) + [graph 0.05 ms if edges changed] + Σ_{d∈C} c·nodes(d) + T_pub + T_ipc`
- F = 6 ns/B (measured: 1.24 ms for 208 KB)
- r ≈ 40 ns/node
- c = 2 µs/node checker target. This matches the cold targets: TodoMVC 7.6k nodes → 15 ms, NovyWave 29.7k → 60 ms.
- R = definitions with Body/Header/Added change plus the users of any changed name
- C = R ∪ dependents reached through interface-fingerprint changes (with cutoff)
- T_pub ≈ 0.1 ms + 0.5 µs per changed hint or diagnostic
- T_ipc ≈ 0.3 ms for 2 hops with deltas
- Average definition: 44 nodes (TodoMVC), 23 (NovyWave)

**Plan.** `T_plan = T_diag + Σ_{d∈L} l·nodes(d) + T_link`
- l ≈ 3 µs/node lowering target
- L = definitions whose DefCheck or lowering deps changed
- T_link ≈ 1 µs/definition (1.3k definitions → 1.3 ms), or a patch.

Cold is warm with an empty store.

**Warm targets** (p95, both fixtures; the edited unit may be NovyWave RUN.bn):

| edit class | compiler edit→diagnostics | end-to-end (incl. projection + IPC) | edit→plan |
|---|---:|---:|---:|
| whitespace / comment / position-only | 2 ms | 4 ms | no-op (same Arc) ≤1 ms |
| literal-only | 2.5 ms | 5 ms | 5 ms (constant patch) |
| body edit, interface unchanged | 5 ms | 8 ms | 20 ms |
| interface change, cone ≤ 40 definitions | 12 ms | 16.7 ms | 40 ms |
| add / remove / rename definition | 8 ms | 12 ms | 30 ms |
| incomplete syntax (every keystroke) | 3 ms | 5 ms | last good kept |
| undo to previous | 2.5 ms | 5 ms | 5 ms (plan memo, last 4) |
| hub / cross-unit (Theme types) | ≤16.7 ms TodoMVC; NovyWave bounded by cold check ≤60 ms | — | ≤100 ms |

Other targets:
- Supersession stop: ≤1 ms p95, ≤2 ms max.
- Child-program starter compile in-process: ≤1 ms p95, ≤2 ms max (end-to-end gate is 4/8 ms).
- Cold front: NovyWave ≤4 ms (measured 2.6), TodoMVC ≤1.2 ms (0.64), counter ≤0.05 ms (0.021). Resolve adds ≤1.5 ms on NovyWave.

## 6. Crate layout and cutover

New crates live side by side with the old compiler until cutover:
- `boonc_syntax`: lexer, parser, arena, fingerprints, formatter, highlighter, probe, discovery
- `boonc_session`: session, snapshots, resolver, name index, graph, orchestration, publication
- checker and lowering crates (sibling panels)
- `boon_compile_service`: latest-wins worker and lanes, used by the playground preview process **and** the warm bench, so the gate measures the product path

At cutover the `boon_compiler` facade is re-pointed. The old compiler is renamed to `boon_compiler_legacy` and kept only as a test-only differential oracle, together with an AST-equivalence oracle (normalized trees) and a divergence ledger for §1.6.

## owner_questions
- **question**: Adopt one uniform newline rule for all brackets and the top level? A line break ends an element unless the next line starts with ) ] } , |> => or a binary operator other than `-`. Tokens that need a follow-up ( [ { : , |> => continue onto the next line. Indentation is checked but never decides structure. | **options**: (a) Uniform rule. It accepts `LIST {\n 1,\n 2\n}`, `[a: 1,\n b: 2]`, comma-separated arms inside multi-line blocks, and column-0 `|>` continuation after a top-level field (BUILD.bn style). It turns four silent reinterpretations into positioned errors: `value: 1\n    2` (today value = 2), `value:\n 5\n - 7` (today -7), `x |> f() + 1` (today `+ 1` dropped), and continuation lines less indented than their head. (b) Replicate today's behavior construct by construct, including the silent drops. | **recommendation**: (a) | **why**: A prototype parser accepts all 141 current example files unchanged under (a). Option (b) would mean porting known silent-drop bugs and a 3-merge-pass, 4-repair-pass structure into the new parser. Every divergence is either input rejected today or input silently misparsed today.
- **question**: Operator precedence: switch from today's flat right-associative infix to conventional precedence? That is `* / %` above `+ -`, above non-associative comparisons, above left-associative `|>`, with `+ - * /` left-associative. | **options**: (a) Conventional. (b) Keep flat right-associative: `2 * 3 + 1` = 8 and `10 - 3 - 2` = 9. | **recommendation**: (a) | **why**: dump-ir confirms today `2*3+1` parses as `2*(3+1)` and `10-3-2` as `10-(3-2)`. Of the 19 multi-operator sites in examples, only language_surface/current/exact_number_value_algebra.bn changes meaning, and it clearly intends the conventional reading: `0.1 + 0.2 == 0.3` today parses as `0.1 + (0.2 == 0.3)`.
- **question**: Should `--` inside a TEXT { } body be literal text rather than the start of a comment? | **options**: (a) Literal text: TEXT bodies are raw up to the matching brace. (b) Keep today's behavior, where `TEXT { a -- b }` is 'unclosed `{`'. | **recommendation**: (a) | **why**: examples/novywave/BUILD.bn (and the todo_mvc_physical BUILD.bn generator) writes `TEXT { -- Generated from {icons_directory} ... }`. Raw mode also makes highlighting correct. No current example depends on (b).
- **question**: Project membership without examples/manifest.toml: a program's units are the entry file, plus the root-namespace lowercase files next to it, plus the transitive closure of `Module/...` references resolved against uppercase-stem files under the project root. BUILD.bn is always its own program. Is that the rule? | **options**: (a) Reference closure (above); hosts may always pass explicit unit lists. (b) An explicit per-project file list in a non-Boon file. (c) Keep reading the global manifest. | **recommendation**: (a) | **why**: It reproduces the manifest's source_files for every manifest entry, including the 3 fjordpulse roles. The only differences are two BUILD-generated Generated/Assets.bn modules that nothing references. It needs no new syntax. It removes 2.7 ms and ~1,300 readlink calls from every by-path compile, and removes checkout-path-dependent identity.
- **question**: Reject tab characters in indentation? | **options**: (a) Error: 'indent with spaces'. (b) Accept tabs as width 1 (today's behavior). | **recommendation**: (a) | **why**: The new indentation-consistency checks need unambiguous columns. The corpus contains no tabs, and mixed tabs and spaces could otherwise pass the consistency check.
- **question**: `-` stays an identifier character, so `x-1` is one identifier and becomes a strict 'unknown name' error. Keep it, with a targeted hint? | **options**: (a) Keep the lexing (syntax unchanged) and add the diagnostic hint 'did you mean `x - 1`?'. (b) Stop treating `-` as an identifier character. | **recommendation**: (a) | **why**: The owner said syntax is unchanged. No current identifier contains `-`, so (b) is possible later, but it is a syntax change.
- **question**: Playground behavior: publish diagnostics and hints (the language snapshot) as soon as checking finishes, before the preview plan. Also keep and position-shift the previous snapshot in the dev window instead of clearing it on every keystroke. | **options**: (a) Two lanes plus a stale-but-shifted snapshot. (b) Today: one message after the verified compile, and the snapshot cleared on every keystroke (dev.rs:353). | **recommendation**: (a) | **why**: Today diagnostics wait ~340 ms (TodoMVC) or ~1.1 s (NovyWave) for the back half, and under continuous typing none appear at all. The last good preview stays mounted either way.

## risks
- **risk**: The new layout rule or the precedence change silently alters the meaning of some program that is not covered by examples. | **mitigation**: Run an AST-equivalence oracle (normalized trees) against the old parser on every example and on a layout-probe corpus (design/probes, design/feprobes), with a checked-in divergence ledger that lists every intended difference. The parser emits errors, never reinterpretations, for the four known silent-drop shapes.
- **risk**: A fingerprint omits something that affects checking, so reuse becomes unsound (today's M4 key failed this way). | **mitigation**: Fingerprints hash the definition's complete token stream: kinds, symbols, line flags, and type-relevant literal positions. Debug builds and CI run the from-scratch consistency guard after every keystroke of the edit corpus and fail on any warm-vs-cold difference.
- **risk**: Unique-suffix bare-name resolution creates non-local invalidation: adding a field `foo` anywhere changes how `foo` resolves elsewhere. | **mitigation**: A reverse `name_users` index records both hits and misses. Tests cover add, remove and rename of every definition kind against resolution in other definitions.
- **risk**: An unclosed `TEXT {` or bracket typed mid-edit swallows later definitions, so diagnostics and hints collapse while the user types. | **mitigation**: When errors spill outside the edited range, a fenced reparse re-parses only the edited definition's old range. For cold parses, indentation-guided implicit closing is used. Test: type every example character by character and assert that definitions outside the edit keep their syntax.
- **risk**: Hub edits (Theme or store record shapes) reach large dependent cones and exceed 16.7 ms on NovyWave. | **mitigation**: The worst case is bounded by the cold check target (≤60 ms NovyWave). If the per-class p95 fails, publish the edited definition's diagnostics first and stream the rest of the cone (a P2 item). Warm latency is reported per edit class, not only as one aggregate.
- **risk**: Full language snapshots are sent through IPC on every keystroke (estimated ~3 MB for NovyWave semantic items), which dominates end-to-end latency. | **mitigation**: SE5 per-definition block deltas with version stamps. Gate: payload for a literal keystroke ≤ 4 KB; the whole-snapshot size is measured before and after.
- **risk**: Consumers depend on digests being removed (source_bundle_digest_v1, plan_hash, SourceBundleDigestV1 matching). | **mitigation**: The artifact layer (boon_program_runtime, boon_app_package) computes a BLAKE3 content hash over the canonical plan encoding only when it persists or loads artifacts. The language snapshot matches by revision number. verify_plan stays at from_content_artifact.
- **risk**: The distributed union graph gives different results than the legacy per-role fixed point. | **mitigation**: Differential tests on fjordpulse and the server-runtime tests (in_process, loopback, session auth, http contract) against the legacy oracle until cutover. Any intended differences are recorded in the divergence ledger.
- **risk**: Long sessions grow the append-only interner with symbols from intermediate keystrokes. | **mitigation**: Count unused symbols. Past 1 M, rebuild the session cold on idle (~30-75 ms). The 8 MiB RSS-growth budget across the edit corpus keeps this measured.
- **risk**: The discovery heuristics (role directories, the `X/` sibling directory) do not fit a future project layout. | **mitigation**: Discovery is used only by by-path tools. Hosts and the playground pass explicit units. The compiler reports `unknown module X` with the path it searched, and the CLI accepts extra `--unit` paths.

## work_items
- **id**: FE1 | **title**: Lexer, interner, tooling mode | **description**: Byte-class single-pass lexer producing SoA tokens: kind, start, symbol, FIRST_ON_LINE, indent, bracket partner, plus a line table. TEXT raw mode with interpolation tokens. Inline error tokens for removed spellings. A session-global append-only interner with builtins pre-interned in fixed order. A tooling mode that emits trivia and TEXT chunks for highlighting. | **size**: S | **acceptance**: All 141 current examples lex. NovyWave lexes at ≥300 MB/s single-thread. A fuzz run (random bytes and example mutations) never panics. Tokens plus trivia cover every byte. highlight() classifies TEXT bodies as text. | **depends_on**: 
- **id**: FE2 | **title**: Parser with layout rules L1-L5, per-definition arenas and recovery | **description**: Recursive descent with Pratt for binary operators. Final-at-birth 12-byte nodes in per-definition arenas with definition-relative spans. All node kinds: patterns, BYTES/BITS/MAP/SET/DRAIN, PASS/PASSED, OUT, spread. Element-level recovery with indentation-guided implicit closing, and multi-error reporting. | **size**: L | **acceptance**: Zero diagnostics on all current examples and both BUILD.bn files. The layout-probe corpus produces the expected diagnostics. Cold front (lex+parse+defs) ≤4 ms NovyWave and ≤1.2 ms TodoMVC. The two_errors probe reports both errors with other items intact. | **depends_on**: FE1
- **id**: FE3 | **title**: Definitions, DefKey, fingerprints, diff, fenced reparse | **description**: Definition predicate: top-level fields, FUNCTIONs, and fields of definition-valued record literals. 128-bit header/shape/literal fingerprints, with type-relevant literals hashed into shape. Diff classes per DefKey. Fenced reparse of the edited definition when errors spill outside the edit. | **size**: M | **acceptance**: Moving text by lines changes 0 definitions. A literal edit is LiteralOnly. A local rename changes 1 definition (Body). Adding a field changes 2. NovyWave RUN.bn reparse + diff ≤1.5 ms. Typing each example character by character never changes the syntax of definitions outside the edited one. | **depends_on**: FE2
- **id**: FE4 | **title**: Divergence ledger and AST-equivalence oracle | **description**: Normalized tree comparison between the old parser (test-only) and the new parser on every example and probe. tests/compiler/divergences.toml lists precedence, the layout rules, TEXT `--`, and silent-drop-to-error cases pending owner answers Q1-Q3. | **size**: M | **acceptance**: Every example is equivalent or explained by a ledger entry. CI fails on an unlisted divergence. | **depends_on**: FE2
- **id**: FE5 | **title**: Project discovery by reference closure | **description**: project::discover(entry) implementing the root, namespace-directory and module-closure rules. Wire it into boon_cli, phase0 and the host migration-scenario runner. Move ExampleManifest validation to xtask only. | **size**: S | **acceptance**: Reproduces the manifest source_files of every entry, except the unreferenced Generated/Assets.bn units. strace of a counter compile shows no manifest.toml read and no readlink storm. Counter front ≤0.05 ms. | **depends_on**: FE1
- **id**: FE6 | **title**: Resolver, name index, module table, dependency graph | **description**: Per-definition resolution: scopes for params, bindings, pattern binders and builtin binder args, then own module, Module/fn, builtin, and the unique-suffix fallback. NameIndex plus a name_users reverse index covering both hits and misses. CSR graph and iterative Tarjan SCCs. | **size**: M | **acceptance**: Resolution matches the old checker's bindings on all examples (differential). Adding or removing a definition re-resolves only its name users. Full resolve + graph ≤1.5 ms on NovyWave. | **depends_on**: FE3
- **id**: SE1 | **title**: Session core with early cutoff and backdating | **description**: Session and Snapshot types, the append-only DefId store, and per-definition stores (resolved, checks, fragments). SCC-ordered check orchestration with an exact-input reuse key, interface-fingerprint backdating, a bounded memo for undo and cancelled revisions, and the Poll API (4096-step granularity). Last-good interface for definitions with syntax errors. | **size**: L | **acceptance**: The from-scratch consistency guard passes on every keystroke of budgets/compiler_edits.toml. Body edits recheck exactly 1 definition when the interface is unchanged. Warm per-class targets from the design (§5) are met with the checker panel's check_group. | **depends_on**: FE6 and the checker panel's check_group
- **id**: SE2 | **title**: Literal-only fast path and incremental lowering bookkeeping | **description**: Skip checking for LiteralOnly definitions and run their literal validations. Track the lowering deps each fragment declares, relower only definitions whose check or lowering deps changed, and call patch_literals with a separate constant-pool Arc. Return the same plan Arc for position-only edits. | **size**: M | **acceptance**: A literal keystroke in NovyWave RUN.bn produces diagnostics in ≤2.5 ms and a plan in ≤5 ms. A whitespace keystroke returns an Arc-identical plan. Warm and cold canonical plans are equal after the corpus. | **depends_on**: SE1 and the backend panel's Lowerer
- **id**: SE3 | **title**: Publication: Checked API and editor projection port | **description**: Checked with lazy absolute spans, hints and occurrences. The boon_compiler::display types, which replace the boon_checked DeclId, TypeDisplayNode and DiagnosticSeverity used by the IPC protocol. Port boon_editor language.rs to a single project_language(rev, &Checked). This replaces boon_typecheck::project_type_hints* and the SHA digest matching. | **size**: M | **acceptance**: The editor shows diagnostics, hints and navigation on all examples, with the same hint set as today modulo decision 7 display order. boon_editor has no dependency on boon_parser, boon_typecheck or boon_checked. | **depends_on**: SE1
- **id**: SE4 | **title**: Compile service crate and playground port | **description**: boon_compile_service: a depth-1 latest-wins mailbox, one compile thread, and LanguageReady/PlanReady/PlanFailed messages. Port the playground: compile.rs, the preview.rs forwarding, dev.rs keeping the stale shifted snapshot, and deletion of preview_project_key/source_key SHA-256. Content-keyed caching of migration predecessor stage plans, also used by Preview/Activate. | **size**: M | **acceptance**: The native playground reports edit-to-language p95 ≤16.7 ms on TodoMVC across the corpus. In-flight supersession stop p95 ≤1 ms is measured by starting a compile and cancelling it at a random point. persons_pro keystrokes compile only the target stage. The native GPU handoff gates still pass. | **depends_on**: SE1, SE3
- **id**: SE5 | **title**: Delta publication of language snapshots | **description**: Per-definition hint and occurrence blocks with (DefId, version) and a dev-side block cache. Send only changed blocks. Resync the full snapshot on reconnect. | **size**: M | **acceptance**: The IPC payload for a literal keystroke on NovyWave is ≤4 KB, and full-snapshot size is reported in the bench. End-to-end literal edit-to-hints p95 ≤5 ms. | **depends_on**: SE4
- **id**: SE6 | **title**: Warm benchmark driver and consistency guard | **description**: boon_cli compile-bench --warm drives boon_compile_service over budgets/compiler_edits.toml in settled and typing (60 ms) modes and reports per-class p50/p95/max, stop latency, RSS growth and CPU per keystroke. The debug/CI consistency guard compares warm and cold results after each keystroke. | **size**: M | **acceptance**: Reports the budget's warm section. The driver uses exactly the same service object as the playground. The guard is green on the corpus. | **depends_on**: SE4
- **id**: IN1 | **title**: Distributed joint compile on the session | **description**: DistributedSession: role-qualified DefKeys, one union graph, cross-role Role/value references as edges, and a client projection through checked(Client). Port boon_program_runtime's distributed functions and the playground's distributed_program.rs. | **size**: L | **acceptance**: The fjordpulse client/session/server plans and scenarios pass. The server-runtime, http, web-host and wellen tests pass. There are no legacy checker calls. Distributed warm edit-to-diagnostics is within 2x of single-role. | **depends_on**: SE1, SE3
- **id**: IN2 | **title**: Port one-shot consumers | **description**: Port boon_program_runtime child compile: use the diagnostics carried in compile() Err instead of the diagnose re-run, and hash artifacts with BLAKE3 at encode. Port boon_app_package and boon_host_runtime migration_scenario. Decouple boon_runtime from the compiler by moving from_source/from_project out. Port boon_cli check, dump-plan, dump-ir and run; phase0 baseline; plan_executor tests; web-host, server, http, host and wellen tests. Port the formatter and highlighter consumers. | **size**: L | **acceptance**: The workspace builds with no crate depending on the old compiler except the test-only legacy oracle. The starter-source child compile is ≤1 ms p95 in-process and the persons_pro bounded-starter gate passes. boon_web_host no longer links compiler code. | **depends_on**: SE1, SE2
- **id**: IN3 | **title**: xtask port | **description**: Rewrite compiler_performance, allocator, interactions, work_sample and producer against the new bench JSON. Move the language-surface probe to boonc_syntax. Delete the compiler parts of architecture.rs, dependency_classifier.rs, verify_phase0.rs and packed_site_inventory.rs (receipt-spine and crate-name gates). Update verify-architecture in the handoff manifest. | **size**: M | **acceptance**: cargo xtask verify-all --check-existing passes with the new compiler. No xtask gate greps compiler source for type or receipt names. | **depends_on**: SE6, IN2
- **id**: IN4 | **title**: Cutover deletion | **description**: Delete the old front end, session and compilation db, the source-loading helpers, compiler_sample.rs, the behavior-harness flat oracle, and the old compiler crates once the differential oracle is retired. | **size**: M | **acceptance**: The deleted crates are absent from the workspace. All gates, examples and scenarios pass. Cold and warm budgets pass on both lanes. | **depends_on**: IN1, IN2, IN3

## deletions
- **what**: boon_parser (line-based parser, 3 line-merge passes, 4 repair passes, ValidationIndex and policy validators, SHA-256 identity and route digests, module string rewriting and id repacking, assembled-program route) | **size**: 14,209 lines
- **what**: boon_syntax (old serde AST DTO crate with String payloads, usize ids, "Field/x" op strings) | **size**: 1,962 lines
- **what**: boon_compiler session.rs request tables plus boon_compilation_db (typed request evaluator; only parse and link are retained) | **size**: 1,906 + 1,964 lines
- **what**: boon_compiler source loading: manifest lookup, CARGO_MANIFEST_DIR logical paths, canonicalize loops (lib.rs:1727-1937) | **size**: ~210 lines
- **what**: boon_contract CanonicalSourceBundleV1, SourceBundleDigestV1 and text catalog on the compile path (artifact layer hashes its own content) | **size**: up to 1,632 lines
- **what**: boon_example_manifest as a dependency of boon_compiler and boon_runtime (stays as the xtask/playground fixture registry) | **size**: 2 dependency edges
- **what**: boon_editor use of boon_typecheck::project_type_hints*, boon_parser lex_source and SHA digest matching | **size**: ~150 lines
- **what**: Playground preview_project_key/source_key/migration_project_key SHA-256 identity and per-stage fresh CompilerSession (compile.rs:744-1088) | **size**: ~350 lines
- **what**: boon_runtime LiveRuntime::from_source*/from_project* compile helpers and its boon_compiler dependency | **size**: ~250 lines
- **what**: boon_program_runtime diagnose-on-failure re-parse/re-check path and compile-time double plan verification | **size**: ~60 lines
- **what**: boon_cli compiler_sample.rs (replaced by compile-bench) | **size**: 2,015 lines
- **what**: boon_behavior_harness test-flat-oracle feature and artifact_oracle tests; boon_compiler tests/*.rs (6 files) replaced by the diagnostics corpus | **size**: ~2k lines
- **what**: xtask compiler gates: architecture.rs compiler/receipt checks, dependency_classifier.rs, packed_site_inventory.rs, verify_phase0.rs compiler parts; compiler_interactions.rs rewritten | **size**: ~8-10k lines
- **what**: boon_native_playground unused normal dependency on boon_typecheck; boon_parser and boon_checked dependencies | **size**: 3 dependency edges

## evidence
- Prototype scratchpad/design/fe_proto.rs (rustc -O, i7-9700K): NovyWave 433,371 B lex+parse+defs 2.61-2.64 ms, TodoMVC 0.64 ms, counter 0.021 ms, all 140 example files (1.08 MB) 6.9 ms; warm reparse+fingerprint+diff novywave/RUN.bn 1.24 ms, NovyView.bn 1.33 ms, todo RUN.bn 0.23 ms
- Prototype acceptance: `fe_proto check $(find examples -name '*.bn')` gives files=142 with_errors=1 (only language_surface/future/where_contracts.bn, which is expected to be rejected); both BUILD.bn files parse
- Prototype fingerprint diff: insert 2 lines at file top → 0 changed defs; literal → 1 literal-only; local rename → 1; new store field → 2
- Corpus scan: 533 lines start with `|>` (473 at the same indentation, 60 deeper, 0 shallower), 2 with `==`, none with other operators; no line ends with a binary op, `,` or `|>`; no `-` in identifiers; no tabs
- Precedence: dump-ir of design/probes/prec_2___3___1_.bn shows infix(2, *, infix(3, +, 1)); prec_10___3___2_.bn shows infix(10, -, infix(3, -, 2)); cause is split_infix taking the first top-level operator (boon_parser/src/lib.rs:6981-6999)
- Silent drops: dump-ir of feprobes/l2.bn (`value:\n 5\n - 7`) has field value = -7 with 5 unused; probes/nl_unexpected_indent.bn (`value: 1\n    2`) has value = 2
- Layout today: ast_statement_block builds children from indentation (boon_parser lib.rs:5786-5815); only `(`, BYTES and DRAIN lines merge (4391-4397); parenthesis separator rule at 5621-5630; last top-level pipe split (6950-6964); pipe args read only up to the first parenthesis (6603, 6681-6690)
- Lexer today: identifiers continue with `-` and `/` (lib.rs:5275-5283); `--` comments are lexed before TEXT recognition (5327-5333), so probes/text_dash.bn fails with 'unclosed `{`'; heap lexeme per token (5393)
- First parse error aborts the whole project: session.rs:862-870 returns Err; ParseError is one message (boon_parser lib.rs:2101-2106)
- BUILD.bn: `boon_cli check examples/novywave/BUILD.bn` fails with the same 'dense kernel checked construction does not cover the complete project' error as probes/pipe_cont_col0.bn
- Discovery: a python reference-closure check reproduces examples/manifest.toml source_files for all entries; the only differences are unreferenced novywave/Generated/Assets.bn and todo_mvc_physical/Generated/Assets.bn
- Module naming rule: boon_parser lib.rs:3987-3999; manifest-driven loading boon_compiler/src/lib.rs:1876-1901 and CARGO_MANIFEST_DIR root 1816-1825
- NovyWave RUN.bn is dominated by one 4,158-line top-level item `store: [` (47 items total), so top-level item granularity is not enough; definitions are root field paths (C9: 155 TodoMVC, 1,389 NovyWave)
- HOLD bodies hold multiple update expressions: examples/flush_error_propagation.bn:69-75, examples/persons_pro/RUN.bn:139-151, examples/host_service_effects.bn:84-100
- Playground: single CompileOutcome after the full compile (compile.rs:373-393); EditorDiagnostics then VerifiedPreview (compile.rs:537-606); forwarded on Wake::Compiled (preview.rs:1493-1508); preview_project_key SHA at preview.rs:1252 and compile.rs:416, 963; dev clears language and sends all units per keystroke (dev.rs:353-358); format_source_unit at dev.rs:1045
- Migration: fresh CompilerSession per stage per keystroke (compile.rs:744-811, 848); Preview/Activate compile synchronously on the main loop (preview.rs:3332, 3356)
- Session invalidation: apply_updates clears diagnostics/checked (session.rs:485-486); request flow (session.rs:711-816); the last verified plan is kept (487-490)
- boon_editor: project_type_hints* (language.rs:358-365), lex_source (144), SHA digest matching (117-128); protocol.rs:8/165 uses boon_checked display types
- Program runtime: compile_sealed_machine_plan plus diagnose_runtime_source_units re-run on failure (program_core.rs:517-539); untrusted artifact load verifies (446-460); artifact digest at 694-731
- boon_runtime compiles inside LiveRuntime::from_source*/from_project* (lib.rs:397, 505) with only boon_cli run and phase0 fixtures as external callers
- xtask spawns `boon_cli compiler-sample` (compiler_performance.rs:971-976, compiler_interactions.rs:680-700, 1234-1281); the language-surface probe runs `cargo run -p boon_parser --example language_surface_probe` (language_surface.rs:474-485)
- persons_pro budget: bounded_starter_source_compile_p95 = 4.0, max = 8.0 (examples/persons_pro.budget.toml:5-6), measured by playground verify.rs:4879-4909
- Earlier lexbench calibration: 352 MB/s including interning (NovyWave 1.23 ms, warm RUN.bn relex 0.49 ms)

## perf_targets
- **metric**: Cold front end (lex+parse+defs), NovyWave 433 KB | **target**: ≤4 ms single-thread (resolve ≤1.5 ms extra) | **basis**: Prototype measured 2.61-2.64 ms; today 65 ms
- **metric**: Cold front end, TodoMVC physical 102 KB | **target**: ≤1.2 ms | **basis**: Prototype measured 0.64 ms; today 18 ms
- **metric**: Cold front end, counter | **target**: ≤0.05 ms | **basis**: Prototype measured 0.021 ms; today 3.6 ms, 2.7 ms of it manifest loading
- **metric**: Lexer throughput | **target**: ≥300 MB/s | **basis**: lexbench 352 MB/s; prototype lexer 288-297 MB/s
- **metric**: Warm whole-unit reparse + fingerprint + diff, largest unit (novywave/RUN.bn 208 KB) | **target**: ≤1.5 ms | **basis**: Measured 1.24 ms (NovyView.bn 1.33 ms); today 41.7-49.1 ms
- **metric**: Warm edit→diagnostics, position-only / literal-only (compiler side) | **target**: p95 ≤2 ms / ≤2.5 ms | **basis**: Reparse 1.24 ms + diff + publish 0.1 ms; literal path skips checking (decision 4)
- **metric**: Warm edit→diagnostics, body edit keeping the interface | **target**: p95 ≤5 ms | **basis**: Model: front 1.24 ms + one definition at 2 µs/node (avg 23-44 nodes, largest ~1.5k) + publish
- **metric**: Warm edit→diagnostics, interface change with cone ≤40 definitions | **target**: p95 ≤12 ms compiler, ≤16.7 ms end-to-end | **basis**: Model: 1.5 ms + 40 × 0.1-0.25 ms
- **metric**: Warm edit→diagnostics, incomplete-syntax keystrokes | **target**: p95 ≤3 ms | **basis**: Reparse, plus fenced reparse only when errors spill; broken definition uses its last good interface
- **metric**: Warm edit→plan (literal / body / interface) | **target**: p95 ≤5 / ≤20 / ≤40 ms; all classes p95 ≤100 ms | **basis**: Constant patch; relower changed definitions at 3 µs/node + link ≈1 µs/definition
- **metric**: Position-only edit→plan | **target**: No-op: same Arc<MachinePlan>, ≤1 ms | **basis**: Plan holds no absolute positions (DebugMap is labels only)
- **metric**: In-flight supersession stop latency | **target**: p95 ≤1 ms, max ≤2 ms | **basis**: Poll per group, per definition, every 4096 solver steps, every 256 fragments; today up to a whole ~450 ms solve
- **metric**: Child-program starter-source compile in-process | **target**: p95 ≤1 ms, max ≤2 ms | **basis**: persons_pro end-to-end gate is 4/8 ms; needs process-static builtin tables
- **metric**: Retained session memory | **target**: NovyWave ≤64 MiB, TodoMVC ≤24 MiB; ≤8 MiB growth across the edit corpus | **basis**: Syntax ≈1 MB plus per-node facts; today 383-414 MB and 166-199 MB resident
- **metric**: Language snapshot IPC payload per literal keystroke | **target**: ≤4 KB (deltas) | **basis**: Full snapshot estimated at ~3 MB of semantic items for NovyWave

---

# Adversarial review

## area
frontend_session (lexer/parser/definitions/discovery, warm session, integration and public API)

## verdict
needs_changes

## major_issues
- **issue**: The 'Same' diff class keeps the old Arc<DefSyntax>, so spans and layout diagnostics go stale after whitespace or comment edits inside a definition. shape_fp hashes token kind, symbol and FIRST_ON_LINE only. Offsets, intra-line spacing, indentation and local diagnostics are not hashed, yet spans are definition-relative. | **evidence**: §2.2 step 3 and §1.5 ('Same (positions only)'). I reproduced it with the prototype: a copy of design/fe_proto.rs (scratchpad/review_fe/fe_rev.rs, EDIT=space) inserts 3 spaces mid-line plus a '-- note' line inside one novywave/RUN.bn definition. It reports changed_defs=0, so that definition keeps its old span table and diagnostics/hints below the edit move by one line or three columns. The L4 indentation errors are parse-local and indent is not hashed, so de-indenting a continuation line (L4a) is also 'Same' and the new error is dropped. | **fix**: Use the fingerprints only to decide whether DefResolved/DefCheck can be reused (both are indexed by local node id). Always install the freshly parsed DefSyntax (spans, local diagnostics). Alternatively, fold local diagnostics into the fingerprint and keep spans in a separate per-revision table. Key plan reuse on structural identity (the DefCheck Arcs plus structural fps), not on DefSyntax Arc identity.
- **issue**: The check orchestration is unsound under cancellation and under coalesced edits. It decides reuse with transient sets (`dirty`, `iface_changed`) and compares against `old.interface_fp` after overwriting `checks[d]`. Suppose a cancelled run stores a new interface for d and never reaches d's dependents. The next run sees d as clean with an unchanged interface and skips those dependents for good. | **evidence**: §2.4 pseudocode: 'store checks[d]' happens inside the loop, while iface_changed exists only within one check() call. The design explicitly allows cancellation ('Results from a cancelled revision stay in the memo') and a latest-wins mailbox that merges several edits into one check. | **fix**: Store with each group the exact input key it was checked under: member structural fps, dependency interface fps, resolved dependency DefIds, and Config (role/target). On each check, recompute the key from current dependency interfaces and recheck iff it differs (Salsa-style verification). This makes cancellation, undo and coalescing correct by construction and removes the dirty/iface_changed sets and most of check_memo.
- **issue**: Types decided at the use site are not modelled. The forward-only, dependencies-first SCC cutoff assumes a definition's interface is final once its own group is checked. That fails in three known places. (1) Today a SOURCE payload type is the union of fields accessed anywhere in the program. (2) A distributed function's argument types come from cross-role call sites (local_requirements). That is exactly why today's distributed compile needs a fixed point, so the claim 'Role/value references are ordinary edges, so no fixed point is needed' is unsupported. (3) PASSED context, if typed top-down, would fold `scene` and every PASSED reader into one SCC (267 functions on NovyWave). | **evidence**: boon_typecheck/src/lib.rs:39252-39330 (source_payload_shape_table scans every Path expression and every host-effect argument across the program). distributed_compiler.rs:1239, 1377-1390 (local_requirements merged from call sites), 1245-1268 (pass loop). examples/todo_mvc_physical/RUN.bn:15, :139, :498, :596-602: the SOURCE is declared in store and bound through PASSED into Scene/Element/button, and store-SCC member new_todo reads `.click` directly. §2.11 GroupCx/DefCheck contains no PASSED requirements, SOURCE bindings or call-site requirements. | **fix**: Make the checker contract explicit: interfaces are final after their own group (no backflow), and PASSED is a bottom-up requirement row. For facts that really are use-determined (SOURCE payloads, remote-function wire types), add per-definition 'requirements on target' summaries, aggregate them per target with their own fingerprint cutoff, and re-validate readers when the aggregate changes (reverse edges). Otherwise get an owner decision that these types come from the producer's body or element kind. Add distributed differential tests where client call sites drive server argument types.
- **issue**: The diagnostics lane contains only per-definition syntax and check results. Whole-program rules that decision 6 requires as positioned diagnostics are missing from check(): lowering Phase A (instantaneous cycles, OUT producer rules), recursion from the definition graph, SOURCE/host-port/render-slot ABI checks, and list order-chain validation. This repeats a known bug: today's Diagnostics intent skips order validation. It also makes the 2-2.5 ms literal/position targets inconsistent with the sibling panel. | **evidence**: d_lowering.md §5.2 and :141, :228: Phase A runs on every edit in the diagnostics lane, 1.5 ms (TodoMVC) to 4 ms (NovyWave), and applies static arm selection when a WHEN selector 'reduces to a constant after parameter substitution', so literal edits can change which state exists and which Phase A diagnostics appear. a_typecheck_orchestration.md:6 ('Diagnostics intent never runs list-order validation'). typecheck lib.rs:20539-20580 (host_ports table, a whole-program pass). | **fix**: Add a program-facts stage to check() (Phase A, graph recursion and cycles, ABI tables, order chains), built from per-definition summaries with cutoff. A position-only edit can skip it. A literal-only edit must rerun Phase A unless Phase A reports which literal sites it consumed. Restate the targets with this stage included (about 6-8 ms NovyWave compiler side), which is still well inside 16.7 ms.
- **issue**: The Lowerer contract contradicts the lowering panel. This design asks for per-definition lower_def fragments plus link, lowering-dependency tracking, and patch_literals over a separate constant-pool Arc. The sibling design fully inlines every call into hash-consed DAGs, where identical constants and subtrees are shared across sites and arms are pruned statically, and it relowers everything (≤8 ms TodoMVC / ≤15 ms NovyWave). On such a DAG, patching one source literal in place is infeasible. | **evidence**: This design §2.5 step 3, §2.11 (Lowerer trait), SE2. d_lowering.md summary and :141 ('Full re-lowering on every edit ... so no incremental lowering'), :104 (static arm selection), :11 (hash-consed constant pools). | **fix**: Drop Session.frags, lower_def, link, patch_literals, lowering-dependency bookkeeping and the constant-pool contract. build() runs Phase B over the whole snapshot. Memoize the last few plans by snapshot structural identity for undo and position-only edits. Set literal edit→plan to about 20 ms p95 on NovyWave instead of 5 ms.
- **issue**: The from-scratch consistency guard (§2.9: identical diagnostics warm vs cold after every keystroke) fails by construction. Warm results depend on history: definitions with syntax errors keep their 'last good interface' (§2.7), and fenced reparse reuses old definition boundaries (§1.4) while a cold parse uses indentation-guided recovery. Typing one keystroke at a time produces many error revisions. | **evidence**: §2.7 'dependents keep using its last good interface'. §1.4 fenced reparse, used only on the warm path. §2.9 'after every keystroke ... require identical diagnostics'. d_process.md :185 checks warm==cold only at each sequence's final revision. | **fix**: Make recovery a pure function of the text. Use the same indentation-guided implicit close warm and cold, and drop fenced reparse. Replace last-good interfaces with deterministic error-type poisoning, where dependents of a broken definition see an absorbing Error type and report nothing. If the history-dependent policy stays, restrict the guard to error-free revisions and say so.
- **issue**: Child-program compiles have a runtime contract the integration plan does not cover. artifact_id, plan_digest, source digest, compiler and target are delivered to Boon code as a SOURCE payload and stored by persons_pro. The source-bundle digest is also used to match prepared artifacts and package descriptors. The design deletes CanonicalSourceBundleV1/SourceBundleDigestV1 and switches to BLAKE3 without listing these consumers. The ≤1 ms child-compile target also leaves out artifact encoding and hashing, which every child compile must do. | **evidence**: boon_host_runtime/src/persistent.rs:3969-3990 (compiled_program_payload fields). examples/persons_pro/RUN.bn:221-226, 395-404, 437-438 and Model/Workspace.bn:43-55 read .artifact_id/.plan_digest/.source_digest. program_host.rs:643, 2052-2103 (PreparedProgramPayloadIdentity::SourceBundle matching). boon_app_package/src/bundle.rs:53, 497. boon_document_model/src/lib.rs:369-410. program_core.rs:694-731 (encode_program_artifact + plan_hash on every compile). examples/persons_pro.budget.toml also gates valid_edit_to_preview_visible_p95 = 16.7 ms on the same samples (verify.rs:4881-4907). | **fix**: Keep the source digest and plan digest in the artifact layer (boon_program_runtime), computed only there, and keep SHA-256 unless the owner approves changing the Boon-visible digest text. Add these consumers to the matrix. Define the child budget as compile + encode + hash + template, measured against both persons_pro gates.
- **issue**: Name resolution departs from today's rules without saying so. The proposed order leaves out lexical record-field scope (sibling fields are predeclared for their whole record) and call-context names. It also puts builtins after user modules, which implies user modules may shadow builtins, while today reserved standard roots are rejected. Without record scope, `sources.increment_button` inside store.count (counter.bn:26-31) resolves only through the global unique-suffix fallback and breaks as soon as a second `sources` field exists anywhere. | **evidence**: boon_typecheck/src/lib.rs:14364-14390 (resolve_checked_read_name: lexical record scope, then expression_call_contexts, then unique suffix). boon_parser/src/lib.rs:4000-4015 and 7855-7900 (reserved standard namespaces rejected for modules, fields and functions). boon_effect_schema/src/lib.rs:1684 (test uses boon_syntax::is_reserved_standard_root). §2.3 resolution order. | **fix**: Use this order: local scopes, then enclosing record siblings walking outward, then call contexts, then own module, then Module/fn, then builtins, then the unique-suffix fallback. Keep rejecting reserved roots. Export STANDARD_ROOTS/is_reserved_standard_root from boonc_syntax. Record in name_users that a sibling lookup depends on the parent's child list.
- **issue**: Plan identities must not come from session-local Symbol ids. DefKey and all fingerprints hash interner ids, which depend on the order in which symbols were interned (intermediate keystrokes included) and are renumbered by the '1 M junk symbols' rebuild. If site keys or node ids in the plan derive from DefKey, the warm plan differs from the cold plan (breaking the guard and determinism), and an interner rebuild changes runtime node identity, so focus, caret and scroll are lost. | **evidence**: §1.5 ('DefKey = hash128(... [path Symbols] ...)', 'Symbol ids are session-stable'). §1.2 interner rebuild. d_lowering.md :199 (LP2 wants stable 64-bit site keys; node identity = hash(site key, call-path key)). | **fix**: Specify that DefKey and the fps live in memory only, and that everything placed in the plan (site keys, debug labels, persistence inputs) hashes name bytes or structural routes. Drop the interner rebuild: 1 M symbols is about 16 MB, and hours of typing produce far fewer.
- **issue**: The latest-wins mailbox is incompatible with the optional Splice edits. The worker cancels and replaces the pending request, so splices queued behind an in-flight compile would be dropped and the session's text would be corrupted. | **evidence**: crates/boon_native_playground/src/compile.rs:142-165 (pending.take(), cancel, replace). §2.6 'mailbox.take_latest()'. Matrix #7 'optional Splice deltas'. | **fix**: Keep full-text Set per changed unit, which costs about 0.1 ms per bincode hop even for 433 KB, and drop Edit::Splice. Or make the mailbox append edits and supersede only the compile.
- **issue**: The migration predecessor cache key is incomplete, and the cutover plan contradicts owner decision 1. | **evidence**: compile.rs:757-807: stage N is compiled with the MigrationPredecessorBinding built from stage N-1's plan, but the design keys on (stage path, per-unit hash, schema_version). §6 and IN4 keep boon_compiler_legacy as a test oracle after cutover, while decisions.md:3-5 say the old compiler is deleted at cutover. | **fix**: Key each stage on (unit hashes, schema_version, key of the predecessor stage). Retire the differential and AST oracles before the cutover commit, archiving the divergence ledger.

## missing
- Record-level interface cascade. As written, `store`'s interface is the record of all its children (430 depth-1 fields in NovyWave), so any child's interface change rechecks every whole-`store` reader, such as the PASS site at novywave/RUN.bn:4949. The parent interface should refer to child DefIds structurally, so the cascade stops at readers of the specific field.
- Interface flapping under typing mode is not addressed. A partial identifier changes a function's interface to Error and back on each keystroke, rechecking the whole cone twice. Streaming the edited definition's diagnostics first (listed as P2) should be P1 for the typing-mode corpus.
- SE5 blocks need definition-relative positions plus a per-definition start-line table. Otherwise a position-only edit changes every block below it and the 4 ms end-to-end target needs a full resend.
- Snapshot matching by revision alone is not enough: the key must include application, example and migration stage, because revisions restart when the example or stage changes.
- DistributedSession needs a rule for files shared by several roles (whose hints and diagnostics the editor shows) and must keep the Client projection (today's compile_*_with_client_projection, distributed_compiler.rs:232-237). Returning Vec<(role, plan)> is enough for program_host, which rebuilds the DistributedGraphPlan (program_host.rs:80).
- PlanFailed { rev } carries no diagnostics. Any error found in build() is lost unless all rules run in check().
- Consumer matrix corrections: boon_app_cli builds packages (src/main.rs:1 build_app_package), so it is a compile consumer, not load-only. The matrix leaves out boon_document_model::EmbeddedProgramDescriptor digests (lib.rs:369-410), the boon_effect_schema dev-dependency on boon_syntax (lib.rs:1684), catalog.rs:99 boon_runtime::source_units_for_entry, and boon_phase0_baseline/src/evidence.rs:428 hard-coding ('boon_typecheck', 131).
- SE1 acceptance 'body edits recheck exactly 1 definition' ignores SCC groups. Editing any member of NovyWave's 22-node SCC or TodoMVC's 5-node store SCC rechecks the whole group.
- The check memo key leaves out Config (role and target change builtin availability) and resolution targets. set_config should invalidate explicitly.
- The OUT inline-hint channel (LanguageProjectSnapshot.inline_out_hints) is not mapped in the Checked API.
- The TEXT interpolation grammar `{ $?name(.name)* }` leaves out the `{PASSED.x...}` form used in examples (e.g. `TEXT { {PASSED.store.active_count} }`). The spec should list it, since PASSED is a keyword token.
- Naming does not match the process panel (`compile-bench --warm` vs `boon_cli bench-warm`, PM8). Pick one.

## unrealistic
- Literal edit→plan in 5 ms through patch_literals. It does not fit the sibling lowering (hash-consed shared constants, static arm selection) and is not needed, because full relowering takes ≤15 ms.
- 2 ms (position-only) and 2.5 ms (literal-only) compiler edit→diagnostics on NovyWave. Literal edits must rerun Phase A (≤4 ms) and the whole-program ABI facts, so expect about 6-8 ms.
- Warm==cold identical diagnostics after every keystroke while keeping last-good interfaces and fenced reparse.
- Distributed compile with 'no fixed point needed' while cross-role call sites still decide argument types (distributed_compiler.rs:1377-1390).
- Child starter compile ≤1 ms p95 'in-process' if it is meant to satisfy the 4 ms end-to-end gate. The artifact encode, content-id hash and plan digest (program_core.rs:704-710) are not in the model.
- End-to-end NovyWave targets of 4-5 ms before SE5 deltas land. SE5 is scheduled after SE4, and the design itself estimates a full snapshot at about 3 MB.
- The prototype evidence for fingerprint precision is 64-bit, single shape plus literal hashes over one flat unit arena (fe_proto.rs:490-517), with no header fp, no per-definition arenas and no 128-bit hashing. The 1.24 ms figure is plausible but not yet what the design specifies. Zero diagnostics on examples does not show that the trees are correct until FE4 exists.

## simplifications
- Delete incremental lowering entirely (frags, lower_def, link, patch_literals, lowering deps, constant-pool Arc). build() is one call over the snapshot with a small plan memo.
- Replace dirty/iface_changed/check_memo with one stored input key per SCC group. That covers cancellation, undo and coalesced edits with a single mechanism.
- Drop Edit::Splice and fenced reparse. Whole-unit Set plus whole-unit reparse (1.24 ms worst case) plus deterministic indentation-guided recovery is simpler and matches the cold result.
- Use two fingerprints instead of three. Structural (with type-relevant literals and local diagnostics) and literal bytes are enough. The NameIndex diff comes from DefKey sets, so header_fp adds nothing to correctness.
- Hash the parsed node arena (kinds, symbols, literal classes) instead of tokens with FIRST_ON_LINE. Reformatting then really is position-only, and the hash covers about half as many items (29.7k nodes vs 56.9k tokens on NovyWave).
- Drop the display-order part of interface_fp. Render dependents' hints lazily from the dependency's current DefCheck instead of rechecking them after a field reorder.
- Drop the interner rebuild path.
- Merge DistributedSession into Session with one Config per role. Move last_good_plan policy into boon_compile_service instead of Session.
- Keep the artifact layer's existing SHA-256 digests (computed only on encode and load) instead of introducing BLAKE3. It is one fewer format change and one fewer dependency, and the cost stays off the compile path.

## extra_owner_questions
- Today a user module, field or function named after a reserved standard root (List, Text, File, ...) is an error (boon_parser lib.rs:4000-4015, 7855-7900). Keep that, or allow user modules to shadow builtins, as the proposed resolution order implies?
- Child-program compile results expose artifact_id, plan_digest, source digest, compiler and target to Boon code (persistent.rs:3969-3990; persons_pro reads them). Are these field names and SHA-256 values part of the language/runtime contract, or may they change, for example to BLAKE3 or with fields removed?
- SOURCE payload typing: today it is the union of fields accessed anywhere in the program (typecheck lib.rs:39252-39330). Under strict decision 5, should the element kind at the binding site decide it (interprocedural), or should payloads be declared or derived locally? The choice decides whether the session needs reverse edges.
- Distributed function argument types are inferred today from cross-role call sites (a fixed point). Should they instead be fixed by the producer's own body, with call sites only checked against them?
- Hints for generalized function parameters and PASSED reads: show the principal type (type variables or requirement rows), or concrete types taken from call sites as some of today's hints do?
- When a file is shared by several distributed roles, whose diagnostics and hints should the editor show: all roles merged, the active role, or the Client?
