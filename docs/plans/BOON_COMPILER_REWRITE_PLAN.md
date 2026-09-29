# Boon Compiler Rewrite Plan

Written: 2026-09-29. Status: **proposed**. Once the owner approves it, this is
the single compiler execution plan, and every other `BOON_COMPILER_*` plan
becomes history (§9.2). Nothing described here has been implemented.

The owner asked for the fastest possible compiler, designed as if no compiler
code existed, with every architecture decision, process rule and AGENTS.md
constraint open to criticism. This plan is the result of:

- a 16-agent audit of every compiler subsystem, plus external research;
- 10 adversarial claim checks;
- a 12-agent design round, each design adversarially reviewed;
- three independent critiques of an earlier draft of this plan;
- seven rounds of owner decisions.

Numbers come from the current release binary (`target/release/boon_cli`,
i7-9700K without SHA-NI, fresh process) unless marked as an estimate or as a
prototype measurement.

Detailed design-panel notes are in
[`compiler_rewrite_notes/`](compiler_rewrite_notes/). They are input, not
authority: where they disagree with this plan, this plan wins.

---

## 0. Summary

| | today | target (estimates; validated by the P0 spikes) |
| --- | ---: | ---: |
| counter (140 lines), diagnostics / verified | 7 / 16 ms | ≤1 / ≤2 ms in-process |
| TodoMVC (3,576 lines), diagnostics / verified | 390 / 790 ms | ≤10 / ≤20 ms |
| NovyWave (11,926 lines), diagnostics / verified | 526 / 1,870 ms | ≤30 / ≤55 ms |
| warm edit → diagnostics (TodoMVC) | 372-397 ms | ≤16.7 ms gate, ≤8 ms goal |
| warm edit → preview (TodoMVC) | 1,151-1,207 ms | ≤40 ms |
| parse NovyWave | 64 ms | 2.6 ms (**prototype-measured**) |
| peak RSS, NovyWave verified | 314 MiB | ≤64 MiB |
| compiler source (pipeline crates) | ~400 k lines | ~45 k lines |

**Why the gap is so large.**
- The current compiler solves types per call path, not per function.
  TodoMVC's 350 checked calls become 10,537 compiled call sites, and the
  solver can retract its own conclusions.
- It proves its own output. SHA-256 plus CBOR is 25-39% of verified time; all
  plainly incidental work together is 34-46%.
- It converts the program through about 62 representations.
- It has no warm path at all.

These are architecture decisions, and none of them is required by the language.
Removing them is the only way to reach the targets; tuning is not (§3).

**What replaces it.** A new pipeline in new crates, built beside the old one:
1. a single-pass flat parser;
2. a strict type checker that checks each function once into a scheme and
   instantiates it at each call site;
3. a lowering straight to the MachinePlan, extended by a small listed format
   delta, including elements as real data (D13).

There are no in-process proofs. The old compiler survives only as a weak test
oracle and is deleted at cutover.

---

## 1. Owner decisions (2026-09-29)

These are settled. Any semantic change beyond them goes to the owner.

| # | Topic | Decision |
| --- | --- | --- |
| D1 | Strategy | Greenfield only. New crates, new pipeline. No effort on quick wins in the old pipeline. The old compiler is a test-only differential oracle until cutover, then deleted. |
| D2 | Freedom | Change anything except Boon syntax and semantics; semantic changes need the owner. Byte-identical oracles, budgets, gates, AGENTS.md rules, plan docs, crate layout and the MachinePlan format are all open. Nobody depends on the compiler in production. |
| D3 | Self-proofs | Removed: receipts, digests, manifests, seals, construction images, in-process re-verification. Plans are verified only when loaded from outside the process (artifacts, packages, received plans), and in debug builds and checkpoints. |
| D4 | Narrowing | Dropped. One type per function. A WHEN's type is the union of all its arms, and requirements come from all its arms. Types never depend on values. |
| D5 | Unions | Sound and fully typed: `WHEN { Light => Oklch[...], Dark => TEXT {...} }` has type `Oklch[...] \| TEXT`. No lenient field-union widening, no "open empty object" fallback. Reading a field that is not guaranteed present is an error. |
| D6 | Strictness | Strict. The documented rules are enforced as positioned diagnostics. Examples that break get fixed. |
| D7 | Field order | Not part of type identity. Display uses written order, and merged types use first-appearance order, everywhere. |
| D8 | Themes | Split the mixed-kind Theme dispatchers per kind (a consequence of D4). Refactor the TodoMVC and NovyWave themes toward far fewer shapes ("better Boon code"). |
| D9 | Cycles | A dependency cycle is legal only through a **HOLD update**, an **event-driven collection update** (append, remove, replace_all, …) or a **stateful builtin** (Bool/toggle and the like; "some can almost be written with HOLD"). Nothing else closes a cycle: not LATEST, not derived fields, not THEN bodies. Explicit cycle-handling builtins (`Dependency/catch_cycle`) are removed ("duct tape"). LIST, SET and MAP values, and records or tagged objects containing them, may not be HOLD state. The owner would welcome a HOLD alternative (P0 produces an options note). |
| D10 | WHEN payloads | Binder patterns only, Rust/Roc style: `HierarchyPage[rows] => rows`. The WHEN subject is not refined inside arms. |
| D11 | Precedence | Pony rule: mixing two different binary operators without parentheses is an error, with a fix-it. A chain of one operator (`a + b + c`) is left-associative. Comparisons do not chain (D18). Unary minus binds tightest. `\|>` is structural. |
| D12 | Persistence | A clean schema that stores only source-of-truth state: what a restart needs to restore the previous state, never anything that can be recomputed. That is HOLD states, LATEST current values, stateful-builtin state, authoritative collection rows, and (because every value keeps its last value, D30) the last value of anything that gets its first value later when code reads it outside its own update; the compiler infers that set. Query results are re-run on restore, commands never (D31). Existing local data may reset once. |
| D13 | Elements | Everything is structural data, **now**. Element values are real data at runtime: code may read, spread and build them. Builtin and render contracts are closed: "you can pass only what the functions called expects/can handle". |
| D14 | Own name | A field's own name is not in scope inside its own initializer, so `[events: events]` copies the outer `events`. The undocumented unique-suffix bare-name lookup is removed. A value refers to itself only through HOLD's named binder. |
| D15 | LATEST | *Revised 2026-09-29 with D30:* **self-reference only via HOLD.** LATEST is the most recently updated of its arms; it may not name itself. A constant or other starting arm gives LATEST its starting value, so `LATEST { Text/empty(), input.text, done \|> THEN { Text/empty() } }` keeps the last text without HOLD. The owner: "HOLD should be an escape hatch for cycles, not something needed to use". (Originally: "state only via HOLD; LATEST merges events only".) |
| D16 | Cells | Cells becomes a spreadsheet written in plain Boon that behaves like Excel: dependent recalculation, and circular references shown as an error value. No language support for cycles. |
| D17 | Effect lists | New event-driven builtin that replaces a collection's contents, e.g. `rows: LIST {} \|> List/replace_all(with: page_event.rows)`. An effect-returned list lives in its own collection authority. |
| D18 | Syntax cleanups | 1. One newline/indent rule (§4.2), which must not break multi-line TEXT. 2. `--` inside TEXT is text. 3. Comparisons do not chain. 4. A tag is either bare (`Panel`) or a **tagged object** (`Panel[x: 1]`) within one type, never both. |
| D19 | FjordPulse | The deployed app (apps/fjordpulse, deploy/fjordpulse) may be reset at cutover; no data migration. |
| D20 | Element values | Elements are **tagged objects**: `Element/button(...)` returns `Button[label: TEXT, style: [...], ...]`. Each kind keeps its exact payload inside unions and matches with binder patterns (`Button[label] => label`). |
| D21 | Snapshot reads | Fix the runtime so reads within a tick see committed values, as RUNTIME_MODEL says. Today a HOLD can see another cell's new same-tick value, so results depend on declaration order. The owner's explanation: earlier runtimes were asynchronous and had no ticks, so the rewrite may have implemented ticks incorrectly. Unconditional. |
| D22 | Unrendered style data | Keys the themes set that no renderer draws today (`glow` and `shadows` inside `material:`, `family` in spread font records, …) get **renderer support**. They are not deleted. The contracts include them, and the renderer implements them (work item R-RENDER). |
| D23 | Exact record parameters | A record passed to a user FUNCTION must contain exactly the fields the function consumes. "Consumes" means reads, or forwards into another exact position: a callee parameter, state, a collection, a contract. Extra fields are an error. The owner does not want functions to receive data by accident, especially when a function changes later. Context flows through PASSED. |
| D24 | Unused names | Unused FUNCTION parameters and unused pattern binders are errors, with a fix-it. |
| D25 | Dead arms | Every WHEN arm's state exists, even when a compile-time constant rules the arm out. State never depends on values. The compiler may warn about unreachable arms. |
| D26 | Stateful builtins | Rewritten as ordinary Boon standard-library functions over HOLD (e.g. `Bool/toggle`). A builtin stays only where Boon cannot express it, and is then marked stateful in the catalog. HOLD and collection updates are then the only state primitives, which is what the cycle (D9) and persistence (D12) rules are built on. |
| D27 | Nested FUNCTION | A FUNCTION declared inside a BLOCK is a positioned error; FUNCTIONs live at module level. |
| D28 | Immutability | "Everything is basically immutable from the user point of view." A collection chosen through WHEN (`zs: WHEN { A => xs, B => ys }`) is a read-only view whose element type is the union. Writes must name the real authority. |
| D29 | FLUSH boundaries | Rust `?` style: FLUSH skips the rest of the enclosing expression and lands at the nearest boundary. Boundaries are record fields (including `store` fields), FUNCTION results, BLOCK results and the root. BLOCK locals are **not** boundaries: they keep their normal type. The docs' "named binding initializer" wording is corrected in P0. |
| D30 | Change model | **"Everything is change": the original Boon model, made strict.** There is no event type, no event keyword and no "moment". Every expression is a value that updates; it has a value from the start or gets its first value later (SOURCE payloads, effect results, THEN outputs), and it keeps its last value. **WHEN and THEN copy:** the body runs once each time the input updates and copies the current values of everything else it reads (the owner: WHEN "freezes/copies dependencies, a sip of the current values"). **WHILE is live:** while an arm is selected, it follows every update of what it reads. So TodoMVC's `key_down.key \|> WHEN { Enter => new_todo_text … }` copies the text at the press and later keystrokes do not re-fire it. The compiler still infers, per expression, "never updates / has a value from the start / gets its first value later" and reports mistakes in those words (e.g. "`TEXT {x} \|> THEN {…}` never runs: `TEXT {x}` never updates"). Rules: §4.3 "Change rules". |
| D31 | Effects | **Query/command split by catalog class, no syntax.** A *query* is safe to re-run (File/read_*, Directory/entries, Wellen/*, Secret/verify, Timer/deadline, …): in a live context (a plain field or a WHILE arm) it is a live resource that starts when its scope activates, restarts when its arguments change, cancels the superseded run and stops when its scope ends; in a copy context it runs once per update. A *command* acts on the outside world or answers differently each run (File/write_*, DevelopmentPasskey/*, Http/request, Clock/wall, Random/bytes, …): allowed only in copy contexts (THEN bodies and WHEN arms, D30), runs exactly once each time the input updates with its arguments copied at that moment, and never runs on start, restore or hot reload. Effects become visible through hover, an inferred effect row on every FUNCTION scheme, diagnostics, and an effect log in the dev window and scenario runner. |
| D32 | What counts as an update | **Every write fires**, equal or not: one rule everywhere, as in the original Boon ("more consistent, less surprising"). Presses, keys and effect results fire on every occurrence; a HOLD fires on every accepted write; a derived value fires (at most once per step) whenever one of its inputs fires. So `count > 5 \|> THEN` runs on every count update, and an effect-result HOLD fires on every completion without a special case. Start, restore, hot reload, entering a WHILE arm and creating a row are not updates. A "only real changes" filter, if ever wanted, is a library function over HOLD, not engine support. |
| D33 | HOLD input | **A later update of the piped value resets the HOLD** (the original rule). HOLD takes every value that arrives, from its piped input or its body; its own writes do not re-trigger it. |
| D34 | Asynchrony | **Hidden, as the original Boon intended. No `Pending` in the language.** A query result is a value that gets its first value later, like a key press; while a restarted query runs, the value keeps its last answer. An app that wants to show "loading" writes it as business logic, e.g. `LATEST { Loading, request \|> THEN { Loading }, Wellen/hierarchy_page(…) }`. Wellen's hand-written `request_fingerprint` is unnecessary once restarts are driven by argument updates. |

---

## 2. Where the time goes today

| fixture | intent | total | parse | typecheck | semantic | IR | backend | plan validation |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| counter | diagnostics | 7 | 3.5 | 3.4 | | | | |
| counter | verified | 16 | 3.7 | 6.6 | 4.8 | 0.1 | 0.4 | 0.4 |
| TodoMVC | diagnostics | 390 | 18 | 370 | | | | |
| TodoMVC | verified | 790 | 18 | 435 | 158 | 3 | 85 | 90 |
| NovyWave | diagnostics | 526 | 64 | 457 | | | | |
| NovyWave | verified | 1,870 | 64 | 713 | 780 | 25 | 214 | 66 |

- **Warm path.** Warm is not faster than cold: TodoMVC edit-to-diagnostics is
  372-397 ms. The playground shows diagnostics only after the whole verified
  preview, about 1.15 s.
- **Throughput.** End to end, diagnostics run at 9-23 k lines/s and verified
  compiles at 4.5-6.4 k lines/s. Parsing alone runs at ~190 k lines/s; oxc
  reaches 6.5 M.
- **Memory.** Peak RSS is 154 MiB on TodoMVC (budget 128) and 314 MiB on
  NovyWave. A NovyWave verified compile makes 7.46 M allocations totalling
  1.33 GB.
- **The old compiler is a weak oracle.** `boon_cli check` fails on 8 of the 22
  manifest entry sources, and only 5 of the 21 manifest scenarios pass under
  `boon_cli run --scenario`.

---

## 3. Diagnosis: architecture decisions that made the compiler slow

**3.1 Types are solved per call path, by an engine that retracts.**
- The kernel compiles every definition into one global constraint graph before
  solving anything.
- Each call path gets a copy of the callee body, keyed by the caller's type
  variables, so the copies almost never share. On TodoMVC that is 5,789 fresh
  frames (plus 302 reused), 82,451 operations and 169,680 activations for
  4,081 expressions.
- Function types are "summary bytecode", re-interpreted on every activation:
  416 k node evaluations on TodoMVC and 1.34 M on NovyWave.
- The solver is incremental Datalog with retraction. It withdraws requirement
  facts, refolds requirements before every dequeue, and walks whole equivalence
  classes on every bind. It never generalizes a type.
- The root cause is semantic. Results were narrowed per call by singleton
  tags, joins were non-monotone, and field order was part of type identity.
  D4, D5 and D7 remove all three.
- Deleting every plainly incidental step would still leave TodoMVC diagnostics
  at about 340 ms, against a 75 ms budget.

**3.2 The compiler proves its own output.**
- SHA-256 plus canonical CBOR costs 25% of TodoMVC verified time, 31% of
  NovyWave verified and 39% of counter verified. NovyWave hashes 157 MB for
  435 KB of source.
- What gets hashed or built for proof:
  - per-owner basis fingerprints, and two SHA-256 "proofs" per type term;
  - currentness receipts, checked-image seals and pairing receipts;
  - 31 k semantic execution-receipt rows;
  - a construction image;
  - a dependency manifest, and a request graph that only tests read;
  - seven parallel semantic graphs, each with its own digest and validator.
- The IR is verified twice.
- `verify_plan` runs at every seal and again at runtime open, and 80-92% of it
  is a SHA-256 over the whole plan.
- No product consumer reads any of it (claim check C1). The parser's
  per-expression route digests are working identity keys, but the new design
  replaces them with cheap structural keys.

**3.3 Too many representations.**
- About 62 program representations and at least 27 conversions.
- Five type encodings, four string interners, three call-expansion passes
  (kernel frames, semantic OutNet, document inlining) and at least six ID
  spaces.
- Strings interned by the kernel are turned back into owned `String`s and
  `BTreeMap<String, Type>` trees.

**3.4 Every phase has its own blowup.**

| Phase | Blowup |
| --- | --- |
| Parser | Line-oriented, 15+ passes, a heap string per token, a quadratic span search, SHA identities. Counter's 3.5 ms parse is 76% `examples/manifest.toml` validation. |
| Typecheck orchestration | 30-70% of the phase is outside the solver: DTO projection, repacking, row materialization, seals, and the builtin ABI rebuilt on every compile. |
| Semantic | Half of NovyWave's 780 ms is receipts and self-validation. |
| Backend | One field predicate is 57% of NovyWave's backend. Per-call-site document inlining turns 4,536 IR expressions into 64,482 document nodes. SHA-256 is used as the interning hash. |
| Engineering | No FxHash, ~1,000 BTreeMaps in semantic alone, no `#[inline]`, and the release profile is frozen. |

**3.5 There is no warm path.** Only per-unit parse and link are retained.
Diagnostics wait for the full preview. Cancellation is polled only between
phases, so under continuous typing no diagnostics appear. The warm benchmark
measures a request sequence the product never makes, and its status is
hard-coded to fail.

**3.6 The process locked the architecture in place.**
- **Byte oracles.** `plan_sha256` and the diagnostics fingerprint had to stay
  byte-identical, and editing the budgets file is forbidden. Two of the three
  pinned hashes are already stale.
- **Misattributed bans.** Bans based on misattributed measurements ("no
  second solver", "never result-type equality", "do not reopen summary
  memoization") fenced off the structural fix.
- **Optional profiling.** The profiler was optional, and the summarizer
  dropped zero-size assembly symbols, which hid all SHA-256 time.
- **Heavy protocol.** A 25-30 minute 3+30 A/B/A protocol ran in two lanes that
  differ by about 1%.
- **Mandatory proof spine.** Gate 0 enforced it by source text, together with
  a 15.8 k-line classifier registry.
- **Document sprawl.** Twelve plan documents (13 k lines), four of them
  claiming authority. Docs grew four times faster than code.
- **Frozen protocol settings.** Single-thread, no-cache and generic-CPU
  settings were frozen.
- **Semantics decided by accident.** The checker enforces far less than the
  spec, and joins and narrowing were whatever the implementation happened to do.

---

## 4. Target architecture

### 4.1 Pipeline

```
Sources (host-provided texts; by-path tools use project::discover)
  │  boonc_syntax: byte-class lexer → Pratt parser → per-definition flat arenas,
  │                session interner, definition table, 2 fingerprints per definition
  ▼
Resolve + definition graph (root-field-path granularity; recursion, name rules)
  │  boonc_check: per-FUNCTION biunification over kind-partitioned polar types
  │               → hash-consed scheme; root values solved as one monomorphic group
  ▼
Typed program (dense side tables: expression types, call instantiations, resources)
  │  boonc_lower Phase A (diagnostics lane): instance tree, OUT nets,
  │               instance cycles, storage and persistence paths
  │  boonc_lower Phase B (preview lane): hash-consed dataflow DAG → MachinePlan
  ▼
MachinePlan v11 + listed delta (§4.5) → runtime through a trusted in-process constructor
```

- **Diagnostics lane:** front end + check + Phase A.
- **Preview lane:** the diagnostics lane plus Phase B.

**Crates.** All new, built beside the old ones until cutover.

| crate | contents | size estimate |
| --- | --- | ---: |
| `boonc_syntax` | lexer, parser, interner, definitions, fingerprints, `discover`, `format`, `highlight`; the language-feature registry and surface probe (moved from boon_syntax/boon_parser) | ~6 k |
| `boonc_types` | TypeStore, ground algebra, printer; builtin, render, host catalog as Rust tables (host effects generated from `boon_effect_schema`) | ~3.5 k + ~2.5 k data |
| `boonc_check` | definition graph, constraint generation, solver, schemes, root group, diagnostics | ~11 k |
| `boonc_lower` | Phase A, dataflow and element emission, persistence, list access, distributed link, assembly | ~12-15 k |
| `boonc_session` | session, revisions, lanes, cancellation; the one compile service used by both the playground and the benchmark | ~3 k |
| `boon_compiler` (facade, rewritten) | small public API (§4.8) | ~1 k |

`boon_plan` is kept: the format plus a frozen `identity_v1` encoder.
`verify_plan` remains for untrusted loads and checkpoints.

### 4.2 Front end (`boonc_syntax`)

A prototype (standalone, not repo code) parses NovyWave in 2.6 ms (today
65 ms), TodoMVC in 0.64 ms (today 18 ms), counter in 0.021 ms, and all 141
example files in 6.9 ms.

**Lexer.**
- One pass over a 256-entry byte-class table, producing struct-of-arrays
  tokens (u8 kind, u32 start).
- Identifiers are interned in the lexer. Builtins are pre-interned at fixed
  ids.
- `TEXT {` switches to raw mode, with `{interpolation}` including
  `{PASSED.x}`. In raw mode, newlines and `--` are text (D18). Multi-line TEXT
  gets explicit fixtures.
- Removed spellings (`#`, `EXAMPLE`, `LINK`, …) become inline error tokens.
- The path-keyed example-style lint moves to xtask.

**Layout (D18).**
- **L1.** A newline ends an element unless the next line starts with
  `) ] } , |> =>` or a binary operator other than `-`. Tokens that need a
  follow-up continue onto the next line.
- **L2.** A line that starts with `-` begins a new element.
- **L3.** A binary operator's right operand is on the same line.
- **L4.** Indentation is checked but never decides structure:
  - (a) a continuation line is not indented less than its head;
  - (b) a value on the line after `name:` or `=>` is indented deeper;
  - (c) sibling elements are aligned;
  - (d) top-level items start at column 0;
  - (e) tabs are rejected.

The prototype accepts every one of the 141 current example files under these
rules. Several silent reinterpretations become errors:
- `value: 1⏎ 2` (today 2);
- `x |> f() + 1` (today `+ 1` is dropped);
- continuation lines indented less than their head.

**Expressions.** D11 precedence; comparisons do not chain; a pipe form is the
only thing allowed on the right of `|>`. Of the 19 example sites with
multi-operator expressions, the mixed ones get parentheses. Only
`exact_number_value_algebra.bn` changes meaning, and it evidently intends
conventional arithmetic.

**Arena.**
- Nodes are final at birth: `Node { kind: u8, flags: u8, a: u32, b: u32 }`,
  12 bytes.
- There is one arena per definition, with definition-relative spans, so an
  edit never renumbers other definitions.
- Tokens are not retained.

**Recovery.** Every unit reports all of its errors. Recovery is a pure
function of the text, so warm and cold results match. Today the first parse
error aborts the whole project.

**Definitions and fingerprints.**
- A definition is a top-level field, a FUNCTION, or a field of a record literal
  that is itself a definition's value. That gives 155 definitions on TodoMVC
  and 1,389 on NovyWave.
- `DefKey` is an in-memory key. Anything that ends up in a plan is hashed from
  name bytes and structural routes, never from session SymbolIds.
- Each definition gets two fingerprints: a **structural** one (node kinds,
  name hashes, type-relevant literals, local diagnostics) and one over its
  **literal bytes**.
- The parser always installs freshly parsed syntax. Fingerprints only decide
  whether check results may be reused.

**Discovery.**
- `project::discover(entry)` collects the entry file, its lowercase sibling
  units, and the transitive closure of `Module/...` references over
  uppercase-stem files under the project root.
- This reproduces every manifest entry's file list. `BUILD.bn` is its own
  program. App packages keep passing their explicit source lists.
- The compiler API takes texts and never touches the filesystem.
- `examples/manifest.toml` stays as the xtask/playground fixture registry.
  Dropping it from the compile path saves 2.7 ms and about 1,300 failing
  readlink calls per compile.

**Tooling.** `highlight` replaces `boon_parser::lex_source` in the editor.
`format` ports `format_source_unit`.

### 4.3 Typing rules ("Boon static semantics v2"; normative after P0)

**Types.**
- Every expression has an **update kind** (never updates, has a value from the
  start, gets its first value later; tracked at every nested field, D30) and
  **data**.
- The data is a **kind-partitioned union**: at most one member per kind. The
  kinds are TEXT, NUMBER, BYTES, BITS, tags, record, LIST, SET, MAP and SOURCE
  port.
- **Record member.** A single record whose fields are Required or Optional:
  `[a: 1] ⊔ [b: 2]` is `[a?, b?]`. Boon cannot tell record shapes apart at
  runtime, so a union of shapes would add no usable precision.
- **Tag member.** One tag set. Each variant is either bare or a tagged object
  with a payload record (D18).
- **Join.** The join is associative, commutative and idempotent, so the
  solution is unique and nothing is ever retracted.

**Homogeneity (D6).** These must share one kind set:
- LATEST arms;
- a HOLD's initial value and its updates;
- the elements of LIST, SET and MAP literals.

All tags count as one kind, and records join with Optional presence. So
`LATEST { e1 |> THEN {1}, e2 |> THEN {TEXT {a}} }` and `LIST {1, TEXT {a}}`
are errors. WHEN results and FUNCTION results may be multi-kind unions (D5).

**Functions.**
- FUNCTION is the only polymorphic entity. Each is checked once into a scheme:
  parameter predicates, an implicit **PASSED** requirement row, OUT scope
  effects and collection-write effects. The scheme is instantiated at every
  call site.
- There is no recursion, so the call graph is a DAG.
- Root values are monomorphic outer variables, solved as one group.

**WHEN and WHILE.**
- The result is the union of all arms, and requirements come from all arms.
- Patterns are `__`, a binder, a literal, `Tag`, or `Tag[binders]`. A bare
  `Tag` matches the variant, bare or tagged, without binding anything.
- Exhaustiveness is checked when the selector type is concrete. For a
  polymorphic selector, the arms become the scheme predicate "tags within {…}".
- The subject is never refined inside an arm (D10).

**Change rules (D30-D34).** They replace the continuous/event flow table of
TYPE_INFERENCE_AND_TYPECHECKING_PLAN.md.
- **Updates.** Every write fires, equal or not (D32). A derived value fires at
  most once per step when any input fires. Start, restore, hot reload, entering
  a WHILE arm and creating a row are not updates. Every value keeps its last
  value.
- **Copy contexts: THEN bodies and WHEN arms.** The body runs once each time the
  input updates, reading the committed snapshot (D21) plus the input's new
  value; nothing else it reads re-runs it. THEN or WHEN over something that
  never updates is an error ("never runs").
- **Live contexts: plain expressions and WHILE arms.** They follow every update
  of what they read while selected.
- **Placement.** HOLD, SOURCE declarations, stateful builtins and live queries
  belong to live contexts; inside a copy context they are an error ("this HOLD
  would be copied once and never update; use WHILE"). Commands belong to copy
  contexts only (D31). This also answers L6.
- **LATEST** is the most recently updated arm. At most one arm has a value from
  the start (the starting value). Two arms that can update from the same
  trigger in the same step are an error. A one-input LATEST is an error: remove
  the wrapper. LATEST may not name itself (D15).
- **HOLD** takes every value that arrives, from its piped input (a reset, D33)
  or its body; its own writes do not re-trigger it. Self-reference only through
  its binder (D14).
- **SKIP** means no update: the value keeps its last value and nothing
  downstream fires.
- **No value yet.** Anything computed from a value that has no value yet has no
  value yet. How the document shows that is L3c.
- **Fire edges never close a cycle.** A cycle must pass through a HOLD's
  committed-value read, a collection update or a stateful builtin (D9); a THEN
  input, WHEN selector or update candidate on the cycle is an error, not a
  runtime loop.
- **Stale-copy hint.** Hover always says what a WHEN copies ("copies `todos`
  when `selected_filter` updates"). P0 decides whether a WHEN over a value with
  a start value that copies an independently updating value into the document
  also gets a warning with the fix-it "use WHILE".

Update kinds are computed in one forward pass in dependency order, seeded at
constants, HOLD, stateful builtins, SOURCE and effect results.

**Cycles (D9).**
- There is one labelled dependency graph, at root-field-path granularity,
  including local nodes and state cells.
- Only three kinds of edge may close a cycle: HOLD update edges, edges into
  event-driven collection updates, and stateful-builtin state edges.
- Any other cycle is a positioned diagnostic that lists the path.
- Cross-function and instance cycles are detected on instances in lowering
  Phase A, which runs in the diagnostics lane.

**Names (D14).**
- Lookup goes through lexical scopes, innermost first: record-literal
  siblings, BLOCK locals, WHEN binders, OUT/call-context binders, FUNCTION
  parameters. Then the module (own functions and fields), then `Module/fn`,
  then builtins.
- Reserved standard roots cannot be shadowed.
- A field's own name is hidden inside its own initializer, so `[x: x]` copies
  the outer `x`. With no outer `x`, it is an "unknown name" error.
- There is no suffix fallback. Roots are reached as `store.…` or through
  PASSED.

**Contracts (D13).**
- Builtin, render, style, event and host-port contracts are closed. An unknown
  key is an error, and so is a value the contract does not accept.
- The catalog is the documented API, checked in both directions against the
  renderer and host key tables.
- Element values are data. Code may read them, spread them, pattern-match
  them and build them from literals. Slots check element values against the
  per-kind contracts. Elements are tagged objects (D20).
- User FUNCTIONs: record parameters are **exact** (D23). A scheme's record
  parameter lists exactly the fields the body consumes: reads, and forwarding
  into another exact position. Passing extra fields is an error. Whole records
  forwarded to a callee, into state or into a collection take that
  destination's exact type. P0 specifies how exactness composes through
  spreads and forwarding.

**SOURCE payloads** come from the provider: the element event group or the host
port. This replaces today's heuristic based on how names are spelled.

**Diagnostics.** The checker and Phase A together own about 60 diagnostic
families, all emitted in the diagnostics lane:
- **Phase A** owns instance-level cycles, OUT producer checks and pulse
  membership.
- **The checker** owns kinds, fields, exhaustiveness, change rules, homogeneity,
  definition-level cycles and recursion, PASSED presence, D14 names, contracts,
  DRAIN/DRAINING, the document root, role adjacency, unused parameters (the
  OUT plan's rule) and static constant checks.
- Code after Phase A keeps only debug assertions.

The catalog with codes, each tagged with its owner, is a P0 deliverable. The
starting point is `compiler_rewrite_notes/spec.md` §15.

**Display (D7).** Field order lives in a presentation side table, never in a
TypeId. Optional fields display as `f?: T` and unions as `A | B`. One printer
serves hints, the inspector and diagnostics.

### 4.4 Checker algorithm (`boonc_check`)

**Approach.** Flow-directed biunification (the Simple-sub family) over the
kind-partitioned polar types above.
- Producers add lower bounds and consumers add upper bounds.
- Bounds are lattice elements merged per head.
- Types depend only on producers, so a use never changes a producer's type.
- HM with bolted-on joins was rejected. D5 unions and record presence are
  subtyping, so HM would need two mechanisms, and its errors would depend on
  unification order.

**Per function.**
1. A private arena per FUNCTION.
2. One post-order generation pass. Ground expressions are evaluated directly
   through memoized TypeId operations.
3. Linear compaction into a canonical, hash-consed scheme.
4. Instantiation exactly once per syntactic call site. It copies only nodes
   that contain variables.

**Root group.** One monomorphic arena. When a function reads a root value, that
read becomes an implicit outer parameter, so each function solves
independently.

**Representation.**
- A per-compile TypeStore over an immutable static catalog base, so TypeIds are
  deterministic. If P6 needs per-definition reuse, schemes are stored in a
  store-independent canonical form and re-interned on reuse. This is decided in
  P2, before scheme work starts.
- Ports are polarized (`PORT(ρ⁺, ρ⁻)`). The solver has no union-find.

**Diagnostics.**
- Conflicts are order-independent (consumer, producer) pairs.
- Provenance stays off the hot path. On failure, only the failing definition
  is re-solved, in an explain mode.
- Text is rendered lazily and ordered by source position and name bytes.

**Output.** Dense tables (`CheckedDef { scheme, expr_type, call_inst,
inst_args, resources, out_scopes, conflicts }`) that lowering monomorphizes
directly.

**Distributed programs.** All roles are checked in one session. A cross-role
call instantiates the scheme of the role that owns the function. Cross-role
reads join the combined root group. A `WireEligible` predicate plus role
adjacency complete the rules. This removes today's legacy fixed point: at
least 9 (usually 12 or more) whole-program checks.

**Oracles.**
- Inline equivalence: with no recursion, a reference checker that inlines
  every call must give the same accept/reject result and the same substituted
  types for called functions.
- Confluence: shuffling the order must not change the result.
- Lattice-law property tests.

**Model.** The pre-D13 estimate was 1.2 ms for TodoMVC and 4.5 ms for
NovyWave. With structural elements (D13), every view function's result type
carries its element subtree. Spike S2 re-derives the model. Targets: ≤6 ms
TodoMVC, ≤20 ms NovyWave.

### 4.5 Lowering (`boonc_lower`) and the runtime delta

**Phase A** (diagnostics lane; target ≤1.5 ms TodoMVC, ≤4 ms NovyWave):
- definition flags and roots;
- an **instance tree only for stateful definitions**, one instance per static
  call path, with list-row regions as static owners;
- OUT nets by union-find, with producer checks;
- one iterative Tarjan pass over instance cells, giving cycle diagnostics and
  pulse membership;
- publication paths;
- target-profile limits keyed on `Config.target`. These are the checks that
  are user-reachable only inside `verify_plan` today: typed-index capacity and
  declared capacity.

**No static arm pruning (D25).** Every WHEN arm's state exists, whatever the
values.

**Phase B** (preview lane; target ≤8 ms TodoMVC, ≤20 ms NovyWave):
- A partial evaluator emits one hash-consed dataflow DAG (FxHash on compact
  keys, plus constant pools).
- Elements are ordinary record values in that DAG (D13): there are no document
  templates, materializations or value classes.
- Nodes that carry identity are never merged: element construction sites,
  effects, HOLD, SOURCE and streams. One `owns_identity` predicate derives the
  set.
- Op ids follow source pre-order. The executor sorts update ops itself. Effect
  emission order is source order.
- DependencyEdge ops are dropped, once a checkpoint has proven them redundant.

**Hidden v11 contracts to reproduce.**
- debug_map labels are semantic paths in today's `field:N`/`state:N`/`list:N`
  format. The executor's root lookups, the dev inspector and scenario matching
  all depend on them.
- Source-route `path` strings.
- `capability_summary`, derived at assembly. program_host reads it.
- `host_ports` and output roots.
- Persistence plan structures.

**Persistence (D12).**
- Durable leaves are:
  - HOLD states;
  - stateful-builtin state;
  - authoritative collection rows, whose fields are those not derived from
    other durable leaves.
- Each identity is the named data path plus the **structural route** of the
  state site inside its definition. It never uses a declaration ordinal, so it
  is stable under unrelated edits.
- State reachable only from view code, such as element-local hover, is
  transient (L13).
- `identity_v1` covers MemoryId, leaf ids, type fingerprints (including
  Optional presence), `semantic_schema_hash`, DRAIN recipe and edge ids, and
  EffectId. It keeps SHA-256 over small canonical inputs through `boon_plan`
  constructors, pinned by golden vectors.
- Per-compile ids (output roots, distributed and wire ids) may switch to a
  128-bit non-cryptographic hash.
- Nothing hashes a whole program or a whole plan during a compile.
- `PERSISTENCE_FORMAT_VERSION` is bumped. The executor accepts versions 5 and 6
  until cutover.
- Migration predecessor stages compile in a `persistence_only` mode. The cache
  key is (stage texts, schema version, predecessor key).

**Trust (D3).**
- **In process:** nothing is sealed, `verify_plan`-ed or plan-hashed.
  `MachineTemplate` gets a trusted constructor, and program_host drops its
  in-process `verify_plan`.
- **Loaded from outside:** `load_untrusted(bytes)` means decode +
  `verify_plan`, used for artifacts, app bundles and received plans.
- **Artifact digests.** `plan_digest` and the source-bundle digest are computed
  once, when the artifact is encoded, in the artifact layer. They stay SHA-256
  because persons_pro reads them from Boon code.

**Format: MachinePlan v11 plus a listed delta.** D2 allows it. Each item is
sized from spike S1:

| # | Delta | Kind |
| --- | --- | --- |
| R1 | Trusted template constructor and `load_untrusted` | API |
| R2 | `optional: bool` on `DataTypeFieldPlan` (serde default false). Executor and persistence validators updated; presence included in identity_v1 | Format |
| R3 | Non-durable semantic ids for lists and row fields that are not persisted. Touches `semantic_list_identities`, bounded paging and cursor.rs, which today require persistence leaves | Runtime |
| R4 | **Document model v2 (D13).** Elements are data values with a hidden stable identity (construction site key + instance path + row key). The renderer reconciles record trees by identity into frame nodes. This replaces DocumentPlan templates and constructors | Runtime + format |
| R5 | A `List/replace_all` collection op (D17) | Runtime + catalog |
| R6 | Removal of `Dependency/catch_cycle` (D9) | Catalog |
| R7 | Snapshot-read semantics (D21), landed early on the shared executor (P0/P1); old-engine gates re-run afterwards | Runtime behaviour |

Until cutover the runtime accepts both the old engine's plans (document v11) and
the new ones. The old-vs-new differential is exact for dataflow parts that use
none of R2-R7. Documents are compared by rendered frames, structurally, modulo
ids.

**Lowering is total.** Each of the ~680 PlanError sites in today's back half
becomes a checker or Phase A user rule, or an internal compiler error.

### 4.6 Document runtime v2 (runtime track R-DOC, D13)

Elements become real data, so the runtime, not the compiler, reconciles the
UI.

**Design points, decided in spike S3.**
- **Representation.** Element values are tagged objects (D20) with structural
  sharing (Arc).
- **Identity.** A hidden identity comes from the construction site key + the
  instance path + the list-row key. Values copied or moved keep their identity.
- **Recomputation.** The dataflow recomputes element subtrees incrementally,
  and the reconciler visits only subtrees whose values changed, so nothing is
  diffed per frame.
- **Hot reload.** Retained-node identity survives hot reload, so focus, caret
  and scroll survive edits outside the edited definition.
- **Hand-built elements.** They get the identity of their literal site.

**Replaces.** DocumentPlan templates, materializations, constructors,
value-class caching, and today's per-call-site inlining.

**Consumers to adapt.**
- `boon_document` runtime and renderer bridge;
- the native playground runtime view;
- the web host;
- the native GPU pipeline contract (NATIVE_GPU_PIPELINE.md).

**Performance gate.** Per-frame work and retained-update counts on TodoMVC
(many rows), NovyWave and Cells must be no worse than today at 60 FPS.

### 4.7 Session and warm path (`boonc_session`)

- **Cold first.** A full recheck per keystroke fits TodoMVC's budget.
  Per-definition reuse is added in P6 if the warm corpus requires it (likely for
  NovyWave). Until then NovyWave warm numbers are report-only.
- **Reuse, when added.** Check results are a pure function of (member
  structural fingerprints, dependency interface fingerprints, config). Each
  group stores its exact input key and is re-verified Salsa-style. Early cutoff
  compares freshly computed interface fingerprints (backdating).
- **Errors.** Dependents of a broken definition see a deterministic absorbing
  error type. There are no history-dependent "last good" interfaces, so warm
  and cold results match.
- **Two lanes on one compile thread:**
  1. `check`: front end + check + Phase A, which publishes the language
     snapshot;
  2. `build`: Phase B, which publishes the plan. The last good plan stays
     mounted.
- **Cancellation.** Polled per unit, per group, every 4,096 solver steps and per
  definition in lowering. Stop latency ≤1 ms.
- **Messages.** Full text per changed unit, about 0.1 ms per hop. The dev
  window keeps and shifts the previous snapshot. Snapshots are matched on
  (app, example, stage, revision).
- **Memory.** Retained session state ≤64 MiB on NovyWave (today 383-414 MB).

### 4.8 Public API (facade `boon_compiler`) and engine selection

```rust
Source { path: Arc<str>, text: Arc<str> }
Config { entry, target, role, app, schema_version, predecessors }
Session::new(config, sources) / edit(edits) / check(&cancel) -> Arc<Checked> / build(&cancel) -> Build / last_good_plan()
Checked::diagnostics() / hints(file) / occurrences() / has_errors()
check(&config, &sources) -> Arc<Checked>;  compile(&config, &sources) -> Result<Arc<MachinePlan>, Arc<Checked>>
DistributedSession   // roles; same edit/check/build; Client projection
mod syntax { highlight, format }   mod project { discover }   mod debug { dump_lowered, PhaseTimings, WorkCounters }
```

**Engine selection.**
- `BOON_COMPILER_ENGINE=old|next`, default `old`, lives only in the shared
  compile service (playground preview and dev lanes) and in
  `boon_cli compile-bench`. The native verifier passes it explicitly, and it is
  stamped into reports. `verify-all` refuses reports from a non-default
  engine.
- `COMPILER_ID` and the prepared-program cache key include the engine, so the
  two engines' stored child artifacts never mix.

**Porting other consumers.** Everything else (editor, program_runtime,
app_package, host_runtime, phase0, behavior harness, xtask, web host,
`boon_runtime`, which loses its compiler dependency) is ported in the P7
switch change set. There is no old-engine adapter behind the new facade.

The full caller map is in `compiler_rewrite_notes/frontend_session.md` §4. P0
copies it into the contract.

### 4.9 Engineering rules for the new crates

**Checked from the day each crate exists:**
- no `sha2`, `ciborium` or `serde_json` in the compile crates; identity hashing
  goes only through `boon_plan`'s frozen constructors;
- no example-specific branches;
- a line cap per crate;
- builds for `wasm32-unknown-unknown` (single-threaded, clock abstraction) and
  for musl.

**Code rules:**
- dense u32 ids in `Vec`s;
- FxHash or foldhash for sparse maps; `BTreeMap` only at output boundaries;
- no `String` keys, and no strings on the success path;
- lazy diagnostics;
- per-compile arenas, with size assertions on hot node types;
- `#[inline]` on hot cross-crate accessors;
- no reliance on `catch_unwind`.

---

## 5. Example migration

These counts are estimates from scanners. The authoritative census is the new
checker's own output, taken over **every tracked `.bn` file and every inline
test source**:
- the 22 manifest entry sources;
- the 43 examples outside the manifest (several are read by crate tests);
- migration stages and embedded sources;
- crate testdata;
- Boon inside about 12 Rust test files.

Each source is either fixed or deleted with a reason. The 31 orphan `.scn`
files are either triaged or deleted.

### 5.1 Breakage classes

| class | sites | fix |
| --- | ---: | --- |
| TodoMVC Theme `get(request)` mixes NUMBER, record, LIST and tags (D4, D8) | 94 call sites, 6 files | **Themes as data.** Each theme exports `tokens(mode)`, whose tokens have the same type in every theme. `Theme/tokens(name, mode)` picks one. RUN.bn reads `theme.material.panel`, `theme.font.body.color`. About 2,443 lines become ~1,110, and ~20 shapes become ~8 role types. Draft: `compiler_rewrite_notes/examples.md` §4. |
| NovyWave `NovyTheme` (D8) | 84 material + 60 font calls | Same idiom. Bordered and borderless materials are separate roles. Dead `trace_*` functions are deleted. |
| Self-referential LATEST (D15 revised) | counter_latest, interval_latest; census for others | Fix-it: `initial \|> HOLD s { LATEST { … } }`. A LATEST with a starting arm is legal now. LATEST blocks with two starting arms get manual review. |
| One-input LATEST | ~80 (mostly bytes_* fixtures) | Remove the wrapper (fix-it). |
| Cycles (D9) | TodoMVC new_todo (title ↔ edited_title) after D15; completed ↔ all_completed passes through Bool/toggle (legal) | Rewrite with HOLD where the census flags it. |
| Cells (D16) | the formula engine | **Redesign in plain Boon, Excel-like.** Computed values live in HOLD/collection state updated on edit events. Recalculation follows dependency order. A visited/depth guard marks circular references with an error value. Size L. |
| WHEN subject reads in arms (D10) | 68 (NovyWave 55) | Binder patterns. |
| WHEN that must follow live updates (D30) | census in P2b; 1,400 WHEN vs 40 WHILE today | Today's runtime runs WHEN over a value live, but the language copies. Every WHEN whose arm reads something that can update independently of its selector, and whose result must follow it (views, themes with nested WHENs on other parameters), becomes WHILE. The new checker lists them; WHENs that copy on purpose (TodoMVC `title_to_add`) stay. THEN over HOLD state (18 sites) stays legal (D30, D32). |
| Effect-returned lists (D9, D17) and effect placement (D31) | NovyWave hierarchy and signal pages, BUILD files | `List/replace_all` authorities. Commands move into THEN bodies or WHEN arms; queries in WHILE arms stay live. Hand-written `request_fingerprint` keys go (D34). |
| Suffix fallback removed (D14) | TodoMVC `visible_todos`/`selected_filter`, NovyWave RUN.bn:4506, … (census) | Qualify with `store.` or pass through PASSED. |
| Optional-field reads (D5) | TodoMVC append without `completed`; persons_pro variant-union reads (5) | Append complete rows; use binder patterns. |
| Collections in HOLD (D9) | census | Move them into collection authorities. |
| Closed contracts (D13) | census: `event` vs `events` (142/59), `hovered: <port>` (137), private keys such as Cells `__selected_*` | Fix against the generated contracts. Unrendered theme keys (glow, material shadows, font family) stay and get renderer support (D22). |
| Exact record parameters (D23) | census | Remove the extra fields at call sites, or move context into PASSED. |
| Pony precedence and comparison chains (D11, D18) | ≤19 | Parentheses (fix-it). |
| Unused parameters and binders (D24) | ~12 | Remove them (fix-it). |
| Lists of row updates `List/map(new: <row update>) \|> List/latest()` | 27 (Cells, NovyWave) | Ordinary lists of last values; `List/latest` gives the most recently updated row (L5). |
| Old-compiler bugs that look like example bugs | fjordpulse (the parser tokenizes TEXT contents), cells (builtins missing from the old ABI) | Fixed in the new compiler; examples unchanged. |

**Old-engine compatibility rule.** Until P7 the old engine stays the native
gates' default. So an example change lands on main only if the old engine still
passes that example's gate. A change the old engine cannot handle lands with
the P7 switch. The differential oracle O4 runs on the revision of each example
that works with both engines.

### 5.2 Verifying example behaviour

1. **Baselines.** There are two:
   - the *behaviour baseline*: a pinned worktree build at the last commit where
     the triaged scenarios pass, found by bisection in P0 and run with host
     services;
   - the *differential engine*: the in-tree old engine behind
     `BOON_COMPILER_ENGINE=old`.
2. **Scenario triage** (spike S6). Build a host-service scenario runner by
   reusing MigrationScenarioRunner/PersistentRuntime. Classify every failure as
   harness, scenario drift, runtime bug or compiler bug. Freeze the step ids and
   source paths the gates consume (persons-pro `valid-edit-preview` and
   `corrected-edit-preview`).
3. **CPU render diff.** Static theme-probe documents render every combination
   of theme, mode, role and state, with state passed explicitly; the harness
   can only click today. After R-DOC the diff compares rendered frame trees.
4. **Native readback A/B.** Uses the existing `--required-checkpoint` workflow,
   one run per theme (the verifier allows at most 32 checkpoints and 64 steps),
   with settled frames only.
5. **Handoff.** The native gates, then `verify-all`.

**Gate coupling.** counter-dev also compiles `counter_migration` (its switch
alternates between them). persons-pro also compiles its migration stages and
its embedded `starter_source`/`library_source`, and has a 4 ms p95
child-compile budget.

---

## 6. Performance targets

Every number is an estimate, re-derived after the P0 spikes. The gates in
`budgets/compiler.toml` move only by tightening.

| metric | target | basis |
| --- | --- | --- |
| front end, NovyWave / TodoMVC / counter | ≤4 / ≤1.2 / ≤0.05 ms | prototype: 2.6 / 0.64 / 0.021 ms |
| check, TodoMVC / NovyWave | ≤6 / ≤20 ms | S2 re-derives; pre-D13 model 1.2 / 4.5 ms |
| Phase A, TodoMVC / NovyWave | ≤1.5 / ≤4 ms | per-node model |
| Phase B, TodoMVC / NovyWave | ≤8 / ≤20 ms | per-node model; S1 measures v11+delta emission |
| cold diagnostics / verified, TodoMVC | ≤10 / ≤20 ms | sum |
| cold diagnostics / verified, NovyWave | ≤30 / ≤55 ms | sum |
| small program in-process (counter, starter source) | ≤1 / ≤2 ms p95 | persons-pro native gate: 4 ms end to end, encode included |
| warm edit → diagnostics, TodoMVC | p95 ≤16.7 ms gate, ≤8 ms goal; no-op edits ≤1 ms | full recheck (front + check + Phase A + snapshot/IPC ≈ 1.2 + 6 + 1.5 + ≤2) |
| warm edit → diagnostics, NovyWave | report-only until per-definition reuse lands | full recheck ≈ 30 ms |
| warm edit → preview, TodoMVC | p95 ≤40 ms (budget 100) | full re-lowering |
| supersession stop latency | p95 ≤1 ms, max ≤8 ms | polls |
| single-thread throughput, fixtures ≥10 k lines (NovyWave and stress corpus) | ≥250 k lines/s diagnostics, ≥150 k verified | today 23 k / 6.4 k |
| peak RSS (absolute), NovyWave verified | ≤64 MiB | today 314 MiB |
| compiler-internal allocations per compile, NovyWave | ≤10 k; plan construction reported | today 7.46 M in total |
| amplification counters | 1 instantiation per syntactic call; ≤4 solver work items per expression | today 30× calls, ~41 steps per expression |

A synthetic stress corpus (deep helper nesting, wide unions and records, large
element trees, many call sites) is added, so the examples getting simpler after
migration cannot make the targets look met. Scaling gates use CPU time or
instruction counts per doubling.

---

## 7. Correctness strategy

A byte hash is never an oracle.

| oracle | what it checks |
| --- | --- |
| O1 spec corpus | `tests/compiler/diagnostics/**/*.bn`, each with a `.expect` listing `(severity, code, line:col)` plus hint lines for display rules. Expectations are written from the spec, never taken from old-compiler output. Seeds: every audit probe, a negative and a positive fixture per rule, multi-line TEXT layout fixtures, `bytes_negative_templates`, `language_surface/current`, `testdata/phase0`. |
| O2 behaviour | Every manifest scenario and migration scenario (all three migration sequences) through the host-service runner after triage. Also a restart scenario per persisted example (D12), and a UI-state retention scenario (focus survives an unrelated edit). |
| O3 determinism | Each fixture is compiled twice, with threads 1 and N. Canonical plan bytes, diagnostics and the language snapshot must be identical. The harness compares. |
| O4 differential | Old engine vs new engine, only where the old engine passes its own scenario, on sources both engines accept. Every divergence is listed in `tests/compiler/divergences.toml` with its decision (D-number or L-number). |
| O5 warm == cold | At the end of every edit-corpus sequence: identical diagnostics, snapshot and canonical plan. |
| O6 debug verifier | Debug builds run `verify_plan`, behind a feature or env switch rather than on every keystroke. So does every checkpoint. |
| checker oracles | Inline equivalence, confluence, lattice laws (§4.4). |
| persistence | `identity_v1` golden vectors. Per example, the durable leaf set must match the D12 rule (§4.5). |

"Checkpoint" means `cargo xtask compiler-checkpoint` plus
`cargo test --workspace`, run at milestone close and after build-profile
changes. The repo has no CI.

---

## 8. Execution phases

**Sizing.** One strong engineer or agent per phase. ∥ marks work that runs in
parallel with the critical path.

**Critical path.**
```
P0 (incl. spikes) → P2a → P2b → P3b → P4 → P5 → P6 → P7 → P8
                                          ↘ R-DOC must finish before P5 exit
```
P1a overlaps P0.

### P0: Reset, decide, de-risk (2 weeks)

**Semantics (owner).**
- Answer the language questions L1-L15 (§11).
- Write "static semantics v2" into LANGUAGE_SEMANTICS.md, covering all of D4-D18:
  - types, flows, the union model, homogeneity, cycles and temporal rules;
  - names, contracts, precedence, layout;
  - the diagnostic catalog, with codes and owners.
- Produce the HOLD-alternative options note (D9).

**Reconcile the active contracts.** Name the new owner of each item:
- BOON_CONSOLE.md and BOON_CONSOLE_IMPLEMENTATION_PLAN.md:
  - target eligibility goes to `boonc_check`/Phase A, keyed on `Config.target`;
  - ConsolePort becomes a catalog host-port family;
  - Wasm and hardware backends consume the MachinePlan only;
  - lineage is the source digest plus the plan digest at artifact encode.
- BOON_FIRST_RISCV_PROCESSOR_PLAN.md: drop its "separately hashed stage
  artifacts" prerequisite.
- BOON_FORMAL_VERIFICATION_AND_WHERE_PLAN.md: WHERE becomes a future checker
  phase with plain diagnostics and no manifests, per D3.
- NATIVE_GPU_PIPELINE.md: ownership table, "verified MachineTemplate", "compile
  cache", document model v2.
- BOON_PERSISTENCE_ARCHITECTURE_PLAN.md: the D12 rules.
- TYPE_INFERENCE_AND_TYPECHECKING_PLAN.md, the OUT plan and
  BOON_TYPE_NOTATION_AND_INSPECTOR.md: the conflicting paragraphs are
  superseded.
- AGENTS.md lines 10-14 and 30-44 (§9.1).

**Process.**
- Write the `docs/architecture/BOON_COMPILER.md` contract (≤300 lines).
- Archive the old plans (§9.2).
- Bring xtask back under its line cap by deleting `compiler_allocator.rs` and
  the evidence lane. Gate 0 is probably red today (25,023 lines against a cap
  of 25,000).
- Run all 7 handoff gates once and record which are red.
- Measure line-cap headroom on the playground and runtime (31,082/32,000 and
  41,105/42,000). Plan compensating deletions for R1-R7.
- The pending kernel edits are committed (Q14), and `perf_event_paranoid=1` is
  persisted (Q13).

**Spikes** (time-boxed; each ends with a report):

| spike | scope | exit criteria |
| --- | --- | --- |
| **S1: vertical slice** | counter, counter_migration and one list-plus-persistence fixture go through a minimal new front end, a ground-only checker and Phase A/B emitting v11+delta, then run on the runtime. Also a shadow counter-dev native run with engine=next. | `verify_plan` passes; the scenarios pass; restart restores state under D12; an unrelated edit keeps focus; every needed runtime/format change is listed and sized. |
| **S2: checker core with structural elements** | TodoMVC view functions (theme refactored) plus a NovyWave-shaped generator (373 constructors, 252 style records, depth 22, theme and PASSED reads). | Largest scheme and instantiation counts within the model; extrapolated check ≤6 ms TodoMVC, ≤20 ms NovyWave. If not, the element representation is fixed before P2. |
| **S3: document model v2** | Prototype element-record reconciliation on TodoMVC (many rows), Cells and a NovyWave list. | Per-frame work and retained-update counts no worse than today; identity rules settled. |
| **S4: census scanner** | Closed contracts against the renderer and host tables, stateful LATEST, collections in state, effects in continuous arms, THEN over state, suffix-resolved names, Optional fields reaching storage, cycles under D9. | Counts per class, which size P3. |
| **S5: snapshot semantics** | Instrument the executor to count same-tick reads of another cell's new value across all runnable scenarios, then implement the D21 fix. | Fix landed on the shared executor; affected examples and scenarios listed and adjusted; old-engine gates re-run. |
| **S6: scenario triage and baseline pin** | Host-service runner, bisection, triage table. | Each runtime bug sized. If they add up to more than ~1 week, a runtime-fix track is added that must finish before P4 exits. |

**Exit.** The owner signs off the spec and the spike reports, and the plan is
re-baselined from them.

### ∥ P1a: Parser core (2 weeks, overlaps P0)

Lexer, layout (D18), Pony precedence, parser, recovery, interner, definitions,
resolver (D14). Builds for wasm32 and musl from day one.

### ∥ P1b: Front-end completion and harness v4 (2 weeks, overlaps P2a)

- Fingerprints, `discover`, `highlight`, `format`. The language-feature
  registry and surface probe move into `boonc_syntax`.
- Tree equivalence with the old parser on the accepted corpus, excluding the
  listed intended divergences.
- **Harness v4:**
  - `boon_cli compile-bench` (no binary self-hash, no pretty-JSON export,
    in-process repeat mode);
  - `cargo xtask compiler-ab` (ABBA over two binaries, instruction counts as
    the fast signal, a measurement lock that refuses to run while cargo is
    active);
  - a samply summarizer (nearest-symbol attribution, phase cross-check);
  - budgets v4 (§9.3).

**Exit:** parse ≥1 M lines/s; reparsing the largest unit ≤1.5 ms.

### P2a: Checker core (3-4 weeks)

- `boonc_types`: store, algebra, printer.
- `boonc_check`: generation, solver, schemes, root group, diagnostics, display
  order.
- Decide the TypeStore lifetime and reuse form (§4.4).

**Exit:** O1 green on the implemented rules; checker oracles green; counter and
the P3a-migrated fixtures check within target.

### P2b: Catalog and census (2 weeks)

- The full catalog: every builtin, render/scene contract and host port, with
  host effects generated from `boon_effect_schema`. A completeness test runs in
  both directions against the renderer and host tables.
- Distributed roles.
- Publish the real census of every source.

**Exit:** O1 fully green; census published.

### ∥ P3a: Spec-driven example migration (3-4 weeks, from week 1 of P2a)

**Tooling:** render diff, theme probe documents, readback A/B.

**Scanner- and fix-it-driven fixes:**
- Theme refactors (D8);
- binder patterns;
- THEN event fields;
- LATEST → HOLD;
- `List/replace_all` rewrites;
- suffix qualification;
- parentheses;
- unused parameters;
- migration stages kept consistent, so persisted paths match.

All of this follows the old-engine compatibility rule (§5.1).

### P3b: Census-driven migration (2-3 weeks, critical path)

- Closed-contract fixes.
- The Cells redesign (D16).
- Every remaining census item.
- Embedded sources and inline test sources.

**Exit:** every tracked Boon source checks with 0 diagnostics on the new
checker, or has been deleted with a stated reason. The render diff shows no
unintended changes.

### ∥ R-DOC: Document runtime v2 (4-8 weeks, from S3; must finish before P5 exit)

The element-record runtime and reconciler, with renderer, playground-view and
web-host adaptation, per §4.6.

**Exit:** its own performance gate (§4.6); both document representations run
until P8.

### ∥ R-RENDER: Renderer support for theme styling (D22; 1-2 weeks, any time before P7)

Implement `glow`, shadows inside `material:`, and font `family` in the native
renderer and the web host. Add them to the render contracts. Verify with the
native readback A/B. Unlike the rest of the migration, the pixels change here
on purpose; the owner reviews screenshots.

### P4: Lowering (5-7 weeks; can be split across 2-3 agents)

- `boonc_lower`: Phase A, dataflow and element emission, persistence (D12 plus
  identity_v1 golden vectors), list access, distributed link,
  `persistence_only` stages, assembly with the hidden v11 contracts (§4.5).
- Runtime delta R1-R3 and R5-R6, plus R7 if scheduled.
- Classify all PlanError sites.

**Exit:**
- `verify_plan` passes on every example plan at checkpoint.
- O2 passes where the scenarios were triaged.
- The back half is within target.
- DependencyEdge ops are shown redundant before being dropped.

### P5: Session, compile service, integration (3-4 weeks)

- `boonc_session` and the facade.
- The shared compile service, used by the playground and
  `compile-bench --warm`: two lanes, cancellation, snapshot deltas.
- The engine selector and COMPILER_ID per §4.8.
- The playground adopts the bench's allocator (mimalloc, L-process).
- The server, http, host, wellen, web and plan_executor crate tests run with
  `next`.

**Exit:** native preflight runs with `next` pass the product gates (R-DOC
done); O3 and O5 green.

### P6: Warm and performance hardening (2 weeks, critical path)

- The warm edit corpus (12 classes × TodoMVC/NovyWave, settled and typing
  modes).
- The stress corpus.
- Per-definition reuse, if needed (size L on its own if NovyWave requires it).
- Build-profile A/B: LTO, codegen-units=1, target-cpu, then PGO. Not
  `panic=abort`, because the host runtime relies on `catch_unwind`. Adopting any
  of these also requires re-running the native gates.
- `boon-app build apps/fjordpulse/app.toml` plus the three Docker build targets.

**Exit:** budgets v4 checkpoint green on `next`.

### P7: Switch (days)

- Flip the default to `next`.
- Port the remaining consumers (§4.8) in the same change set.
- Fresh `verify-all`, all native gates and the checkpoint on the switch tree.
- Reset the FjordPulse deployment (D19): staging first, per its RUNBOOK.
- Tag `compiler-v1-final` on the last tree that still contains the old engine.

### P8: Cutover deletion (1 week, atomic, owner-approved)

- Delete the old crates, the document v11 runtime path, their features and
  tests, and the gates and docs that pin them (§10).
- Update BOON_CONSOLE's stage chain (source → syntax → typed program →
  MachinePlan), README links and the remaining docs.
- Run `cargo test --workspace`, a fresh `verify-all` and the checkpoint.
- Restart the release playground, as AGENTS.md requires.
- Ratchet the budgets.

### Effort

| | estimate |
| --- | --- |
| Critical path, sequential, one engineer per phase | ~20-27 weeks |
| Sum of all phases and tracks | ~32-45 engineer-weeks |
| Calendar time, with P2a, P4 and P5 each split across 2-3 parallel agents and every ∥ track staffed | roughly 12-18 weeks |

The estimates are re-baselined from the S1-S6 reports. The largest
uncertainties are R-DOC, the scenario triage and the closed-contract census.

---

## 9. Process changes

### 9.1 AGENTS.md compiler block

This block replaces lines 30-44. Lines 10-14, the console/CPU authority
paragraph, are updated to point to the reconciled contracts (P0).

```markdown
Treat `docs/architecture/BOON_COMPILER.md` as the compiler contract and
`docs/plans/BOON_COMPILER_REWRITE_PLAN.md` as the only compiler execution plan.
Older compiler plans are history, not authority.

- The compiler is being rewritten in new crates (`boonc_*`) behind the
  `boon_compiler` facade. Do not optimize or extend the old pipeline
  (boon_typecheck, boon_compiler_kernel, boon_checked, boon_semantic, boon_verify,
  boon_ir, boon_parser/boon_syntax, the kernel path of boon_compiler); change it
  only to keep it building, to feed the differential oracle, or to delete it.
- Boon syntax is frozen; the settled semantic decisions are in the contract.
  Any other change to what a program means goes to the owner first. When a
  strict rule rejects an example, fix the example; when an example exposes a
  compiler limitation, fix the compiler.
- Correctness is behavioural: the spec diagnostics corpus, every example and
  migration scenario, determinism, warm == cold, and (until cutover) the
  old-vs-new differential whose every divergence cites a decision. A stored byte
  hash of a plan or of diagnostics is never an oracle. Update expectations in
  the same change set as the intended change.
- No self-proofs: nothing re-verifies or digests what the same process just
  built, and a compile never hashes a whole program or plan. Persisted
  identities go only through boon_plan's frozen identity_v1 constructors. Plans
  are verified when loaded from outside the process, in debug builds and at
  checkpoints.
- Profile before optimizing: samply on the release binary (perf_event_paranoid
  must be <= 1; never change system settings yourself), summarized with the repo
  summarizer, ranked by self and inclusive time. Timers and counters corroborate.
- Develop with `cargo xtask compiler-ab`; run the checkpoint
  (`cargo xtask compiler-checkpoint` + `cargo test --workspace`) at milestone
  close and after build-profile changes. Warm numbers count only from the shared
  compile service driven over the edit corpus.
- budgets/compiler.toml holds the gates. Tightening is free; loosening a time,
  CPU, throughput or RSS limit, or removing a fixture, edit class or scenario,
  needs the owner. No code path may depend on an example's name.
- Threads are allowed in latency lanes if the CPU-time gate and the
  single-thread throughput floor pass. LTO/codegen-units/PGO/target-cpu are
  ordinary A/B candidates; re-run the native gates before adopting one.
- Early cutoff on a freshly recomputed interface fingerprint (backdating) is
  allowed; skipping recomputation based on a guessed result is not.
- Delete superseded code in the same change set; revert a rejected experiment
  before its goal ends.
- Lessons live in the contract, dated, with their measurement and the condition
  for reopening them. They guide design; none bans a technique permanently.
- Cite numbers with a command and revision; anything else is an estimate.
- The playground's default compiler engine changes only in a change set that
  passes a fresh `cargo xtask verify-all` with that engine as the default.
```

The rest of AGENTS.md stays: fix the engine rather than work around it in Boon,
no fabricated human observation, and the native launch rules.

### 9.2 Documents

- **Archive.** Delete from the tree, keeping a ≤60-line
  `docs/archive/COMPILER_HISTORY.md` index with last-commit hashes:
  - every `BOON_COMPILER_*` plan except this one (12 files, 13 k lines);
  - all 44 `docs/plans/evidence/compiler-*.json` files.

  First commit the untracked or modified ones once, if the owner wants them on
  record.
- **Re-point.** README.md:40-41, GOAL_PROMPT.md, steps.md, and every plan that
  names BOON_COMPILER_PERFORMANCE_PLAN as the owner of compiler latency.
- **`compiler_rewrite_notes/`.** Retire it after P5. Before that, move the
  diagnostic catalog into LANGUAGE_SEMANTICS.md, the caller map into the
  contract, and the Theme drafts into the examples.

### 9.3 budgets/compiler.toml v4

**Removed:** `machine_plan_sha256`; frozen `profile_options`, `target_cpu`,
`compiler_threads = 1` and `compiler_caches = "disabled"`; the 3+30
two-lane/two-mode protocol; scaling keyed on counter names.

**Kept or added:**
- time budgets (counter tightened);
- absolute peak RSS from a lean producer (baseline ~6 MiB);
- a CPU-time gate at 1.25× wall;
- throughput floors on fixtures of 10 k lines or more;
- a small-program lane;
- the warm edit corpus, with typing and settled modes;
- supersession stop limits, both p95 and max (the old 8 ms max is kept);
- a switch/bundle-lookup limit (the old 1 ms is kept);
- CPU- or instruction-based scaling;
- the stress corpus;
- the allocator recorded in every report.

**Protocol:** ABBA A/B in development that stops once the result is
significant or within ±1%; a 3+20 checkpoint at milestone close; per-milestone
gate profiles (oracle gates always apply to `next`; performance gates apply per
fixture once `next` accepts that fixture).

---

## 10. Deletion inventory (at P8)

| what | lines |
| --- | ---: |
| boon_typecheck | 109 k |
| boon_semantic | 97 k |
| boon_compiler_kernel | 81 k |
| boon_compiler: kernel_oracle.rs, machine_plan_backend.rs, document backends, distributed_compiler.rs, session.rs | ~56 k → ~1 k facade |
| boon_parser + boon_syntax (replaced by boonc_syntax; the editor, formatter and effect_schema are ported first) | 16 k |
| boon_checked, boon_ir, boon_verify, boon_compilation_db | 18 k |
| boon_contract (its digest helpers move to the artifact layer) | 1.6 k |
| document v11 runtime path in boon_document (after R-DOC) | several k |
| dependency_classifier_schema_v1.toml + dependency_classifier.rs + ~10 compiler-shaped architecture checks | 15.8 k + 3.2 k + ~1.8 k |
| xtask compiler verifiers + boon_cli compiler_sample.rs | ~9 k |
| phase0 compiler probes, packed-site inventory roots, kernel layering test, behaviour-harness flat oracle, old compiler tests | hundreds |
| 12 plan documents + 44 evidence files | 13 k + 648 KB |

The replacement is roughly 45 k lines of compiler code, plus the R-DOC runtime
work and a few thousand lines of tooling.

---

## 11. Open questions

**Language semantics.** These are not adopted until the owner answers them,
and P0 cannot exit without the answers.

| # | question | recommended |
| --- | --- | --- |
| L1 | Element representation. | **Answered 2026-09-29: tagged objects (D20).** |
| L2 | When do host effects run, and how is that visible to the user? | **Answered 2026-09-29: query/command split (D31); no `Pending` (D34).** Open detail: borderline catalog classes (Http/request, Log/*). |
| L3 | Events vs values. | **Answered 2026-09-29: model C (D30).** |
| L3a | What counts as an update for a HOLD or a derived value? | **Answered 2026-09-29: every write fires (D32).** |
| L3b | Does a payload stay readable after it happens? | **Answered 2026-09-29: yes; every value keeps its last value, and WHEN/THEN copy while WHILE is live (D30).** |
| L3c | A part of the document whose value has no value yet (e.g. a label showing the last key before any key was pressed). | Proposed: renders nothing until the first value arrives (the original behaviour, hides asynchrony, D34); hover says "no value until … first happens". |
| L4 | Snapshot reads. | **Answered 2026-09-29: fix the runtime (D21).** |
| L5 | Lists of row updates `List/map(new: <row update>) \|> List/latest()` (27 sites). | Proposed: ordinary lists of last values (every value keeps its last value, D30); `List/latest` gives the most recently updated row. No special restriction. |
| L6 | State and SOURCE created inside WHEN arms or THEN bodies. | Follows from D30: allowed in WHILE arms (live scopes), an error in WHEN arms and THEN bodies (they copy once). Owner to confirm. |
| L7 | FLUSH boundaries. | **Answered 2026-09-29: BLOCK locals are not boundaries (D29).** |
| L8 | Style keys no renderer draws. | **Answered 2026-09-29: add renderer support (D22).** |
| L9 | Nested FUNCTION in BLOCK. | **Answered 2026-09-29: error (D27).** |
| L10 | User FUNCTIONs and extra keys. | **Answered 2026-09-29: exact record parameters (D23).** |
| L11 | Collections joined through WHEN. | **Answered 2026-09-29: read-only views (D28).** |
| L12 | Unused parameters and binders. | **Answered 2026-09-29: error with fix-it (D24).** |
| L13 | State reachable only from view code (element-local HOLD such as hover). | Transient, not persisted. |
| L14 | Constant-selector arm pruning. | **Answered 2026-09-29: arms always exist (D25).** |
| L15 | Stateful builtins. | **Answered 2026-09-29: rewritten as a Boon library over HOLD where possible (D26).** |

**Process and product.** The default applies unless the owner objects.

| # | question | default |
| --- | --- | --- |
| Q13 | Persist `perf_event_paranoid=1` via sysctl.d so samply works. | **Done** 2026-09-29 (`/etc/sysctl.d/60-perf.conf`); samply recording verified. |
| Q14 | The pending uncommitted old-kernel speedups. | **Done:** committed 2026-09-29 (e36b2c24, 198 kernel tests pass). They become the behaviour baseline only if they pass its scenarios (§5.2). |
| Q15 | Engine switch timing. | As soon as the P7 gates pass on `next`, then P8. |
| Q16 | Allocator. | The playground adopts mimalloc, as the bench measures (glibc malloc is 17% slower on NovyWave). |
| Q17 | wasm32. | The new compiler builds for wasm32, single-threaded. In-browser compile latency is reported, not gated. |
| Q18 | Identity scope. | Every id is a pure function of (source texts, config, compiler version). Element and node identities stay stable across edits outside the edited definition. Wire ids and cursor fingerprints are not stable across compiler versions or redeploys. Only persisted ids (D12) are frozen across versions. |
| Q19 | Budgets. | Re-baseline on the migrated examples, plus the stress corpus. |
| Q20 | NovyWave warm edit → diagnostics. | Report-only at P7; gated once per-definition reuse lands. |

---

## 12. Risks and later work

### 12.1 Risks

| risk | mitigation |
| --- | --- |
| **R-DOC** (structural elements at runtime) regresses the 60 FPS native gates or runs late. | Spike S3 first. A dedicated performance gate. The runtime keeps both document paths until P8. It must finish before P5 exits, so it is visible early. |
| The v11+delta runtime changes turn out larger than listed. | Spike S1 lists and sizes every change before P2. D2 allows format changes. |
| Checker cost with structural element records. | Spike S2 before P2; a representation decision if the model fails. |
| The strict spec breaks more code than estimated (closed contracts, D15, D9). | S4 census plus the real P2b census before sizing P3b; fix-its for mechanical classes; the per-milestone gate profile. |
| The temporal rules disagree with the runtime (L2, L3). | Runtime snapshot fix (D21, S5) plus runtime scenario differentials per rule. |
| No mountable behaviour baseline. | S6: host-service runner, bisected baseline, triage table. |
| Persistence schema drift. | D12 rule, structural-route identities, identity_v1 golden vectors, restart scenarios (O2). |
| Gate churn turns `verify-all` red. | New crates stay additive. The old-engine compatibility rule for examples. Old gates change only in P0 (line cap), P5 (engine stamping) and the atomic P8. |
| A noisy measurement machine (idle load ≈1.4). | ABBA, a measurement lock, instruction counts as the fast signal. |
| Plan sprawl comes back. | One living plan with dated status; no new plan documents; evidence stays in target/ plus a one-line checkpoint summary per milestone. |

### 12.2 After cutover

- The remaining format cleanup: dead fields (view_bindings, initial_patch_batch,
  delta_plan, dirty/commit counters, Unknown variants), typed name tables
  instead of debug-map string parsing, one compact versioned artifact encoding.
- Optional per-level parallel checking, only if the checker exceeds ~5 ms.
- A persistent per-definition cache, only if cold open time matters.

### 12.3 Language exploration (owner-initiated)

- A HOLD alternative (the P0 options note).
- A single store root, which fits the D12 source-of-truth model.
- Optional FUNCTION annotations, which are not needed for speed in this design.
