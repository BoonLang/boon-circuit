# Incremental compilation at sub-frame latency: prior art for §4.7

Research note, 2026-09-29, for review/REVIEW.md; not authority.

Scope: salsa/rust-analyzer/ty, Zig, TypeScript (tsc, tsgo), Lean 4, Sorbet (added), Roc,
Dart/Flutter, wasm32. `plan:NNN` = BOON_COMPILER_REWRITE_PLAN.md line; grepped sources are in the scratchpad.

## Summary

- `plan:658-661` (exact input key, Salsa-style re-verification, backdating) is salsa's red-green algorithm and Zig's PO counting: well proven.
- No system found reaches sub-frame edits by a full recheck: ty (4.7 ms), Zig (62-65 ms) and Sorbet's fast path all reuse per definition.
- "Cold first" (`plan:655`) needs 3.6-6x Sorbet's per-core line rate; NovyWave (≈30 ms cold) needs reuse regardless.
- Warm≠cold failure classes: interned-id order in output (TS 6/7), cycles meeting backdating (salsa #1310/#1338, Zig 0.16),
  a fingerprint dearer than the check it saves (TS 7 #64464: first warm edit 60-75x a full check), stale side data (TS #64424).
- Cancellation: salsa unwinds; tsgo, Zig and Lean poll or propagate. wasm32 aborts on panic, so the plan's polling is right.
- tsgo shows the cost of polling: a checker cancelled mid-run is poisoned and discarded, so memo commits must be per completed group.
- Cascades: rust-analyzer `{unknown}`, Lean synthetic `sorry` and Zig `transitive_failed_analysis` all use an absorbing error.
- None of them keeps "last good" state. This supports `plan:662-664`; the suppression rules must be written down.
- Browser builds gave up threads (or needed SharedArrayBuffer isolation), stack depth, the real std library, or speed (Go wasm).
- Single-threaded wasm also changes how a superseding edit can reach a running compile: poll a deadline, not only a flag.

## 1. salsa and rust-analyzer (plus ty)

**What it does.** A query database. Every derived value is a memoized function of its tracked
inputs. An edit bumps a global revision, and later reads re-verify memos lazily.

**Mechanism.**
- Red-green verification: a memo records its dependencies and when they last changed. It is reused
  when no dependency changed. When it re-executes and yields an *equal* value, it is backdated, so
  dependents stay valid (early cutoff by output equality).
  [verified: https://salsa-rs.github.io/salsa/reference/algorithm.html, the sections on revisions,
  backdating and durability]
- Durability: each input carries a volatility level, and salsa remembers the last revision in which
  an input of each level changed. This lets it skip verifying whole subgraphs (the stdlib) after a
  user-file edit. rust-analyzer measured ~300 ms per edit of pointless stdlib re-verification
  before this; the post gives no "after" number.
  [verified: https://rust-analyzer.github.io/blog/2023/07/24/durable-incrementality.html]
- Cancellation by unwinding. A write bumps a counter. Threads computing on the old revision panic
  with a special payload and are caught at the `ide` boundary as `Result<T, Cancelled>`, so
  "rust-analyzer requires unwinding".
  [verified: https://rust-analyzer.github.io/book/contributing/architecture.html, "Cancellation"]
  salsa 0.28.5 `Cancelled` has the variants `Local`, `PendingWrite` and `PropagatedPanic`.
  [verified: https://docs.rs/salsa/latest/salsa/enum.Cancelled.html]
  salsa checks for cancellation automatically on each query invocation. Long computations must call
  `unwind_if_revision_cancelled` themselves. A write blocks until other threads drop their
  snapshots, and the docs warn of deadlock.
  [verified: https://docs.rs/salsa/latest/salsa/trait.Database.html]
- When a request is superseded mid-query, rust-analyzer either re-queues it (`Task::Retry`, for
  requests that allow retrying) or answers with LSP `ContentModified`. A `Cancelled` payload that
  escapes a handler is logged as a bug.
  [verified: raw crates/rust-analyzer/src/handlers/dispatch.rs, last changed in commit 0625d0f
  (2026-06-19), functions `on_with_thread_intent` and `thread_result_to_response`]
- Diagnostics are computed only for open workspace files, never for library files. Each pass
  carries a `DiagnosticsGeneration`. Syntax diagnostics are sent before semantic ones, on a
  latency-sensitive thread. The main loop times cancellation separately and warns when a loop turn
  exceeds 100 ms.
  [verified: raw crates/rust-analyzer/src/main_loop.rs, `update_diagnostics` and the
  "overly long loop turn" warning]
- Granularity: syntax trees are built per file, while semantic analysis is per crate instance.
  A key invariant: typing inside a function's body never invalidates global derived data.
  The server is described as stateless ("a-la HTTP"); no last-good mechanism is described.
  [verified: architecture.md, same URL as above]
- Laziness matters more than incrementality: "it's not the incrementality that makes an IDE fast.
  Rather, it's laziness."
  [verified: https://rust-analyzer.github.io/blog/2020/07/20/three-architectures-for-responsive-ide.html]

**Error recovery.**
- Parser and analyses return `(T, Vec<Error>)` rather than `Result<T, Error>`. [verified: architecture.md]
- Types that failed to infer are `{unknown}`. Since PR #16583 (merged 2024-02-16), a type-mismatch
  diagnostic is dropped whenever `{unknown}` appears on one side of a zipped comparison.
  [verified: https://github.com/rust-lang/rust-analyzer/pull/16583]
- PR #20022 (merged 2025-06-17) keeps type-mismatch "experimental" because it still has false
  positives. The bar cited is that it never produce a false positive on real code.
  [verified: https://github.com/rust-lang/rust-analyzer/pull/20022]

**Determinism and failure modes.**
- In 2019, two runs of `analysis-stats` on identical code gave different counts of unknown-typed
  expressions (9,217 vs 9,218 out of 64,916, ~17.8 s per run). The issue does not identify the
  cause. [verified: https://github.com/rust-lang/rust-analyzer/issues/1552]
- ruff's red-knot (now ty): an uncaught salsa cancellation tore down its rayon pool. The fix wraps
  public entry points in `Cancelled::catch`. [verified: https://github.com/astral-sh/ruff/pull/12183]
- Salsa #1310 (opened 2026-09-04): a "backdate violation" panic when a fixpoint cycle from an
  earlier revision is broken. [verified: https://github.com/salsa-rs/salsa/issues/1310]
- Salsa #1338 (opened 2026-09-25): validation walks the *previous* execution's edges, finds a cycle
  that no longer exists, and returns provisional results. The incremental result differs from a
  fresh run. [verified: https://github.com/salsa-rs/salsa/issues/1338]

**Latency data.**
- ty (salsa-based, 2025-12-16): after an edit to a load-bearing PyTorch file, diagnostics are
  recomputed in 4.7 ms, against Pyright's 386 ms. A cold, uncached check of home-assistant takes
  2.19 s. [verified: https://astral.sh/blog/ty]
- I found no published per-keystroke latency for rust-analyzer. [not verified: none located]

## 2. Zig incremental compilation (0.14 to 0.16)

**Mechanism.**
- The unit of reuse is the `AnalUnit` (a declaration value/type, or a function body). Dependency
  kinds: `src_hash` (128-bit hash of a declaration's ZIR source range), `namespace`,
  `namespace_name`, `nav_val`, `type` (a container's structure) and `func_ies`.
- Old declarations map to new ones through a mapping built from the old and new ZIR instruction
  indices.
- Units are then split into up-to-date, outdated and "PO" (potentially outdated). A PO unit whose
  PO-dependency count drops to 0 is up to date: this is early cutoff when a re-analysed dependee
  "decided" the same value.
- An outdated type is recreated at a *new* InternPool index. The author accepts the lost
  incrementality because the compiler "is generally fast enough".
  [verified: https://gist.github.com/mlugg/73b3e60c803006f3556d87c9ed3e8a0e]
  (mlugg.co.uk returned 403 and was not read.)
- Failure containment: `failed_analysis` holds each unit's own error, and `transitive_failed_analysis`
  marks units that failed only because a dependency failed. Cancellation is an ordinary error value
  (`error.Canceled` in `CompileError`, "the compilation update is no longer desired"), propagated
  explicitly rather than unwound. The InternPool is meant to be garbage-collected "periodically",
  but that logic is not implemented.
  [verified: raw src/Zcu.zig from the GitHub mirror, last commit ece62a0 (2025-11-24); upstream
  development may have moved elsewhere since]

**Status and numbers.**
- 0.14.0: `-fincremental --watch` "works well" only with `-fno-emit-bin`. A half-million-line
  codebase takes 14 s cold (≈36 k lines/s) and 63 ms to re-analyse after an edit.
  [verified: https://ziglang.org/download/0.14.0/release-notes.html]
- 0.15.1: still experimental, with "incorrect compile errors" possible.
  [verified: https://ziglang.org/download/0.15.1/release-notes.html (downloaded, section "Incremental Compilation")]
- 0.16.0:
  - Over-analysis is mostly gone because "Reworked Type Resolution" made the internal dependency
    graph acyclic (except real loops).
  - It also fixed "dependency loop" errors that appeared in incremental builds but not clean ones,
    or the reverse. The notes call this the biggest inconsistency between the two.
  - Incremental is still off by default, with known bugs including miscompilations.
  - Building the Zig compiler with the new ELF linker takes 14 s cold, then 65 ms and 64 ms for
    one-line edits.
  [verified: https://ziglang.org/download/0.16.0/release-notes.html (downloaded, sections
  "Reworked Type Resolution", "Incremental Compilation", "New ELF Linker")]
- Testing: PR #20688 (merged 2024-07-20) added a test format of `#update=` steps with expected
  output. It immediately found a bug. [verified: https://github.com/ziglang/zig/pull/20688]
- Roc's ~450 k lines of Zig rebuild in ~35 ms with `zig build --watch -fincremental`. This is the
  time to build Roc's *compiler*, not a Roc program.
  [verified: https://rtfeldman.com/rust-to-zig, and the lobste.rs thread that links to it]

## 3. TypeScript: tsc builder, tsserver, tsgo (TypeScript 7)

**tsc incremental and watch.**
- Each module has a "shape" (its declaration-emit signature). Importers are invalidated only when
  the shape changes. This is early cutoff on an interface fingerprint.
- PR #42960 found that computing shapes costs about as much as a full emit. It made shapes lazy, and
  on ~3,000 files the first `--incremental` build fell from 67.5 s to 24.2 s (plain tsc: 23.3 s).
  [verified: https://github.com/microsoft/TypeScript/pull/42960]
- Issue #64464 (2026-09-26): the first comment-only edit after a clean build takes 31.7-41.8 s,
  while a full check takes 0.56-3.2 s. The cause is declaration emit for importers' shapes, with a
  quadratic scan in `getAlternativeContainingModules`. In TS 7 the ratio grew to 60-75x because
  that path stayed single-threaded.
  [verified: https://github.com/microsoft/TypeScript/issues/64464]
- Issue #64424: `tsc -b --watch` without buildinfo drops changes to non-root files and leaves the
  output stale. [verified: https://github.com/microsoft/TypeScript/issues/64424]
- tsserver cancels through named pipes and can split `geterr` into delayed steps.
  [verified: https://github.com/microsoft/TypeScript/wiki/Standalone-Server-(tsserver)]

**tsgo.**
- 2025-03-11 figures: VS Code (1.505 M LOC) 77.8 s → 7.5 s (≈19 k → ≈200 k lines/s, the latter
  multi-threaded); rxjs (2.1 k LOC) 1.1 s → 0.1 s (fixed costs dominate); editor load 9.6 s → 1.2 s.
  [verified: https://devblogs.microsoft.com/typescript/typescript-native-port/]
- The 7.0 beta (2026-04-21) runs 4 checker workers by default (`--checkers`). The partition is
  identical for identical inputs, and the workers may duplicate work. A more efficient `--watch`
  was still "coming". [verified: https://devblogs.microsoft.com/typescript/announcing-typescript-7-0-beta/]
- Cancellation is polled: `isCanceled()` is `ctx.Err() != nil`, checked per top-level statement,
  per deferred node and in some property loops. A cancelled checker sets `wasCanceled`, and any
  later use panics ("Checker was previously cancelled").
  [verified: raw internal/checker/checker.go and utilities.go in microsoft/typescript-go; that repo
  was archived 2026-08-31 and its last checker.go commit is 5252589 (2026-08-18)]
- Interned ids leaking into output:
  - TS gives type IDs in encounter order and sorts unions by ID, so adding an unrelated `const x =
    500` above a function flips its emitted return type from `100 | 500` to `500 | 100`;
  - parallel checkers made this non-deterministic;
  - TS 7 sorts types and symbols by content;
  - TS 6's `--stableTypeOrdering`, which matches TS 7, costs up to 25% of check time.
  [verified: https://www.typescriptlang.org/docs/handbook/release-notes/typescript-6-0.html]

## 4. Lean 4

- Reuse happens at command level and inside commands (headers, bodies, tactic blocks; v4.9.0, PR #3940).
  [verified: https://lean-lang.org/doc/reference/latest/releases/v4.9.0/]
- Reuse is decided by syntax: if all syntax inspected so far is unchanged, the old state is reused.
  The server nonetheless reprocesses at least everything from the edit downwards, and a cancel
  token fires as soon as reuse is ruled out. This is prefix reuse, weaker than dependency-keyed reuse.
  [verified: https://lean-lang.org/doc/api/Lean/Language/Lean.html]
- Partial results: snapshots form a tree of tasks, and the server reports messages in preorder as
  each promise resolves. Every promise created must be resolved, or the server blocks forever;
  helper wrappers enforce this. `SnapshotTask` carries an optional `cancelTk?`.
  [verified: https://lean-lang.org/doc/api/Lean/Language/Basic.html]
- Cancellation is cooperative: `IO.cancel` sets a flag that the task must poll with
  `IO.checkCanceled`. [verified: https://lean-lang.org/doc/api/Init/System/IO.html]
- Error recovery: a failed subterm becomes a *synthetic* `sorry`, which "tends to suppress further
  errors". Whoever creates one must make sure an error was logged.
  [verified: https://lean-lang.org/doc/api/Lean/Meta/Sorry.html]
- Reuse bugs seen: stray tokens inside `by` blocks blocked reuse (PR #4268). Whitespace reuse showed
  goals ahead of the cursor (PR #4395).
  [verified only as listed in the v4.9.0 release notes above; PRs not opened]

## 5. Sorbet (Ruby)

- About 100 k lines/s per core on Stripe's code (informal, 2020). Speed comes from local-only
  inference (method signatures are required), interned names, flat 32-bit-indexed arenas and lazy
  error text. [verified: https://blog.nelhage.com/post/why-sorbet-is-fast/]
- In LSP mode, most edits take a fast path. The slow path (full retypecheck) is for edits touching
  over 50 files or a definition used in over 50 files, a new file, a changed class hierarchy, or an
  unrecoverable syntax error.
- A running slow-path check is cancelled when a new edit makes the combined edit fast-path. Idle
  returns "immediately or after a few hundred milliseconds".
  [verified: https://sorbet.org/docs/server-status]

## 6. Roc

- Roc's goal is builds that "normally feel instant": under 1 s on a median machine, ideally under
  ~100 ms. The page names no mechanism. [verified: https://www.roc-lang.org/fast]
- The Zig rewrite caches parsed and checked data per file on disk and loads it without re-parsing
  when inputs are unchanged. [verified: https://rtfeldman.com/rust-to-zig]
- Issue #11348 (2026-09-13): checked canonical type keys were re-hashed over the whole reachable
  type graph for each root, costing 31.9% inclusive time (SHA-256 alone was 24.65% of self time).
  The proposed fix makes keys compositional, Merkle-style. [verified: https://github.com/roc-lang/roc/issues/11348]
- In-process per-module incremental checking. [not verified: no primary source located]

## 7. Dart / Flutter hot reload

- Hot reload keeps app state. It re-runs neither `main()` nor `initState()`, and changed
  initializers of globals and static fields are not re-run.
- A hot restart is needed for native code changes, enum-to-class changes and changes to generic
  type parameters.
- Example latency: "Reloaded 1 of 448 libraries in 978ms".
  [verified: https://docs.flutter.dev/tools/hot-reload]
- How the Dart VM migrates existing instances when their fields change.
  [not verified: the SDK hot-reload doc URL returned 404]

## 8. Single-threaded wasm32 builds

- rust-analyzer-wasm (archived 2025-05-29):
  - depends on `wasm-bindgen-rayon`, so it has threads, and uses `stacker`;
  - analyses one virtual crate plus bundled `fake_std`, `fake_core` and `fake_alloc` texts;
  - uses a fixed cfg and no cargo.
  [verified: GitHub page, plus raw ra-wasm/Cargo.toml and src/lib.rs]
- Lean: the official web editor runs the server on a server machine. [verified: lean4web README]
- A community in-browser Lean build:
  - is a pthread build, so the page must be cross-origin isolated (needed for SharedArrayBuffer);
  - is 70-101 MB of wasm;
  - hits the browser call-stack limit on four large proofs.
  [verified: https://github.com/cauli/lean4-wasm-in-browser]
- SharedArrayBuffer requires a secure, cross-origin-isolated page (COOP/COEP headers).
  [verified: MDN SharedArrayBuffer]
- tsgo in wasm: participants cite Go's wasm code generation (control flow emulated by a switch
  statement, no interior pointers in WasmGC) and a playground at ~650 ms per compile.
  [partially verified: https://github.com/microsoft/typescript-go/discussions/514; the fetch
  summary said no official wasm build is planned, but I did not read that statement myself]
- Rust's `wasm32-unknown-unknown` defaults to `-Cpanic=abort`, so `catch_unwind` cannot catch a
  panic there. Unwinding needs nightly `-Zbuild-std`, possible since wasm exception handling was
  standardised in mid-2025. [verified: https://doc.rust-lang.org/rustc/platform-support/wasm32-unknown-unknown.html]

## Answers to the five questions

1. **Full recheck of 3.6 k lines in ≤16.7 ms?** Possible; no precedent. The plan's diagnostics lane
   runs at 358 k lines/s (10 ms) and its check alone at 596 k (6 ms). Measured prior art: Sorbet
   ~100 k lines/s per core, tsc ≈19 k, tsgo ≈200 k multi-core, Zig semantic analysis ≈36 k.
   Line rates mislead, because Boon lines are sparse (TodoMVC: ~4.9 k parsed expressions in 3,576
   lines, audit/representations.md). The plan's ≈1.47 µs per checked expression (6 ms / 4,081) has
   no verified external per-expression figure to compare with; S2 must settle it.
2. **Fingerprint reuse.** Salsa (equality backdating), Zig (src_hash plus PO counts), tsc (shapes)
   and Sorbet (fast path on unchanged signatures) all do it. Their documented pitfalls: id order
   (TS), cycles meeting validation (salsa #1310/#1338, Zig 0.16), expensive fingerprints (TS #64464,
   Roc #11348), stale side data (TS #64424), run-to-run noise (rust-analyzer #1552).
3. **Cancellation.** Unwinding (salsa) needs `catch_unwind` at every boundary; missing one killed
   a thread pool (ruff #12183). Polling (tsgo `ctx.Err()`, Lean `checkCanceled`, Zig
   `error.Canceled`) needs explicit checkpoints and leaves partial state that must be discarded.
   rust-analyzer retries a superseded request or answers `ContentModified`.
4. **Cascades.** An absorbing error (`{unknown}`, synthetic sorry, transitive failure) plus a rule
   that drops secondary diagnostics involving it. No last-good interfaces anywhere.
5. **wasm.** None of the four ships an official single-threaded browser build of the full tool.
   What they gave up: threads (or SharedArrayBuffer isolation), stack depth, the real std library
   and build system, and speed (Go wasm).

## Implications for the plan

1. **§4.7 "Cold first" and §6 warm rows (`plan:655`, `plan:814-815`).** Contradicts the plan's
   premise as prior art; suggests a spike.
   - Every production system with sub-frame edits gets there by reuse (ty 4.7 ms; Zig 62-65 ms;
     Sorbet's fast path), and the plan's cold rate has no precedent.
   - NovyWave's own cold target (≈30 ms) is already over the frame, so reuse is *required*, not
     "if needed" (`plan:1035`).
   - Proposed: make per-definition reuse a committed P6 item; add an S2 exit row "TodoMVC full
     diagnostics lane p95 ≤12 ms, or reuse moves into P5"; keep the full recheck as the warm==cold
     oracle rather than as the product path.
2. **§4.7 reuse key and backdating (`plan:658-661`).** Supports, with a change.
   - The cost of the fingerprint must be gated. TS computed shapes by declaration emit and made
     warm slower than cold (#42960, #64464). Roc's whole-graph re-hash cost 31.9%.
   - Proposed: compute the interface fingerprint as a by-product of hash-consed scheme compaction,
     compositional over child hashes. Add a §9.3 gate: cold with fingerprints ≤1.05× cold without,
     and a counter for fingerprint bytes hashed per edit.
3. **Cycles against re-verification (D9, §4.4 root group, `plan:658-661`).** Suggests a change
   (high).
   - salsa #1310/#1338 and Zig 0.16 show warm≠cold wherever cycle membership from an earlier
     revision leaks into verification.
   - Proposed: recompute SCCs from the *current* resolve graph every revision; key each SCC or
     group atomically and never deep-verify through the old revision's edges; add warm-corpus
     classes that create and then break a HOLD cycle or a recursion error.
4. **Determinism of order (§4.2 interner, `plan:314-315`; §4.4 `plan:497-499`; D7).**
   Contradicts the design as it stands in the notes.
   - `plan:315` bans session SymbolIds only from *hashes*. The design notes sort tag rows by
     SymbolId (compiler_rewrite_notes/checker.md), and the session interner is append-only, so
     SymbolId order depends on edit history.
   - TS 6/7 is the exact precedent: output changed with unrelated declarations, and a stable order
     cost up to 25%.
   - Proposed: sort canonical rows by name bytes (or hash with a bytes tie-break); define D7's
     first-appearance order on source positions; add an O3 variant comparing a fresh process with a
     warm session after 500 scripted keystrokes (plan bytes, diagnostics, snapshot).
5. **Cancellation (`plan:670-671`; §4.9 `plan:723`).** Supports polling, and suggests two changes.
   - (a) Commit memo results only per *completed* group. tsgo discards a checker that was cancelled
     mid-run, and the design note's "results from a cancelled revision stay in the memo" is safe
     only at that granularity.
   - (b) Make `Poll` check a flag *and* a deadline. In a single-threaded wasm worker without
     cross-origin isolation, a flag set by the main thread cannot be read while the worker is busy;
     the compile must run in slices and yield. (The event-loop behaviour is inferred, not verified.)
   - Also poll during front-end and snapshot serialization, not only in the checker.
6. **Absorbing error type (`plan:662-664`).** Supports; suggests writing the rules into the
   static semantics.
   - Any conflict with Error on either side, at any nesting depth, is dropped (the zip rule of
     rust-analyzer PR #16583).
   - Every Error node traces back to exactly one logged diagnostic (Lean's synthetic-sorry
     contract), checked as a debug assertion.
   - D24 unused-name errors and D25 unreachable-arm warnings are suppressed inside definitions that
     contain Error.
   - Typing-mode corpus entries include half-typed identifiers and unclosed brackets.
7. **Staged publication (`plan:665-669`).** Suggests a change.
   - rust-analyzer publishes syntax diagnostics before semantic ones, per generation, and only for
     open files. Lean streams snapshots in preorder. "Laziness" beats incrementality.
   - Proposed: publish front-end diagnostics first (≈1.2 ms); serve hints and occurrences on
     demand from the retained `Checked`, outside the per-keystroke snapshot (protects the ≤2 ms IPC share).
8. **§9.3 and P6 warm protocol (`plan:1032`, `plan:1159-1160`).** Suggests a change.
   - Gate "first edit after cold open" separately from steady-state edits (the TS #64464 inversion;
     Zig reports cold, first and second edits).
   - Add a warm/cold ratio ceiling per fixture.
   - Report definitions rechecked per edit class.
   - Add a long-session retained-RSS gate after about 10 k typing keystrokes. An append-only
     interner grows with every partial identifier, and Zig's InternPool GC is still unimplemented.
9. **Warm test format (§7, P6).** Suggests a change.
   - Adopt a Zig-style `#update=` sequence file with the expected diagnostics after each step.
     Check warm==cold at *every* step in CI: Zig's harness found a bug immediately and Zig 0.16
     still has incremental miscompilations.
   - Derive dependency edges from the current revision only; TS #64424 lost edits held in side data.
10. **Position-free fingerprints (`plan:316-318`, definition-relative spans).** Supports, with a test.
    - Lean #4395 shows that reuse leaks position-dependent output.
    - Add an edit class: insert lines above a definition that has a type error, and require the
      diagnostic span and hover range to move with it.
11. **Durability (§4.4 static catalog base).** Supports: the immutable catalog is salsa's HIGH
    durability by construction. Count per-edit verification of NovyWave's 1,389 keys; do not assume it free.
12. **Parallel checking later (§12.2, O3).** Supports the O3 threads-1-versus-N oracle. If threads
    are added, partition deterministically and order by content, as TS 7 does.
13. **Q17 wasm (`plan:1229`, §4.9).** Supports: `no catch_unwind` is required anyway, because
    wasm32 aborts on panic.
    - Add bounded parser and checker recursion with a positioned "too deeply nested" diagnostic
      (browser stack limits hit Lean; rust-analyzer-wasm needed `stacker`).
    - Measure the single-threaded TodoMVC browser latency early in P1.
14. **Hot reload keeps state (D35, §4.6).** Suggests a change.
    - Flutter keeps state, ignores changed initializers, and requires a restart when a generic or
      enum shape changes.
    - Boon must state what hot reload does when a HOLD's starting value or type changes: keep, reset
      with a dev-window notice, or reject. Key the decision on state identity plus a state-type
      fingerprint.

## Sources

1. https://salsa-rs.github.io/salsa/reference/algorithm.html (fetched)
2. https://rust-analyzer.github.io/blog/2023/07/24/durable-incrementality.html (fetched)
3. https://rust-analyzer.github.io/book/contributing/architecture.html (fetched)
4. https://rust-analyzer.github.io/blog/2020/07/20/three-architectures-for-responsive-ide.html (fetched)
5. https://docs.rs/salsa/latest/salsa/enum.Cancelled.html (fetched; salsa 0.28.5)
6. https://docs.rs/salsa/latest/salsa/trait.Database.html (fetched)
7. https://github.com/astral-sh/ruff/pull/12183 (fetched)
8. https://github.com/rust-lang/rust-analyzer/pull/16583 and /pull/20022 (fetched)
9. https://github.com/rust-lang/rust-analyzer/issues/1552 (fetched)
10. https://raw.githubusercontent.com/rust-lang/rust-analyzer/master/crates/rust-analyzer/src/handlers/dispatch.rs and .../main_loop.rs (downloaded, grepped)
11. https://github.com/salsa-rs/salsa/issues/1310 and /issues/1338 (fetched)
12. https://astral.sh/blog/ty (fetched)
13. https://ziglang.org/download/0.14.0/release-notes.html (fetched); 0.15.1 and 0.16.0 (downloaded, grepped)
14. https://gist.github.com/mlugg/73b3e60c803006f3556d87c9ed3e8a0e (fetched); https://mlugg.co.uk/posts/incremental-compilation-internals/ (403, not read)
15. https://raw.githubusercontent.com/ziglang/zig/master/src/Zcu.zig (downloaded; GitHub mirror, last commit 2025-11-24)
16. https://github.com/ziglang/zig/pull/20688 (fetched)
17. https://rtfeldman.com/rust-to-zig (fetched) and https://lobste.rs/s/axdfjx/how_our_rust_zig_rewrite_is_going (fetched)
18. https://github.com/roc-lang/roc/issues/11348 (fetched); https://www.roc-lang.org/fast (fetched)
19. https://devblogs.microsoft.com/typescript/typescript-native-port/ (fetched)
20. https://devblogs.microsoft.com/typescript/announcing-typescript-7-0-beta/ (fetched)
21. https://www.typescriptlang.org/docs/handbook/release-notes/typescript-6-0.html (fetched)
22. https://github.com/microsoft/TypeScript/pull/42960, /issues/64464, /issues/64424 (fetched)
23. https://github.com/microsoft/TypeScript/wiki/Standalone-Server-(tsserver) (fetched)
24. https://raw.githubusercontent.com/microsoft/typescript-go/main/internal/checker/checker.go and utilities.go (downloaded; repo archived)
25. https://github.com/microsoft/typescript-go/discussions/514 (fetched, summary only)
26. https://lean-lang.org/doc/reference/latest/releases/v4.9.0/ (fetched)
27. https://lean-lang.org/doc/api/Lean/Language/Lean.html, /Lean/Language/Basic.html, /Init/System/IO.html, /Lean/Meta/Sorry.html (fetched)
28. https://github.com/leanprover-community/lean4web/blob/main/README.md (fetched); https://github.com/cauli/lean4-wasm-in-browser (fetched)
29. https://blog.nelhage.com/post/why-sorbet-is-fast/ and https://sorbet.org/docs/server-status (fetched)
30. https://docs.flutter.dev/tools/hot-reload (fetched); https://github.com/dart-lang/sdk/blob/main/docs/Hot-reload.md (404)
31. https://github.com/rust-analyzer/rust-analyzer-wasm (fetched, plus raw Cargo.toml and lib.rs)
32. https://doc.rust-lang.org/rustc/platform-support/wasm32-unknown-unknown.html (fetched)
33. https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/SharedArrayBuffer (fetched)
