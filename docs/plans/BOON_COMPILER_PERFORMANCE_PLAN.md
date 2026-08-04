# Boon Cold-First Compiler Performance Plan

Date: 2026-08-02

Last architecture reconciliation: 2026-08-03

Status: authoritative blocking implementation contract for compiler latency,
memory, invalidation, cancellation, and compiler-service ownership.

Under the combined order in [`steps.md`](steps.md), this plan is implemented
before the remaining native-recovery exit and before later language, formal,
packed-runtime, console, product, or game work. Documentation reconciliation is
the first slice; passing the cold compiler gates is the first implementation
exit.

[`BOON_COMPILER_ARCHITECTURE_REFACTOR_PLAN.md`](BOON_COMPILER_ARCHITECTURE_REFACTOR_PLAN.md)
is the current high-leverage execution map derived from the live
post-`c870358` audit. This plan remains authoritative for every budget and exit;
the refactor plan fixes the order and deletion criteria for reaching them.

## Purpose And Authority

Boon must feel suitable for interactive authoring and ordinary scripting even
when no previous compiler process, query database, or artifact cache exists.
Switching to a larger checked-in example must not expose graph explosion as a
seconds-long editor stall, and persistent-session optimizations must not hide a
slow source-to-verified-plan compiler.

This plan owns:

- compiler latency and peak-memory budgets;
- source-unit snapshots, compiler-session state, dependency invalidation, and
  cancellation;
- compiler phase instrumentation, work counters, benchmark protocol, and
  immutable compiler-artifact reuse;
- the internal representations needed to meet those budgets without changing
  Boon semantics;
- editor and preview integration with the one compiler service.

It does not own or weaken:

- public language, type, `OUT`, exact-value, `WHERE`, or persistence semantics;
- proof obligations, evidence acceptance, or the meaning of
  `ContractVerifiedProgram`;
- the public verified-artifact spine, packed software-runtime phases, native
  input/WGPU evidence, console/processor/FPGA behavior, or product acceptance;
- Boon Orchard or any other game implementation.

The owning language, formal, packed, persistence, native, and console plans
remain authoritative for those concerns. Where an older plan describes a
compiler representation or execution order that conflicts with the budgets
here, this plan owns the performance mechanism while preserving that plan's
semantics and acceptance meaning.

## Non-Negotiable Outcome

Cold source compilation is normative. Both of these modes must independently
pass with compiler caches disabled:

1. a fresh invocation of the prebuilt `boon` compiler process; and
2. an in-process request against a newly created, empty compiler database.

OS file caching may remain natural to the machine, but no prior parse tree,
typed graph, semantic graph, proof result, lowered plan, serialized artifact,
or compiler daemon state may be reused by either cold mode. Incremental state
and content-addressed artifacts are a second optimization layer; they cannot
be used to satisfy a cold gate.

A `/goal` run does not reach a performance stopping point by reconciling
documentation, adding counters, extracting a crate, producing a directional
sample, committing a checkpoint, or passing a focused correctness test. Those
are implementation steps. While any required report is missing or red, the
next action is the highest-impact measured compiler change or the smallest
harness change that makes that blocker measurable. After an authorized phase
checkpoint, continue directly with the next failing gate. Do not wait for a
second `/goal resume` instruction and do not enter a later plan.

The default and measured compiler is single-threaded. Parallel compilation is
permitted only after the single-threaded cold targets pass and only when it
improves a separately measured workload without increasing interactive
contention or making correctness depend on scheduling.

## Current Baseline

The current post-reboot debug-profile checkpoint is historical diagnostic
evidence, not an accepted performance ceiling:

| Fixture | Package / compiler-input lines | Source to `MachinePlan` | Peak RSS | Historical plan SHA-256 |
| --- | ---: | ---: | ---: | --- |
| Counter | 140 / 140 | 0.09 s | 29,992 KiB | `dc1fe51b659d1746a0b0b4ae2dcba21d50a9426499eb2bde28dbed988e6cfb08` |
| Physical TodoMVC | 3,647 / 3,576 | 2.02 s | 146,340 KiB | `c9a12cd0a1bcf748a20e3a072afa09d0f923c2c9dbd664f2343d343494404f96` |
| NovyWave | 11,994 / 11,923 | 20.68 s | 1,000,416 KiB | `4d3c284a9240cdc68c70aff7f30c570367e285cc1e8f823585900829bafd8ff7` |

NovyWave's package count includes its separate 71-line `BUILD.bn`; the compiler
input count names the source bundle actually passed to the Client compiler.

These hashes identify the historical artifacts that produced the measurements;
they are not all valid semantic oracles. The NovyWave `4d3c284a...` artifact is
known to under-approximate the reachable persistence type of
`store.selected_value_column_width_key`: it records only `Compact`, `Normal`,
and `Wide`, while checked-in source produces `Widest` on growth and consumes it
on shrink. The later `c77dabc`-like and retained-overlay artifacts both
include all four variants and have byte-identical persistence sections. Never
restore `4d3c284a...` or update the budget to a faster candidate merely to make
the hash gate green. The artifact-oracle repair defined below is the first
current correctness gate.

The traced NovyWave path currently attributes approximately 4.84 seconds to
typechecking, 2.84 seconds to semantic materialization/execution, 3.81 seconds
to callable-dependency-manifest construction, 9.51 seconds to the complete
semantic portion, and 0.81 seconds to backend work. `boon_verify` itself is not
the 3.81-second blocker; manifest discovery inside semantic construction is.

The editor currently pays avoidable latency in addition to those compiler
costs: a fixed 90 ms debounce precedes a whole-project parse/check/editor-
semantics pass, and preview publication performs another whole-project
`compile_machine_plan` pass. The landed parser now parses source units
independently, but cold project assembly still constructs and repeatedly
validates one global program. Checker-owned caches die with each borrowed
checker, and an in-flight compile cannot be canceled.

These observations choose the first architectural work. They do not authorize
fixture-specific shortcuts, reduced diagnostics, skipped verification, longer
timeouts, or altered plan semantics.

The current structured-counter checkpoint adds a narrower directional
measurement of complete checked diagnostics. The directly invoked debug
producer measured Counter at about 14.9 ms and NovyWave at 744.1 ms: about
332.4 ms of parsing and 411.7 ms of typechecking, with 76,412 KiB peak RSS.
A separate parser trace measured about 153.8 ms in raw unit parsing and 126.5
ms in canonical project validation. The NovyWave parse observed 73,571 tokens
but 1,060,559 validation visits; the typechecker performed 35 inference rounds
and 24,218 diagnostic-replay requests. These are not release acceptance
numbers, but they identify repeated project validation/assembly and recursive
checked-diagnostic projection as the first frontend hypotheses to confirm with
one current release producer before a large rewrite. The 20.68-second full-plan
baseline remains the relevant evidence for later semantic, proof, and backend
work; the two scopes must never be presented as the same measurement.

## Performance Contract

### Cold Budgets

Each cold mode must pass the following single-threaded p95 limits with compiler
caches disabled. Every scored sample must remain below the listed peak-RSS
limit:

| Fixture | Complete checked diagnostics p95 | Verified runnable `MachinePlan` p95 | Peak RSS |
| --- | ---: | ---: | ---: |
| Counter | 10 ms | 50 ms | 32 MiB |
| Physical TodoMVC | 75 ms | 300 ms | 128 MiB |
| NovyWave | 250 ms | 1,000 ms | 384 MiB |

"Complete checked diagnostics" parses and checks the entire requested source
snapshot and returns every diagnostic required by the typechecking contract. It
is not a syntax-only, visible-range, or first-error response.

"Verified runnable `MachinePlan`" traverses the complete public artifact spine
through proof acceptance and backend lowering. A non-executable internal
semantic artifact cannot satisfy this budget.

### Warm Interaction Budgets

Against an already opened project:

- an edit's complete affected checked diagnostics finish within 16.7 ms p95,
  25 ms p99, and 33.4 ms maximum;
- a valid constant-dependency-cone edit reaches a replacement verified preview
  within 100 ms p95 and 200 ms maximum;
- lookup of an already loaded verified bundle completes within 1 ms;
- an example switch is acknowledged within 16.7 ms and reaches the final
  presented native frame within 100 ms p95 and 200 ms maximum;
- a generation superseded by a newer edit or switch stops consuming compiler
  work within 8 ms;
- no superseded diagnostic, proof, plan, or preview generation is published.

Compiler completion and native presentation are separate measurements. The
native pipeline remains responsible for proving the final presented frame; it
must not subtract compiler time or substitute a compiler acknowledgement for a
readback-proved presentation.

### Scaling Budgets

Checked-in synthetic fixtures independently scale call depth, call-site count,
contextual call sites, static branch count, source-unit count, and dependency-
cone size. After fixed setup overhead, doubling one controlled dimension may
increase the owning phase's counted work and allocation by at most 2.2 times.

Ordinary callable bodies are stored once. A pruned static branch creates no
semantic dependency, proof obligation, or backend node. A constant-cone edit
does not reparse, recheck, reseal, reverify, or relower an unrelated unit or
component. A timeout increase or a larger thread pool cannot satisfy a scaling
gate.

## Compiler Architecture

### Source And Checked Database

- Parse source units independently and retain real spans, source-unit
  revisions, and stable declaration identities across unrelated edits.
- Replace the borrowed `Checker<'a>` plus rebuilt checked-output pass with one
  owned compiler database. The checked representation is the sole structural
  owner consumed by later stages; parser semantic side channels and backend
  source-name rediscovery are forbidden.
- Intern repeated names, type terms, shapes, paths, and immutable values. Store
  hot entities in dense typed arrays or boxed slices and store graph edges in
  compact offset/edge arrays where traversal dominates mutation.
- Use snapshot-local dense IDs for compact execution while maintaining explicit
  maps to stable source-unit and declaration identities. Dense IDs are not
  persistence identities.
- Use one Boon-owned constraint/worklist engine with bounded iterative queues,
  union-find where appropriate, SCC/component indexes, bitsets, and generation-
  stamped scratch vectors. Salsa may later schedule invalidated compiler
  queries, but it is not the Boon type solver and is not required by this plan.
- Keep one compact checked-expression representation through the compiler
  spine. Debug trees, reverse-name maps, and report sidecars are constructed
  only for a request that asks for them and must not remain reachable from the
  normal executable artifact.

### Rust Build And Crate Boundaries

Rust build latency is development infrastructure, not a substitute for the
measured Boon compiler latency. Use one Cargo invocation at a time with two
build jobs on the reference machine. A third job may be adopted only after a
measured dependency-boundary change shows a benefit without renewed memory or
I/O pressure; do not run independent Cargo producers concurrently.

Keep the existing profile intent unless an A/B measurement justifies a change:
selected hot compiler crates use optimized dev builds with line tables, focused
test harnesses favor quick unoptimized correctness builds, and acceptance uses
the ordinary `release` producer. `boon_parser`, `boon_typecheck`,
`boon_semantic`, `boon_ir`, `boon_plan`, and `boon_compiler` now have the
documented package-local dev `opt-level = 2`/line-table treatment; focused test
overrides keep the largest compiler test harnesses at level 0. Preserve this
measured intent unless another bounded A/B reports Rust rebuild wall time/RSS
and direct debug Counter/NovyWave latency. These settings cannot satisfy or
weaken the release cold gates. Do not add LTO,
one-codegen-unit builds,
`target-cpu=native`, global `RUSTFLAGS`, disabled correctness checks, or a
custom profile merely to improve a reported number. A proposed profile change
must report Rust build wall time and peak RSS as development-loop evidence,
then report Boon latency, RSS, diagnostics, work counters, and artifact hashes
from the same source before and after. Keep it only when the combined tradeoff
is useful; never relabel a custom or debug producer as release acceptance.

The normal build discipline is:

- use `cargo check` or the smallest focused debug test for a correctness edit;
- pass `--jobs 2` to the one active Cargo build/test on the reference machine;
- build `boon_cli` in release once per coherent milestone candidate, then run
  `target/release/boon_cli` directly for every repeated fixture sample;
- build `xtask` only when its source or report contract changed, then invoke
  the prebuilt `target/debug/xtask` directly; and
- inspect active Cargo/rustc processes before a long build and do not start a
  second producer while one is alive.

Split crates at stable ownership and dependency-invalidation boundaries, not
merely because a source file is large:

1. complete independent source-unit parsing and project assembly before moving
   the stable AST, syntax DTOs, language registry, and canonical vocabulary to
   `boon_syntax`;
2. move the complete immutable checked graph and its pure operations to
   `boon_checked` so semantic, verification, and IR consumers do not depend on
   solver implementation;
3. isolate the large `MachinePlan` backend from compiler-session orchestration;
4. split semantic passes only after the checked boundary removes cycles and
   fresh timings identify independently owned components.

Each move is a flag-day cutover: update production consumers atomically and
delete the old definitions without aliases, blanket re-exports, duplicate DTOs,
feature-selected owners, or fallback paths. `ParsedProgram` and
`CheckedProgram` remain safely unforgeable proof-bearing products. A crate
boundary must not expose a safe constructor that lets a parser snapshot,
checked DTO, semantic core, or unverified program enter the executable spine.

Every proposed crate split must publish its intended dependency graph before
editing and pass four gates after cutover:

1. the moved API has one semantic owner and introduces no dependency cycle or
   reverse dependency on its former implementation crate;
2. touching a common parser, solver, session, or backend implementation file
   rebuilds a strictly smaller relevant crate set, measured once before and
   once after with the same Cargo state;
3. focused behavior, diagnostic parity, proof-boundary, and artifact-hash tests
   remain unchanged; and
4. the split is followed by the measured runtime optimization it enables.

If it only moves files, increases the affected dependency graph, or fails to
enable an ownership/invalidation change, use modules instead of another crate.
A successful split improves Rust organization and rebuild isolation; it is not
itself evidence that Boon source compiles faster and is never a reason for the
performance goal to pause.

#### Current `boon_checked` flag-day dependency graph

The Phase 1 checked-product extraction uses this one-way graph:

```text
boon_contract   boon_data   boon_document_model   boon_effect_schema
       \            |              |                     /
                        boon_checked
                       /      |      \
          boon_semantic   boon_verify   boon_ir
                       \      |      /
boon_parser + boon_syntax -> boon_typecheck -> boon_compiler
                               |
                       editor/compiler callers
```

- `boon_checked` owns `Type`, immutable checked IDs/tables/expressions/calls,
  `CheckedProgram`, checked lowering metadata, and their pure lookup and
  substitution operations. It does not depend on `boon_typecheck`,
  `boon_parser`, `boon_syntax`, or `ena`.
- `boon_typecheck` owns parsing-to-checked construction, constraints,
  unification, worklists, diagnostics projection, and report/session policy. It
  depends on `boon_checked`; the reverse edge is forbidden.
- `boon_semantic`, `boon_verify`, and `boon_ir` consume the immutable product
  from `boon_checked` directly and do not rebuild merely because solver
  implementation changes. Orchestration crates that invoke checking may depend
  on both crates, but must import checked DTOs from their owning crate.
- `CheckedProgramFields` remains an inspectable/serializable DTO, while sealing
  it as a proof-bearing `CheckedProgram` is an unsafe invariant boundary used
  only after successful checking. No safe constructor, compatibility re-export,
  duplicate DTO, or feature-selected old owner is permitted.
- After cutover, the enabled runtime optimization is an owned checked database
  with compact/interned immutable terms and direct consumption by later phases;
  the split itself does not satisfy a Boon latency gate.

Checked-boundary cutover evidence (2026-08-03):

- `boon_checked` is the sole owner of the immutable checked model and pure
  checked operations. Cargo metadata confirms that it has no parser, syntax,
  typechecker, or `ena` dependency; `boon_semantic`, `boon_verify`, and
  `boon_ir` depend on it normally and keep `boon_typecheck` only as a test
  dependency. No checked DTO is re-exported by `boon_typecheck`.
- The pre-cutover solver/model owner forced semantic, verification, IR,
  compiler, runtime, and CLI consumers into the affected Rust rebuild set. On
  a stabilized post-cutover target, touching only
  `crates/boon_typecheck/src/lib.rs` and checking the CLI spine rebuilt exactly
  `boon_typecheck`, `boon_compiler`, `boon_runtime`, and `boon_cli`; semantic,
  verification, IR, and `boon_checked` stayed fresh. The controlled post-cutover
  sample took 1.03 seconds and 199,148 KiB peak RSS with one offline two-job
  Cargo invocation.
- Six direct checked-operation/serialization/sharing tests, the unsafe-boundary
  compile-fail doctest, all 78 non-ignored typechecker library tests, and the
  product-scale checked-diagnostic projection parity test pass. Downstream
  semantic/verification/IR test binaries compile. One IR test and one verify
  test that fail in their broad suites were reproduced with the same errors on
  checkpoint `2ef9b1d`, before the cutover, and are not split regressions.
- The extraction removed the unused `ena` allocation table from the
  typechecker. The split has not been counted as a Boon runtime win; the next
  Phase 1 work is the owned database, compact/interned terms, and measured
  solver/worklist reductions it enables, followed by fresh release profiling.

Current cold-diagnostics candidate evidence:

- Immutable List/Set element edges now share checked type subtrees, the ordered
  diagnostic projector reuses its task/continuation storage, and source payload
  analysis indexes `WHEN` selectors once. The parser's logical-line and
  `ParserItem` views share unchanged symbol vectors; multiline normalization
  remains copy-on-write. Planned-feature lookup and hidden-identity rejection
  retain their diagnostic order without repeating registry-wide/name-wide
  searches for every token.
- Against the checked-boundary release baseline, NovyWave fresh-process
  allocation work fell from 1,997,515 calls / 225,753,409 bytes to 1,933,602
  calls / 219,951,108 bytes. The checked-result SHA-256 remains
  `495ef4195e0be4869e431c4036b74b6d6397c4b28b7fe7a8a76c76400a7b7992`.
- One complete three-setup/30-scored direct protocol over checkpoint `677d09d`
  passes all six cold diagnostics time/RSS combinations:
  Counter is 5.34/5.27 ms p95 fresh/empty, physical TodoMVC is 61.24/61.30 ms,
  and NovyWave is 234.48/248.86 ms. Maximum observed RSS is 11,140 KiB,
  27,132 KiB, and 70,868 KiB respectively. Each fixture/mode has one unchanged
  diagnostic count and one unchanged checked-result digest.
- Checkpoint `677d09d`'s NovyWave empty-session result has only 1.14 ms of p95
  headroom. It is a passing historical candidate, not the Phase 1 exit, and its
  timing evidence became stale with the next frontend edit.
- The next ownership slice makes `ParsedProgram` one immutable `Arc`-backed
  snapshot, removes the lifetime from both `Checker` and
  `CheckedProgramBuilder`, passes the complete 64-parser/80-typechecker suite
  including both product-scale ignored oracles, and retains the exact NovyWave
  checked-result digest. Function statements use one flat path-segment arena
  rather than one allocation per function. Current directional release samples
  are 230.83/229.30 ms fresh/empty; fresh allocation work is 1,933,612 calls /
  219,965,836 bytes, only 10 calls / 14,728 bytes above `677d09d` while
  establishing the required lifetime-free ownership boundary.
- The subsequent measured worklist slice makes the complete
  `flow_cache_dependents` index the sole expression-propagation lane instead of
  redundantly re-enqueueing parent and pattern-selector edges, and recycles the
  bounded dense pending-set buffers between fixed-point rounds. Both
  product-scale oracles and the exact checked-result digest remain unchanged.
  A current release fresh-process sample is 230.78 ms with 1,932,386
  allocations / 219,267,245 bytes: 1,226 calls / 698,591 bytes below the
  lifetime-free ownership checkpoint. The traced checked-program builder falls
  directionally from 134.02 to 130.61 ms. This is an owner-level edit-loop
  improvement, not percentile acceptance; inference still takes 35 rounds and
  5,060 call visits.
- Call inference plans now distinguish concrete fixed-result builtins whose
  checked product is independent of changing actual-input flow. They remain in
  the expression-flow graph where projection inference requires them, but are
  seeded only once in the call-instantiation worklist; `Field/*` projections
  and every generic, mode-sensitive, contextual, OUT, user, and dependency-
  catch call retain their input edges. The complete 80-test typechecker suite,
  both product oracles, and the fail-closed full-sweep audit pass with the exact
  NovyWave digest. NovyWave call visits fall from 5,060 to 3,848, no-op visits
  from 1,964 to 752, and input enqueues from 2,465 to 1,253. Directional current
  release samples are 229.97/228.51 ms fresh/empty; the fresh sample uses
  1,929,058 allocations / 219,061,685 bytes, 3,328 calls / 205,560 bytes below
  checkpoint `69614c2`. This is not percentile acceptance: the remaining 35
  rounds and contextual/user-call owner remain open.
- Recursive checked-flow inference now clones the immutable parsed-program
  owner once per root and borrows that snapshot through read and projection
  recursion, instead of performing an atomic shared-owner clone/drop on every
  cache miss. Solver work, allocation counts, the exact digest, and the complete
  80-test suite remain unchanged. A three-sample alternating release edit-loop
  batch has 227.33/227.73 ms fresh/empty medians and 171.50/172.32 ms typecheck
  medians. This small directional win is not percentile acceptance and does not
  close the remaining construction, interning, or contextual/user-call owners.
- Hidden FLUSH propagation now stores dense expression results directly and
  computes the derived expression/declaration closure from the authoritative
  reverse dependency index instead of rescanning the entire program to a fixed
  point. Test builds compare that queue against the old whole-program oracle.
  Direct AST-child walks also use four-entry inline buffers instead of one heap
  allocation per ordinary node. The complete 80-test suite and exact NovyWave
  digest pass. A six-sample alternating release edit-loop batch has
  224.50/225.92 ms fresh/empty medians and 169.20/169.95 ms typecheck medians;
  fresh allocation work is 1,827,343 calls / 217,055,736 bytes, 101,715 calls /
  2,005,949 bytes below checkpoint `408433b`. The FLUSH phase itself falls from
  about 6.4 to 2.25 ms. This remains directional rather than percentile
  acceptance; contextual schemes and checked inference remain the dominant
  measured owners.
- Checked-read dependency plans now borrow path segments from the immutable AST
  while they are built, allocate a canonical path only for unresolved reads,
  and validate indexed read lookups against that same AST instead of retaining
  a second owned segment vector per read. Declaration invalidation shares the
  authoritative reader table instead of cloning it. Contextual parameter setup
  consumes the sorted selector index and pattern domains in place, visits
  actuals and mutable user signatures without intermediate vectors, and avoids
  invalidation entries at the immediately-reset scratch-cache boundary. The
  final reverse flow graph is filled directly rather than materializing and
  copying a transient 29,812-edge tuple buffer. The complete 80-test suite,
  seed-free/full-sweep audit, exact solver counters, and checked digest pass. A
  six-sample alternating release edit-loop batch has 220.22/221.15 ms
  fresh/empty medians and 165.15/165.51 ms typecheck medians; fresh allocation
  work is 1,791,466 calls / 213,748,071 bytes, 35,877 calls / 3,307,665 bytes
  below the preceding FLUSH checkpoint. Empty-session allocation work falls by
  the same amounts to 1,791,472 calls / 213,812,668 bytes. A traced release
  sample attributes about 8.18 ms to dependency/setup inside the 28.31 ms
  contextual-scheme phase and 43.73 ms to checked inference, so the owned
  database and larger contextual/inference work reduction remain open.
- Signature-to-declaration synchronization now publishes directly through the
  authoritative dense declaration index instead of constructing complete
  ordered parameter/callable snapshots and a declaration-update vector. It
  retains the original complete-registry synchronization points and exact
  invalidation semantics. The complete 80-test suite and exact NovyWave digest
  pass. Fresh allocation work is 1,789,024 calls / 212,744,182 bytes, 2,442
  calls / 1,003,889 bytes below the preceding dependency-plan checkpoint;
  empty-session is 1,789,030 calls / 212,808,779 bytes. A release trace reduces
  the user-scheme signature synchronization from about 0.78 to 0.40 ms, but a
  six-sample confirmation batch is timing-neutral/noisy at 222.34/219.94 ms
  fresh/empty and 166.27/165.14 ms typecheck, so this is an allocation and
  ownership result rather than a latency claim. A narrower changed-signature
  publication plus PASSED dependency-cone experiment was rejected after it
  changed the checked digest. The complete sync currently repairs callable
  declarations also written by ordinary structural/result lanes.
- The rejected joint experiment is now decomposed at that ownership boundary.
  Generic declaration writes journal the exact callable-signature indices they
  disturb, while each existing synchronization point contributes only the
  signatures whose parameters or results it changed. The same sorted boundary
  order then republishes the union directly through the dense declaration
  index; no synchronization point or mutation order moved. The temporary
  pre-change 35.4 MB NovyWave tuple oracle, the complete 80-test suite, and the
  exact checked digest all pass. Fresh allocation work falls to 1,788,315 calls
  / 212,679,422 bytes and empty-session to 1,788,321 / 212,744,019, exactly 709
  calls / 64,760 bytes below the complete-registry checkpoint in each mode. A
  six-pair alternating release batch is still bimodal at 225.75/231.58 ms
  fresh/empty medians and 169.80/173.69 ms typecheck medians, so this is an exact
  repair-ownership and allocation result, not a latency claim. The PASSED cone
  remains unimplemented: remove the duplicate generic callable-declaration
  mutation owner, then retry the contextual cone independently against the
  exact oracle.
- Callable publication now has an explicit signature-owned API, while the
  structural, solver, and final value-declaration lanes reject callable IDs
  instead of temporarily replacing a function type with its raw result. The
  dirty journal remains a fail-safe for other generic declaration writers.
  With that duplicate owner removed, the independently retried PASSED worklist
  follows the exact reverse callee-to-caller cone rooted at lexical PASSED reads
  and no longer visits context-free user signatures. NovyWave worklist visits
  fall from 504 to 369 while all 369 real changes, the pre-change tuple oracle,
  exact digest, and complete 80-test suite remain unchanged. Fresh allocation
  work is 1,787,633 calls / 212,614,838 bytes and empty-session is 1,787,639 /
  212,679,435, another 682 calls / 64,584 bytes below the dirty-journal
  checkpoint in each mode. A six-pair release batch has 219.70/219.39 ms
  fresh/empty medians and 164.18/164.53 ms typecheck medians; one slow outlier in
  each mode keeps this directional rather than acceptance evidence. The traced
  context phase still takes about 10.52 ms because it constructs complete
  ordered call maps before pruning the worklist, and checked inference still
  takes 35 rounds. The next contextual slice must move that dependency cone to
  a compact indexed owner rather than polishing the now-cheap no-op visits.
- Expression ownership is now projected once after signature registration into
  dense signature ordinals plus compact per-signature expression/root slices.
  This replaces the initialization-time HashMap that cloned and hashed an owner
  `String` for every owned expression, repeated name resolution in calls,
  PASSED inference, effects, and lowering, and per-owner BTree root discovery.
  PASSED projections borrow the immutable AST; leaf variables, requirements,
  recursion/queue membership, and formal lookup use dense or sorted indexes,
  while call-child deduplication stays inline for ordinary arities. The complete
  80-test suite, legacy owner/root oracle used during the cutover, fixed
  pre-change tuple oracle, and exact checked digest pass. Fresh NovyWave
  allocation work falls to 1,758,855 calls / 211,969,403 bytes and empty-session
  to 1,758,861 / 212,034,000, another 28,778 calls / 645,435 bytes below the
  contextual-cone checkpoint in each mode. A six-pair release batch has
  216.62/217.17 ms fresh/empty medians and 161.95/162.37 ms typecheck medians.
  A release trace attributes 0.72 ms to the new owner index and reduces context
  setup/work from about 10.52 to 8.22 ms (0.52 reads, 0.35 graph, 5.31 worklist,
  1.86 install); the checked builder is directionally 117.37 ms versus 123.45
  ms. The still-open dominant owners are about 9.08 ms of parameter schemes,
  5.17 ms of structural schemes, and 43.23 ms/35 rounds of checked inference.
- The cold checked solver now instantiates its 618 input-insensitive,
  fixed-product calls before the first whole-expression wave, while leaving all
  1,202 input-sensitive calls in the ordinary stable round order. This uses the
  generic call-plan sensitivity bit rather than function names or fixture
  knowledge. NovyWave inference falls from 35 to 34 rounds, 35,930 to 34,653
  expression visits, 1,032 to 690 callable visits, and 3,848 to 3,484 call
  visits; callee enqueues fall from 521 to 209. The fixed 35.4 MB pre-change
  tuple oracle, clean full-sweep audit, exact checked digest, and all 80 tests
  pass. Fresh/empty allocation work falls to 1,732,729/1,732,735 calls and
  210,213,894/210,278,491 bytes, 26,126 calls / 1,755,509 bytes below the dense
  owner checkpoint in each mode. A six-pair release batch has 214.35/212.82 ms
  total and 158.87/158.18 ms typecheck medians. The locked downstream release
  rebuild still takes 1m35s, independently confirming that the later measured
  crate/relink boundary remains open. This is another directional owner-level
  result, not fresh percentile or Phase 1 acceptance evidence.
- Item 7's owned construction is now complete inside `boon_typecheck`:
  `CheckedProgramDatabase` directly owns checked construction, exact ordered
  diagnostic projection, and final report assembly. The separate `Checker`,
  `CheckedProgramBuilder`, owned-input bundle, replay-input bundle, repeated
  named-value table owner, and post-seal external-environment deep clone are
  deleted. Compiler-proven unreachable recursive inference/report helpers are
  also deleted rather than retained as a second latent engine; the complete
  source diff is net 1,153 lines smaller and production plus test-target checks
  have no dead-code warnings. The fixed 35.4 MB tuple oracle, exact digest,
  clean audit, all 78 ordinary tests, and both product-scale ignored gates pass
  with every work counter unchanged. Fresh/empty allocation work is
  1,732,728/1,732,734 calls and 210,213,846/210,278,443 bytes, exactly one call
  and 48 bytes below the preceding checkpoint in each mode. Six alternating-
  pair release medians are 213.89/212.96 ms total and 158.70/157.43 ms
  typecheck. This is an ownership/deletion result rather than a new percentile
  claim. The locked release rebuild remains 1m35s even after the source cut,
  confirming that the later measured crate/downstream-relink boundary remains
  open.
- Checked inference reverse dependencies now use immutable packed offset/edge
  arrays instead of one `Vec` header for each of 154,585 possible base rows and
  one allocation for each of 26,425 populated rows. Sixteen construction
  columns collect 40,880 sorted/deduplicated base edges directly; the derived
  29,812-edge flow graph is packed the same way, and the construction-only
  pattern column is no longer retained in the final database. The bounded-row,
  empty-row, last-row, ordering, deduplication, and invalid-ID tests pass. All
  79 ordinary typechecker tests and both product-scale ignored gates pass; the
  exact checked digest and every inference/cache/replay work counter are
  unchanged. Fresh/empty allocation work falls to
  1,687,722/1,687,728 calls and 210,064,354/210,128,951 bytes, exactly 45,006
  calls and 149,492 bytes below the owned-database checkpoint in each mode. The
  release dependency allocation/fill/sort/flow path falls directionally from
  about 8.1 to 6.6 ms and parameter-scheme setup from about 9.08 to 7.64 ms. A
  six-pair release batch has 208.89/209.55 ms total and 152.86/153.76 ms
  typecheck medians, with 65,768/66,400 KiB maximum RSS. The rejected dense
  counting-scatter and inline-first-edge builders were slower in direct debug
  A/Bs and are not retained.
- Round-level tracing then exposed the actual inference tail: several generic
  aggregate types grow from roughly 60,000 to 121,000 debug characters while
  calls repeatedly replace only substitution evidence. Structural widening now
  keeps the existing shared list/object node and avoids cloning its complete
  field map when a partial merge makes no semantic change. A call must produce
  two consecutive input-triggered no-op/evidence-only visits before later input
  retries may coalesce; every coalesced call is obligatorily refreshed before
  the contextual-wrapper quiescence hook, and any newly visible result/output
  disables coalescing and returns to the ordinary solver. This is a generic,
  fail-closed worklist rule, not a NovyWave/function-name exemption.
- The exact checked digest remains
  `495ef4195e0be4869e431c4036b74b6d6397c4b28b7fe7a8a76c76400a7b7992`;
  all 82 ordinary typechecker tests and both product-scale ignored gates pass.
  NovyWave expression/declaration/call visits fall from 34,653/1,893/3,484 to
  34,502/1,876/3,388, input enqueues from 1,227 to 1,127, and no-op visits from
  957 to 893. Eighteen coalesced calls are refreshed exactly. Stable-round
  accounting rises from 34 to 36 because two fail-closed repair waves remain;
  measured work nevertheless falls: the release worklist is about 36.3 ms
  versus 39.6 ms and its call lane about 14.6 ms versus 17.4 ms.
- Fresh/empty allocation work is now 1,666,870/1,666,876 calls and
  208,381,392/208,445,989 bytes, exactly 20,852 calls and 1,682,962 bytes below
  the packed-graph checkpoint in each mode. A six-pair alternating release
  batch has 206.28/206.55 ms total and 150.21/150.89 ms typecheck medians; the
  maximum observed RSS is 65,932/66,352 KiB. Direct recursive publication was
  rejected because it changed the checked digest; per-call invalidation flushes
  preserved the digest but were slower; eager object fingerprints preserved the
  digest but raised the debug worklist to about 88 ms; and a static unused-type-
  variable classifier found no safe NovyWave edges. None is retained.
- The compact/canonical structural owner is now shared by the checked graph
  instead of duplicated in the typechecker. Substitution is a single
  copy-on-write traversal: it no longer performs a complete applicability scan
  at every recursive level, preserves every unchanged shared object/list/Tag
  node, and uses an eight-entry inline active-variable stack instead of one
  tree allocation per visited replacement. Structural widening likewise
  materializes only real list/object/Tag growth, preserves normalized field
  order without rebuilding its set on a no-op merge, and uses one allocation-
  free comparator that is byte-for-byte equivalent to the old formatted
  canonical Tag key. No eager whole-shape hash or NovyWave-specific cache is
  retained.
- The exact checked digest remains
  `495ef4195e0be4869e431c4036b74b6d6397c4b28b7fe7a8a76c76400a7b7992`.
  All seven checked-graph tests, all 85 ordinary typechecker tests, and both
  product-scale gates pass. Fresh/empty allocation work falls to
  1,621,578/1,621,584 calls and 206,521,350/206,585,947 bytes: exactly 45,292
  calls and 1,860,042 bytes below the preceding worklist checkpoint in each
  mode. An 18-pair directional release batch has 204.74/205.60 ms complete-
  diagnostics and 149.59/150.10 ms typecheck medians, 223.03/223.09 ms maxima,
  and 65,348/65,924 KiB maximum RSS. The release trace reports about 23.8 ms
  contextual schemes, 36.2 ms checked inference, 11.9 ms diagnostic projection,
  and 108.6 ms checked-program construction. Counter and physical TodoMVC
  remain directionally below their cold gates at 4.85 and 52.20 ms.
- The first contextual-scheme ownership slice now reuses the checked solver's
  packed user-call graph for PASSED dependency closure, dependency postorder,
  inherited-call merging, and caller rescheduling instead of rebuilding three
  ordered maps/sets and rescanning every call. Lexical PASSED reads use one
  packed declaration adjacency and a reused projection-order buffer instead of
  per-owner trees and cloned path strings. Structural statement children stay
  inline at ordinary arity and postorder values use the dense parser statement
  arena rather than a tree map. Exact DeclId/projection ordering, 369 worklist
  visits and changes, every checked-inference counter, and the checked digest
  remain unchanged. All 85 ordinary typechecker tests and both product-scale
  gates pass. Fresh/empty allocation work falls to
  1,613,479/1,613,485 calls and 206,406,282/206,470,879 bytes: exactly 8,099
  calls and 115,068 bytes below the compact-type checkpoint in each mode. A
  release trace reduces contextual graph reconstruction from about 0.28 to
  0.03 ms, but the remaining parameter/context-worklist/structural lanes are
  about 8.65/5.23/4.96 ms. An 18-pair directional batch is system-noisy at
  218.24/220.90 ms total and 159.53/161.72 ms typecheck medians, with
  235.28/237.20 ms maxima and 65,372/65,912 KiB maximum RSS. This is an exact
  allocation and ownership result, not a whole-run latency or Phase 1 exit
  claim.
- This diagnostics checkpoint does not close verified compilation. Direct
  NovyWave verified samples remain about 7.72/7.56 seconds fresh/empty and
  515,228/515,624 KiB, dominated by 6.93/6.80 seconds of semantic construction;
  the 1,000 ms and 384 MiB verified gates therefore remain red and must be
  attacked after the frontend tranche rather than hidden by the passing
  diagnostics result.
- A fresh high-level trace at checkpoint `c77dabc` makes the architectural
  multiplier explicit. NovyWave starts with 17,716 checked expressions, 1,820
  calls, and 663 callables, but produces about 45,000 semantic expressions,
  5,146 OUT call instances, 247,537 dependency records, 248,201 proof nodes,
  and 1,060,194 proof edges. The verified compile takes about 7.79 seconds and
  allocates about 2.99 GB cumulatively at 515,172 KiB peak RSS: semantic
  construction owns 6.98 seconds, including 3.86 seconds for the dependency
  manifest. Only 61 of 426 initially pure ordinary-call candidates are retained;
  357 candidates become `body_not_closed`, while open boundary types reject 71
  more definitions before dependency closure. This is graph multiplication,
  not a remaining typechecker-container problem.
- The first retained-definition/invocation-overlay candidate on 2026-08-03
  makes the architectural cut material without a fixture-specific path. Open
  parameter/result types and pure render constructors retain one semantic body;
  a dense occurrence table carries checked-call identity and constructor-local
  context ordinals, and executable lowering resolves those overlays against the
  enclosing callable frame. The dependency manifest records definition and
  occurrence facts directly. One release NovyWave sample in each cold mode
  completes in 4,415.27/4,455.01 ms fresh/empty at 317,844/318,428 KiB peak
  RSS, with 3,758.10/3,777.86 ms semantic time, 16,521 semantic graph nodes,
  and 10,923,253/10,923,259 allocation calls totaling about 1.552 GB. This is
  down from about 7.79 seconds, 515,172 KiB, about 45,000 semantic expressions,
  24.95 million allocations, and 2.99 GB allocated. The RSS gate is now green
  directionally, while the 1,000 ms time gate remains red.
- A traced sample attributes 2,368.00 ms of the remaining 3,799.82 ms semantic
  phase to dependency-manifest construction. The retained execution graph has
  16,417 expressions, while the manifest still materializes 159,612 dependency
  records and a 160,276-node/512,204-edge proof graph; coverage and dependency-
  graph digests alone cost about 354.35 and 653.11 ms. Document/backend lowering
  is about 409.57 ms, including about 75.25 ms for document construction. The
  next measured owner is therefore compact direct proof construction/sealing,
  not another typechecker container micro-edit.
- The first production V4 projection-proof slice confirms the structural
  opportunity without closing it. V3 rich records and coverage are now
  test-only; an independent V3 materializer reconstructs all V4 row/projection
  receipts, exact edges, SCCs, and owner digests. One directional optimized
  NovyWave sample falls from the preceding sealed-plan sample's 4,581.206 ms
  and 317,316 KiB to 3,977.806 ms and 247,092 KiB. Manifest time falls from
  2,321.269 to 1,807.287 ms, and its graph falls from 159,617 nodes/506,915
  edges to 14,518 nodes/43,714 edges. These are single-sample architectural
  measurements, not percentile acceptance. The remaining manifest still
  rescans completed checked, execution, and lowering graphs for about
  367.057, 471.067, and 269.516 ms, then spends 516.468 ms folding projection
  receipts. The next tranche must emit and seal shared receipts during those
  graph builders and delete the corresponding inventory passes; do not resume
  row-hash/container tuning. After that cut, profile and reduce unnecessary
  semantic demand rather than merely making exhaustive demand cheaper.
- A second whole-pipeline audit proves callbacks alone cannot pass: the
  4,052.379 ms/247,284 KiB directional sample spends 3,262.360 ms in semantics,
  and subtracting the entire 1,813.236 ms manifest still leaves about 2.24
  seconds. The later post-`c870358` adversarial audit corrects its initial
  single-owner-unit model: stable interface/definition shards, occurrence-owned
  invocation shards, and ephemeral link fixed points must feed one sealed
  semantic image. Rich component graphs become borrowed views or explicit
  debug materializers. Migrate complete shard/domain batches and delete the old
  graph/inventory owner in each batch; carry these exact boundaries across
  revisions and parallelize only graph-proven independent work. The architecture
  plan's post-`c870358` priority owns the detailed sequence.
- The dependency-bottom kernel is now a real production dependency rather than
  a plan-only crate seam. `boon_compilation_db` owns request revision/backdating,
  compact forward/reverse edges, SCC sealing, and implementation-root digests;
  semantic V4 deletes its duplicate SCC implementation and owner-by-projection
  scan. Four kernel tests and all 19 focused manifest tests pass. One fresh
  directional NovyWave sample is 4,011.485 ms at 250,416 KiB, including
  3,265.269 ms semantics, 1,771.603 ms manifest work, and 465.455 ms projection
  sealing. This saves only roughly 40--50 ms because the kernel still receives
  post-hoc exhaustive rows. Continue directly with finalized shard-row sealing and
  deletion of checked/execution inventories; do not count the crate split as
  the architectural exit.
- The first post-`c870358` interface-firewall cut registers dense projection
  IDs, rejects stale memo publication, and gives every callable a leaf public-
  shape node distinct from its implementation summary. A directional
  NovyWave run is still 3,961.669 ms/250,596 KiB, but the largest projection SCC
  falls from 4,296 nodes to 85 (15,181 nodes, 44,807 edges, 14,483 components
  total). This closes the graph-explosion prerequisite for exact cones and
  later two-worker scheduling, not the latency gate: checked/execution/lowering
  inventories still take 378/477/272 ms and receipt folding 502 ms. The two-job
  release rebuild remains 2m43s. Continue with finalized shard rows and delete
  those inventories; do not tune the now-small SCC kernel.
- The checked/execution owner-deletion checkpoint is now real but its first
  representation is a measured regression, not an optimization result. It is
  preserved by local commit `174eb4b`.
  `SemanticProgram` drops its production checked/execution owners, resource is
  the sole execution mutation window, Manifest V5 imports finalized handoffs,
  and the old checked/execution inventories are test-only. Architecture,
  owner-oracle, 19 focused manifest, minimal-manifest, and ignored NovyWave
  occurrence checks pass. One direct optimized NovyWave sample takes
  5,665.819 ms at 507,428 KiB, with 4,357.397 ms semantics, 1,142.939 ms image
  finalization, 1,534.308 ms manifest work, 18,656,831 allocations, and
  2,989,230,512 allocated bytes. The stable plan hash remains
  `890eff63ce7eff16c5597093179b6878fc8f8ed3e9f49555e73333d71d7bcb42`.
  Full stable projection keys and invocation-path vectors are cloned through
  row routes and 119,441 edges, while 78,336 legacy-domain rows remain. Treat
  this as the required boundary checkpoint allowed by optimization-loop step
  6. Do not polish maps, serializers, or the 156-node maximum SCC as the next
  task.
- The required post-`174eb4b` whole-pipeline audit is complete. Three
  independent read-only reviews of projection/image ownership, semantic demand,
  and artifact lifetime converge on one replacement rather than separate
  micro-optimizations. Replace V1 checked/execution handoffs, Manifest V5 import,
  and the rich-key request graph with one collision-checked dense owner/path/
  projection registry, typed row columns, and CSR relocation/edge arenas.
  Authored call-site identity comes from stable source structure, never owner-
  local ordinals; local-row, linked-projection, and dense-image fingerprints
  remain distinct. Move verified-intent demand immediately after complete
  checking, then build canonical definition variants once and carry compact
  occurrence frames through OUT, contextual semantics, proof, and all backend
  domains. Migrate OUT/resource/reactive/lowering/storage/view/memory into the
  same image, run distributed convergence over compact summaries rather than
  full re-elaboration, link ordinary code once across document/row/migration,
  and consume the result into `SealedRunnableMachine` with runtime indexes built
  once. The exact staging, deletion ledger, anti-facade tests, and counters are
  authoritative in the architecture plan's post-`174eb4b` decision. The first
  implementation batch must delete an existing V1/V5 scan or owner while
  carrying a real demanded definition/occurrence through the dense image; an
  interner-only patch, side table, wrapper, feature fallback, or crate re-export
  is not progress.
- The first dense V2/V6 spine is now a measured checkpoint candidate, not the
  tranche exit. Checked V2 owns stable keys once with dense routes and CSR
  relocations; execution V2 uses dense projections plus collision-checked
  parent-pointer invocation paths; Manifest V6 consumes their fixed digests and
  dense edges without rebuilding recursive key trees; and the compilation graph
  accepts dense registered projections. The V1/V5 public production schemas are
  gone. Snapshot-local authored-call identity survives both unrelated and
  identical earlier insertions by using a reverse duplicate ordinal. It is not
  yet the parser-owned structural identity required for currentness: raw source
  text/path and rich DTO payloads containing dense IDs and spans still enter
  snapshot receipts. Owner-local row ordinals are removed, CSR arithmetic and
  V6 count accounting fail closed, and receipt aggregation domains are now
  separated. One final two-job release rebuild takes 3m00s; its direct optimized NovyWave
  sample is 3,549.342 ms at 274,896 KiB with the unchanged plan hash. Semantic
  work is 2,480.719 ms; execution-image finalization falls to 375.894 ms,
  manifest work to 727.061 ms, and allocated bytes to 1,805,377,118. This is
  directional, not scored evidence. The two V2
  builders still scan finalized rich columns, OUT still creates 5,147 eager call
  instances before demand, the execution image still seals 49,283 rows, and
  Manifest V6 still folds 78,336 legacy rows. Resource/reactive/lowering/
  storage/view/memory owners and canonical core also remain. Continue directly
  with parser structural occurrence routes, typed normalized row payloads, and
  verified intent before OUT/contextual expansion. Carry one demanded
  definition body plus compact occurrence frames and delete the corresponding
  scan/rich owner in the same slice. Do not optimize the new maps or declare
  Phase 1 complete.
- The post-`9540262` multiplier audit is now authoritative for the next cut.
  Keep deterministic snapshot routes, session-only syntax lineage, public
  semantic/persistence identity, and revision-local dense IDs as four separate
  planes; conservative warm misses are allowed, false reuse is not. Even free
  image finalization plus Manifest V6 would leave about 2.45 seconds, so the
  next flag-day slice combines verified-intent demand, canonical definition
  specializations, compact invocation frames, demanded OUT/contextual topology,
  and typed normalized row emission. It must delete a recursive OUT/contextual
  owner and its post-hoc scanner and materially reduce NovyWave call instances
  and execution rows. Direct proof sealing, the shared plan-code linker,
  persistent red/green currentness, compact bundle linking, at-most-two-worker
  scheduling, and low-fanout crate extraction follow in that order. Do not
  begin with crate splitting, generic query machinery, or parallel eager work.
- The first demanded-definition working-tree cut is semantically valid but is
  not the tranche exit. It computes 312 retainable definitions once and keeps
  concrete chains only for the 190 definitions that reach call-local render
  contexts. NovyWave OUT calls fall from 5,147 to 3,494, execution rows from
  49,283 to 47,296, legacy rows from 78,336 to 73,162, and projection edges
  from 119,671 to 82,364. Two direct release runs are 3,451.075/3,462.779 ms at
  271,128/270,692 KiB with deterministic plan hash
  `db18f345676378b8633829c0bbd7870c0a1dc5a2459649c9bbfdd6b8969374ab`;
  the retained-versus-flat NovyWave contract/persistence oracle passes. An
  unsafe 1,251-call prototype was rejected by the full backend because it
  erased `Scene/Element/text` context ancestry; the sparse overlay is the
  corrected contract. Do not tune its maps or the remaining three prunable
  lexical calls. `VerifiedSemanticIntentV1` now publishes every planned root
  kind before OUT, validates its checked identities, supplies OUT's exact
  program schedule, and shares the retained-definition set with OUT and
  contextual expansion. The next cut must make the non-schedule intent kinds
  drive normalized rows and relocations during construction and delete the
  post-hoc execution-image scanner plus Manifest re-import owner. Then link
  ordinary/render code once across backend domains. Current edit-loop evidence
  is directional only; no cold phase or acceptance gate is closed.
- The first construction-owned domain now removes production lowering replay
  and bumps the proof contract to Manifest V7. Lowering emits 36,979 normalized
  rows at its metadata/output/host construction stages; V7 seals them under an
  explicit lowering namespace, leaving 36,183 legacy rows. The focused debug
  NovyWave oracle preserves the 11,608-node/82,364-edge topology and passes.
  Committing the already sealed metadata/contract digests eliminates two
  duplicate aggregate serializations (about 434 ms in that debug trace), but
  fine-grained metadata-row generation still costs about 737 ms. The resulting
  higher-level audit exposed and the next flag-day cut deletes that larger
  bridge: `SemanticLoweringContractV2` deletes the full expression/function
  inventories, and `CanonicalProgramCoreV2` deletes all three full checked
  tables from the runnable core. Remaining lowering named-value metadata is
  projected into a narrow transitional interface rather than reconstructed as
  a checked table. Distributed reads carry exact executable-expression identities,
  export discovery consumes narrow named-value interfaces, and remote callable
  contracts come from exact producer materializations, including result and
  effect ownership even when there is no local ordinary-call entry. A focused
  three-role value/call regression and architecture gate pass without a global
  table fallback. On fresh debug NovyWave evidence, construction rows fall from
  36,979 to 1,885, metadata generation falls from about 736.6 to 120.7 ms, and
  the graph falls to 10,640 nodes/80,698 edges; the focused run is 12.67 s and
  remains far outside acceptance. Continue by making diagnostic maps optional,
  folding the remaining named-value interface into typed storage/interface
  ownership, and deleting the resource/reactive/storage/view/memory replay
  scanners as their construction-owned table/CSR spans become directly sealed.
  Do not optimize the 1,885 transitional rows or retain a compatibility adapter.
- The first reactive ownership cut reuses the exact canonical/local binding
  resolution already emitted by read construction and deletes trigger-time
  lexical/owner/call-ancestry rediscovery. A build-local trigger-plan index
  materializes each exact `(root, terminal)` subplan once, rejects cycles, and
  cannot survive a revision because its keys are dense image IDs. The exact
  ignored NovyWave oracle passes; two samples put state-update-arm construction
  at 295.4--309.5 ms, down from about 962.0 ms, and the full reactive phase at
  496.9--513.2 ms, down from about 1,172.9 ms. This is directional debug
  evidence, not a scored gate. Do not tune the residual cache: execution-image
  finalization and Manifest remain about 1,824.5--1,848.1 and
  1,695.0--1,714.8 ms. Delete those adjacent scan/re-import owners next. If
  reactive work later dominates, replace recursive trigger expansion with a
  normalized dependency graph, SCC/worklist publication, and shared immutable
  arm spans rather than more per-expression memo layers.
- The resource phase now follows the same construction-owned publication rule.
  Execution is immutable before resource construction, the resource table is
  the sole materialization source/target/predecessor owner, and production
  resource construction publishes 735 typed dependency rows directly. In the
  focused debug NovyWave trace, Manifest resource ingestion is 2.688 ms and
  only 35,448 legacy replay rows remain; the exact ignored oracle and
  architecture gate pass. This is an ownership checkpoint, not a speed-gate
  result: resource derivation is still 604.621 ms, execution-image finalization
  is 1,829.237 ms, and the remaining Manifest is 1,701.115 ms. The next tranche
  must replace the post-hoc execution scanner with definition receipts plus
  compact invocation overlays and construction-owned relocations. Reusing a
  serialization buffer or tuning the new resource rows is explicitly not the
  architectural exit.
- The post-resource execution audit now quantifies the next flag-day owner.
  NovyWave's execution handoff mirrors 47,296 rows and routes after construction
  even though 11,257 of 16,525 execution expressions are stable-definition
  routed and only 5,268 are invocation routed. Expression/origin mirroring
  costs 890.791 ms and final projection/whole-handoff sealing costs 340.185 ms
  of a 1,869.164 ms phase. Replace the mirror with checked-definition receipts,
  compact invocation overlays, split definition/occurrence provenance, and
  final executable row receipts emitted at construction. Then use one compact
  summary/relocation linker across domain seals. `CompilerSession` must retain
  immutable unit and request results; clearing the whole checked slot on every
  update cannot satisfy any warm gate. See the architecture plan's fifth audit
  for the schema and flag-day order.
- Do not preserve V3's 208k subject cardinality as a production V4/V5 proof
  requirement. The independent test oracle must map every historical subject
  to exactly one canonical finalized shard row and classifier field/domain and prove
  projection commitments, mutation detection, and exact dependency cones.
  Production fingerprints each actual database row once, binding all of its
  fields, and stores one typed dependency span. A one-receipt-per-historical-
  child-field design would encode the obsolete rich DTO inventory inside the
  replacement architecture and is rejected.
- The current artifact-oracle gate is red and blocks phase exit, but restoring
  the budgeted NovyWave hash would be a correctness regression. Fresh and empty
  modes deterministically emit
  `f293e8a8ef44c773740f769df19c9c08da717e31fb43ccbd96510396ef6594d6`,
  rather than the historical NovyWave hash
  `4d3c284a9240cdc68c70aff7f30c570367e285cc1e8f823585900829bafd8ff7`.
  The historical artifact's persistence type for
  `store.selected_value_column_width_key` omits reachable `Widest`; the current
  and `c77dabc`-like artifacts include it and have byte-identical persistence
  sections. The candidate document has 33,910 expressions, 442 constants,
  1,444 templates, 2,344 initial patches, and 2,430 row expressions versus the
  historical artifact's 42,099/450/1,472/17,517/2,454. Those cardinality
  differences require behavior proof, but they do not make the unsound artifact
  authoritative. Budget format V2 remains intentionally red until the
  controlled oracle migration below lands.
- Return to the remaining Phase 1 name/type interning, scaling/parity evidence,
  and fresh adversarial review after this architectural verified-compile cut,
  then regenerate the full cold protocol from the final Phase 1 source state.

Development profiles and focused debug tests remain directional tools. The
acceptance producer remains the revision-identified `release` binary required
by the budget manifest; a faster Rust profile cannot be relabeled as cold
compiler evidence.

#### Post-Checkpoint Architecture Reassessment (2026-08-03)

The retained-definition and activation/effect checkpoints changed the priority
order. The findings below remain architectural constraints and historical
evidence. The newer post-`32bcf40` sequence in
[`BOON_COMPILER_ARCHITECTURE_REFACTOR_PLAN.md`](BOON_COMPILER_ARCHITECTURE_REFACTOR_PLAN.md)
supersedes this subsection's numbering: preserve the landed exact activation
boundary, finish its full NovyWave oracle, then replace the production
entity-level proof and the future warm invalidation graph with one typed
`CompilationDb`. Carry retained ordinary definitions through the plan rather
than re-expanding them in the backend. Do not return to isolated container or
hashing micro-edits while a higher current tranche is incomplete.

1. **Repair the artifact oracle before accepting another plan hash.** Add a
   test-only flat/specialized semantic expansion that can project the retained
   representation into an independent behavior oracle. Compare ordered
   outputs, executable document behavior, row expressions, source routes,
   effects, list indexes, storage, commit/delta/demand/dirty semantics, and
   exact persistence/migration identity. Internal arena IDs are normalized to
   structural expression identity; duplicated dependency/state-update counts
   are performance telemetry while their zero-unresolved invariants remain
   exact. Retain deterministic full-artifact hashes as revision-local evidence,
   not as the only semantic comparison. A controlled oracle migration records
   the source digest, old and new artifact hashes, changed sections, reason,
   flat-oracle differential result, plan-verifier result, migration/restart
   tests, and focused negative cases. Budget format V3 must carry those stable-
   contract digests and oracle provenance. It may replace V2's NovyWave hash
   only after that evidence passes; `f293e8a8...` is a candidate, not an
   accepted replacement merely because it is current.
   The first real-host authored NovyWave scenario exposed generic activation
   and host-causality defects rather than an example mismatch. Checkpoint
   `32bcf40` now returns and routes the exact initial activation turn, applies
   reset/activation persistence atomically, records and replays exact external
   effect outcomes, and prunes unleased producer-template work before host
   commitment. Focused persistence, effect-transcript, wasm-target,
   producer-pruning, and workspace checks pass. The complete real-host
   NovyWave migration/restart/provenance/negative matrix remains required. A
   store-local epoch is transport metadata; the differential oracle may
   normalize only that epoch while keeping authority, migration identity,
   sequence, and content comparisons exact.
2. **Carry retained definitions through demand-collected plan functions.**
   Collect reachable definition-plus-invocation-overlay requests from published
   roots. Store each ordinary executable body once in `MachinePlan` and encode
   parameter sources, type substitutions, PASSED values, owner/resource/effect
   coordinates, and render context in compact invocation frames. Do not
   reconstruct a specialized semantic tree or recompile each exact call into a
   fresh backend cache scope. The runtime executes verified plan functions and
   existing typed kernels; it does not gain an AST interpreter or flat
   fallback. The test-only flat oracle proves exact mapping and behavior.
3. **Replace the rich proof inventory with one typed request graph.** The
   current production path constructs and drops 159,612 rich dependency
   records, 208,930 coverage records, 512,204 edges, per-record vectors, cloned
   subjects, and entity-to-vector maps primarily to hash them. A typed
   `CompilationDb` request keyed by stable owner/projection owns dense semantic
   row receipts, a compact exact dependency span, input/result fingerprints,
   `changed_at`, `verified_at`, and work counters. This same graph schedules
   cold construction, folds V4 proof roots, and later owns warm currentness and
   backdating; production must not build a second incremental dependency graph.
   A test-only materializer/serializer view must reconstruct the exact current
   V3 proof and coverage records for adversarial parity tests; changing the
   public proof schema is a later explicitly versioned step, not a shortcut.
4. **Seal and fingerprint shared rows once in that database.** Build shared
   immutable indexes and stable row fingerprints while each semantic table is
   sealed.
   OutNet, resource, reactive, lowering, storage, semantic digest, proof, and
   backend consumers reference the same rows instead of rescanning or
   reserializing whole DTO trees. Borrow the architectural shape of rustc's
   definition-plus-concrete-instance collection and red/green query graph,
   Salsa's memoized dependency/backdating model, and ThinLTO's compact global
   summary index with demand-driven materialization; do not import any of them
   as a second Boon solver or semantic authority. Primary references:
   [rustc monomorphization collector](https://doc.rust-lang.org/beta/nightly-rustc/rustc_monomorphize/collector/index.html),
   [rustc incremental queries](https://rustc-dev-guide.rust-lang.org/queries/incremental-compilation-in-detail.html),
   [Salsa algorithm](https://salsa-rs.github.io/salsa/reference/algorithm.html),
   and [LLVM ThinLTO](https://clang.llvm.org/docs/ThinLTO.html).
5. **Invert compiler/runtime dependencies and relocate cross-layer contract
   identities.** The live normal-dependency graph has six direct compiler
   consumers: `boon_cli`, `boon_host_runtime`, `boon_native_playground`,
   `boon_phase0_baseline`, `boon_program_runtime`, and `boon_runtime`. Source
   loading, migration-scenario compilation, and convenience compile facades
   belong in thin outer adapters. Runtime execution must depend only on the
   verified plan, document, persistence, executor, and host contracts. On the
   current graph, removing those runtime-family compiler edges is predicted to
   reduce `boon_compiler`'s normal transitive rebuild closure from 16 crates to
   the four true outer producers (`boon_cli`, `boon_native_playground`,
   `boon_phase0_baseline`, and `xtask`); publish the measured before/after
   closure and wall time rather than assuming the prediction is realized.

   `boon_document_model` also owns `ProgramRole`, `ListId`, `SourceId`,
   `PlanStaticOwnerId`, `OwnerInstanceRoute`, and `SourceRouteToken`, causing
   plan, checked, and typecheck layers to depend on a UI/document crate. Move
   these stable execution identities to a small dependency-bottom contract
   crate with no compiler, plan, document, or runtime implementation
   dependency. The current document-model transitive closure is 31 crates; the
   proposed edge cut predicts 19. Treat that as a hypothesis to verify with
   `cargo metadata` before and after. This is a flag-day ownership move: no
   compatibility re-export or duplicate ID type remains.

   If the proof-index work confirms stable seams, move immutable semantic
   tables and proof sealing to `boon_semantic_model` and
   `boon_semantic_proof`, or equivalently named single-owner crates. A file
   split without a smaller affected set, clearer invalidation authority, or an
   immediately enabled optimization is not progress. These cuts improve Rust
   organization and iteration time; they count toward Boon's cold latency only
   when direct producer measurements also improve.
6. **Separate the generic behavior oracle from the native GPU product shell.**
   The current real-host oracle is compiled inside the roughly 34.6-kLOC native
   playground crate and therefore pulls editor, server, window, and WGPU code
   into every oracle rebuild. Extract a headless, product-faithful harness that
   owns persistent runtime activation, host effects, retained-document hit
   testing, authored actions, artifact replacement, migration, and restart,
   while depending on no compositor, window, renderer, or example-specific
   shortcut. Native GPU handoff reports remain the independent final evidence
   for presentation and input routing. The headless cut passes only when the
   same NovyWave authored trace and artifacts pass through the extracted
   harness and its measured rebuild closure is smaller. At checkpoint
   `37f874e`, the native playground's normal workspace closure contained 37
   crates. Focused oracle bodies take about 0.1--0.2 seconds after linking, but
   dependency-bottom changes observed 38--109 seconds of build/relink work and
   the first optimized feature-oracle build took 3m10s. Those are Rust
   iteration measurements, not Boon latency, and make this boundary a measured
   prerequisite rather than an organizational preference.
7. **Retain the same request graph as the compiler-session invalidation
   firewall.** `CompilerSession::apply_updates` currently sets the single
   project-wide `checked` slot to `None` after any changed unit. The warm path
   therefore preserves a last-good artifact but rechecks an undifferentiated
   project; it has no component interface digest, exact affected-component
   graph, backdating, or reusable current-result boundary.

   Retain the `CompilationDb` created for cold compilation across revisions.
   Give each stable source unit, declaration, callable, semantic component,
   proof obligation, and backend region separate implementation and public-
   semantic fingerprints. The public fingerprint includes exported names and
   normalized types, effects, OUT/resource/persistence contracts, and every
   other fact that can change a dependent result; it excludes private body
   representation. Resolve dependencies once into exact forward and reverse
   indexes. An edit dirties its changed components and the reverse cone of any
   changed public fingerprint. A recomputed component whose result fingerprint
   is unchanged is backdated so its dependents remain green. Record
   `changed_at`, `verified_at`, input/result fingerprints, and owning work
   counters rather than treating cache presence as currentness.

   Complete diagnostics still describe the entire current revision, and an
   executable artifact publishes only after every reachable current-revision
   component, exact dependency proof, and verifier result is current. A
   canceled or superseded revision publishes nothing and cannot borrow an
   unverified result. Compare every incremental result and diagnostic set with
   a clean full compile of the same revision. Prove private-body edits, public-
   surface edits, unrelated-unit edits, transitive invalidation, unchanged-
   result backdating, deletion/rename, error introduction/recovery, and
   cancellation races with exact recomputation counters. Only after this graph
   is explicit may bounded compiler work run in parallel; concurrency is not a
   substitute for removing whole-project invalidation.
8. **Separate durable row overlays from derived view materialization.** The
   current NovyWave plan publishes 24 list memories with 252 persisted row
   fields. Only three list schemas contain generated structural-authority
   fields, while 21 computed/static list schemas are still promoted to durable
   owners. `store.selected_signal_defaults` is the clearest mixed case: two
   row-local `HOLD` fields (`formatter` and `format_dropdown_state`) share a
   persistence schema with 20 computed authority fields, including waveform
   segments and cursor values backed by transient host results. The current
   serialized plan snapshot is about 64 MB. These figures are diagnosis, not a
   budget result, but they expose an ownership error larger than another map or
   allocator micro-optimization.

   Make list row-domain authority, durable indexed overlays, and computed row
   values separate compiler concepts. A pure derived view with no structural
   mutations, durable row state, migration edge, or durable-owner dependency
   must not enter persistence. A stateful derived view persists only its stable
   row identity plus touched indexed overlays, keyed by structural origin; it
   does not persist host-backed display fields. A host/source-owned row domain
   remains a complete materialized authority. The compiler must derive the
   durable-owner closure and row-domain causality once, emit a topologically
   ordered activation program, and let the executor consume that program
   without rediscovering ownership from operation presence. Prove the cut with
   before/after persistent-list, row-field, plan-byte, artifact-byte,
   activation-work, cold-time, and RSS measurements plus migration/restart
   parity.

   The first product-oracle failure after pruning the computed fields makes the
   boundary concrete. `store.variable_rows` is only
   `search_results |> List/chunk(size: 1)` and owns no `HOLD`, mutation, or
   migration state, yet the `load_default_file` turn attempted to serialize
   its `items` field and encountered a nested mapped-row handle. Do not make row
   handles serializable. Remove this pure view from the durable closure. The
   cause is also structural in the compiler: ordinary list materialization is
   represented by `PlanOpKind::DerivedValue`, while `List/chunk` is emitted as
   `PlanOpKind::ListProjection`; activation classification currently sees only
   the former, and the executor independently scans only derived
   materializations. Any fix that adds another projection-specific exception
   preserves the competing authorities and is rejected.

9. **Give all list topology one canonical dataflow owner and seal the plan
   once.** Introduce one compiler-owned list-dataflow table covering static
   literals, structural mutation authorities, maps, filters/retains, ordering,
   chunk/group projections, bounded pages, and host/effect-produced row
   domains. Each row records its source lists, row-identity transform,
   replayability, structural-authority class, indexed-overlay ownership, and
   activation dependencies. Persistence closure, activation ordering, list
   indexes, dirty propagation, demand, and executor setup consume that table;
   none infers list ownership from operation variants or initializer shape.
   Keep specialized runtime operators where they are useful, but make them
   lowerings of this one contract rather than alternative semantic owners.

   The same ownership cut applies to final plan construction. The current
   `refresh_typed_list_view_fingerprints` clones the complete `MachinePlan` so
   it can mutate row-expression fingerprints against an immutable snapshot,
   then performs a separate reachability compaction and validation pass. A
   historical 64,029,143-byte pretty-JSON NovyWave plan shows why this cannot
   remain the scaling shape: compact component sizing attributes about
   26.9 MB to `document`, 1.03 MB to `regions`, and only 243 KB to
   `persistence`. The snapshot is stale correctness evidence and must not be
   used as a budget artifact, but it proves that persistence pruning alone
   cannot fix plan construction or cloning cost. Replace the post-hoc clone and
   rewrite with a `MachinePlanBuilder::seal`-equivalent boundary that freezes
   shared expression/list tables, computes reachable fingerprints bottom-up
   once, compacts once, and returns the immutable verified plan. Measure peak
   live bytes, row-expression visits, fingerprint normalizations, plan bytes,
   and finalization time before and after.

The list distinction follows the useful part of incremental-database design
without turning Boon into a database language: a named pure view is a recipe,
an indexed view is retained runtime acceleration, and only an explicit
materialized authority is durable. Materialize documents the same separation
between ordinary views, in-memory indexes, and durable materialized views:
[views and materialized views](https://materialize.com/docs/concepts/views/)
and [arrangements](https://materialize.com/docs/get-started/arrangements/).
Boon's `HOLD`, hidden row identity, migration, and activation contracts remain
the semantic authority; this precedent only reinforces that a view name must
not automatically imply durable storage.

The post-`9540262` implementation order is therefore: preserve the landed
list-dataflow, durable-overlay, exact-activation, effect-transcript, and
headless-harness cuts; preserve and complete the real-host NovyWave oracle
before phase acceptance without delaying compiler work; preserve the dense
snapshot checkpoint while separating canonical snapshot, session lineage,
semantic/persistence, and dense identities; collect verified demand before occurrence expansion;
delete the remaining rich graph owners into that image; converge distributed
links over compact summaries; link plan code once across document, row/scalar,
and migration domains; and publish one sealed runnable image with runtime
indexes built once. Retain these same requests for warm revisions. Pull
measured dependency inversion only at stable seams that shorten those tranches.
Return to local optimization only when fresh counters show that no higher
architectural tranche dominates. Each crate cut must publish a before/after
dependency closure and build wall time, preserve artifact and diagnostic
parity, and immediately enable the next optimization.

The first oracle slice now exists behind the non-default
`test-flat-oracle` feature; ordinary compiler and runtime builds contain no
representation switch or flat fallback. It parses and checks once, then lowers
the same checked graph through retained definitions and the historical
occurrence-specialized semantic projection. Counter and a focused width-state
fixture pass exact stable-contract comparison plus identity-independent startup
and four-source-turn document/snapshot comparison. The width fixture proves
`Compact | Normal | Wide | Widest` in persistence. An explicit optimized
NovyWave preflight completes in 14.82 seconds and passes the stable-contract,
plan-verifier, and exact four-variant persistence comparison while producing
distinct full plan hashes. Raw list-index expression IDs and duplicated dirty/
commit counts differed as expected, so the gate now commits structural list-key
expressions and zero-unresolved invariants rather than representation offsets or
operation cardinalities. Standalone NovyWave startup still requires real host-
owned values; complete interaction comparison belongs to the later V3 host
scenario and must not manufacture defaults. Budget V2 therefore remains red:
the feature-gated oracle seam is implemented, but the recorded V3 migration,
real-host NovyWave trace, migration/restart matrix, and negative evidence are
not yet complete.

#### Whole-System Boundary Audit (`37f874e` Baseline, `32bcf40` Reconciliation)

This audit deliberately zooms out from the last hot loop. Most inventory rows
were measured at `37f874e`; the landed harness/activation boundary and current
closure are reconciled at `32bcf40`. These are architecture measurements, not
acceptance evidence, and every predicted edge cut must be remeasured after
implementation.

| Boundary | Current evidence | Architectural correction | Required proof |
| --- | --- | --- | --- |
| semantic cardinality | 17,716 checked expressions and 1,820 calls had expanded to about 45,000 semantic expressions, 5,146 OUT instances, 247,537 dependency records, and a 248,201-node/1,060,194-edge proof graph before retained definitions; the retained checkpoint reduces semantic expressions to 16,521 but the manifest still dominates | make retained definitions plus occurrence overlays canonical requests; collect only reachable retained plan definitions and compact invocation frames; build proof/currentness directly from shared row receipts | flat-oracle behavior, stable contracts, proof materializer parity, cardinality/work counters, cold time and RSS |
| live representations | `SemanticProgram` simultaneously retains checked input, execution/resource/reactive/lowering/view/storage/memory graphs, a dependency manifest, and canonical core data | definition and invocation shards finalize rows into one `SealedSemanticImage`; link fixed points retain compact summaries/relocations only; rich graphs become borrowed views or test/debug materializers | no second executable authority, exact digests and verifier result, zero retained superseded graph owners, lower retained bytes/allocations |
| source concentration | `boon_typecheck` is about 40.9 kLOC, `boon_semantic` 69.9 kLOC, `boon_compiler` 27.3 kLOC, `boon_plan` 20.8 kLOC, `boon_plan_executor` 44.4 kLOC, and native playground 34.6 kLOC; the largest files include the 37.2-kLOC executor machine, 36.9-kLOC typechecker root, 16.5-kLOC machine backend, and 16.5-kLOC plan root | split only around durable ownership: stable contract IDs, semantic model/proof, compiler adapters, activation, and headless behavior harness; internal modules alone do not satisfy the gate | smaller normal dependency closure and measured rebuild wall time, no compatibility facade, focused tests remain owned by the new boundary |
| Rust invalidation | normal transitive dependents are currently 31 for `boon_document_model`, 22 for `boon_checked`, 19 for `boon_semantic`, 16 for `boon_compiler`, 22 for `boon_plan`, 18 for `boon_plan_executor`, and 13 for `boon_runtime`; a semantic edit caused about a five-minute release-native rebuild and a document-model edit about 7m38s | move cross-layer IDs to a dependency-bottom contract crate and source/compiler adapters outward; split semantic proof only once its immutable table seam exists | `cargo metadata` closure before/after, one controlled touched-crate rebuild before/after, unchanged Boon producer artifacts and diagnostics |
| compiler invalidation | `CompilerSession` owns one optional whole-project checked result and `apply_updates` clears it for every changed unit; it cannot add/delete/rename units, and parser unit-local products are not retained | retain immutable parsed-unit snapshots plus the same interface/definition/invocation/link graph used by cold proof; separate public/body fingerprints, exact cones, backdating, atomic upsert/remove/rename, revision overlays, cancellation checkpoints, and latest-generation publication | clean-full parity for every revision; parsed/reused/recomputed/backdated counters; private/public/unrelated/transitive/add/delete/rename/error/cancellation races; warm latency/RSS gates |
| persistence activation | `32bcf40` lands the exact activation product, atomic reset/activation persistence, deterministic host transcript, and pre-commit pruning of unleased producer work; the complete real-host NovyWave migration/restart oracle is still red | finish the compiler-emitted authority topology and the bounded migration/restart/provenance/negative matrix without a second mount or replay authority | exact startup effects and turns, focused state-dependent and host-owned tests, real-host NovyWave migration/restart and negative cases |
| durable list ownership | the inspected NovyWave plan has 24 persistent list schemas and 252 persisted row fields; 21 schemas have no generated structural-authority fields, while `selected_signal_defaults` mixes two durable `HOLD` fields with 20 computed/host-backed authority fields; the serialized plan snapshot is about 64 MB | compute the minimal durable-owner closure; separate stable row-domain identity and sparse indexed overlays from computed view fields; omit pure derived views from persistence and emit ordered activation steps | before/after list/field/plan/artifact bytes and activation work, exact migration/restart parity, negative tests for missing origins and cyclic activation dependencies |
| list topology ownership | derived materializations, `ListProjection`/`List/chunk`, storage initializer shape, mutations, indexes, and executor reconstruction scans independently describe overlapping row domains; the real-host oracle tried to persist nested mapped rows in pure `store.variable_rows` | one canonical list-dataflow table with row-identity, replayability, authority, overlay, and dependency columns; all specialized operators lower from it | every list has exactly one topology row, no executor rediscovery, pure-view absence from persistence, projection/map/filter/chunk activation tests, NovyWave restart parity |
| plan sealing | typed-list fingerprint refresh clones the complete `MachinePlan` before a separate rewrite, compaction, and validation sequence; in the stale 64,029,143-byte JSON snapshot the compact document component is about 26.9 MB while persistence is about 243 KB | one mutable builder followed by one immutable seal: reachable postorder, shared fingerprints, compaction, and validation without a full-plan clone | peak live bytes, finalization time, expression visits, fingerprint parity, exact stable contract and behavior oracle |
| plan-code ownership | retained ordinary definitions reach IR, but document, row/scalar, and migration backends independently bind arguments and recursively lower the same roots per specialization/occurrence | one shared plan-code linker keyed by definition, execution domain, resolved layout, overlay/control shape, and capability contract; occurrences become dense invocation frames | old ordinary-call/cache scopes and recursive body-lowering owners reach zero; exact artifacts/behavior; no runtime AST or unresolved substitutions |
| artifact publication | `SealedMachinePlan` removes duplicate trusted verification, but plan construction still clones/compacts/hashes whole tables and `MachineTemplate::new_sealed` rebuilds a broad clone-heavy executor metadata graph per consumer | explicit output intents and one consuming `SealedRunnableMachine` builder carrying compact plan tables, dense runtime indexes built once, digest, verification receipt, and minimal provenance; untrusted deserialization verifies/builds indexes once | before/after retained bytes, plan clones, hash/serialization passes, trusted metadata rebuilds zero, runnable-index builds one per seal, forged/deserialized-plan rejection, unchanged runtime work |
| product behavior harness | `boon_behavior_harness`, `boon_local_host`, and retained document hit testing are outside the native shell; `32bcf40` routes startup effects and records/replays deterministic host outcomes, while full NovyWave closure remains incomplete | finish the single recorded/replayed real-host scenario and its migration/restart/provenance/negative matrix | exact startup effects and turn revisions, identical authored trace/artifacts, complete migration/restart/negative matrix, and no example-name or render shortcut |

The target representation lifetime is:

```text
one CompilationDb at revision zero and later revisions
  -> immutable source snapshots + interface/definition/invocation shards
  -> ephemeral compact link fixed point
  -> one SealedSemanticImage + proof/currentness receipt
  -> shared all-domain plan-code definitions + compact invocation frames
  -> one SealedRunnableMachine + explicit output projections
```

Construction-only adjacency, cloned proof subjects, diagnostic DTO trees, and
flat specialized semantic trees must not survive their owning parity gate.
Compiler/runtime service objects may retain immutable snapshots and indexes,
but runtime execution must never retain a compiler implementation dependency.

The architecture follows primary precedents selectively: rustc uses stable
query fingerprints and projection queries to firewall downstream invalidation,
and separately collects concrete monomorphization roots and uses; Salsa tracks
exact dependencies and backdates unchanged results; ThinLTO merges compact
module summaries and materializes optimization work on demand; TypeScript
builder programs and project references establish affected-file and stable
boundary products rather than repeatedly rebuilding one undifferentiated
program. These are ownership and invalidation precedents, not permission to
add a second semantic solver. Primary references: [rustc incremental
queries](https://rustc-dev-guide.rust-lang.org/queries/incremental-compilation-in-detail.html),
[rustc monomorphization collection](https://doc.rust-lang.org/beta/nightly-rustc/rustc_monomorphize/collector/index.html),
[Salsa algorithm](https://salsa-rs.github.io/salsa/reference/algorithm.html),
[LLVM ThinLTO](https://clang.llvm.org/docs/ThinLTO.html), [TypeScript builder
program](https://github.com/microsoft/TypeScript/wiki/Using-the-Compiler-API),
and [TypeScript project
references](https://www.typescriptlang.org/docs/handbook/project-references.html),
[Swift request evaluation](https://www.swift.org/blog/swift-5.2-released/), and
[Swift fine-grained dependency tracking](https://www.swift.org/blog/swift-5.3-released/).

Parallel compiler work may be introduced only after component ownership and
dependency cones are explicit, with one small shared worker budget and
deterministic publication. It is not a substitute for removing graph
multiplication or rebuild fan-out. Keep Cargo producers serialized and bounded
as specified by this plan.

For every architecture tranche, record: affected dependency closure; source,
binary, diagnostic, stable-contract, and artifact identities; relevant
cardinality/work/allocation counters; direct cold and warm timings; peak RSS;
focused parity/negative tests; and which obsolete owner was deleted. A tranche
that only moves code, or lowers neither a measured work owner nor rebuild
closure, is incomplete.

### Semantic Construction And Proof Boundary

The public executable spine remains:

```text
ParsedProgram
  -> CheckedProgram
  -> SemanticProgram
  -> ContractVerifiedProgram
  -> ErasedProgram
  -> MachinePlan
```

`ParsedSnapshot`, `CheckedSnapshot`, and `SemanticCore` may exist as private
compiler construction types. They are non-executable and cannot be accepted by
a backend or runtime. Only a sealed `SemanticProgram` with an exact callable-
dependency manifest may cross into verification, and only an accepted
`ContractVerifiedProgram` may cross into erasure and lowering.

Each ordinary callable definition is retained once. Contextual call sites use
small overlays that carry only scope-dependent bindings and effects; they do
not clone a complete body, checked tree, or proof graph. Static conditions are
evaluated and dead branches pruned before dependency-manifest, proof, and
backend graph construction.

Build the exact callable-dependency manifest once at semantic sealing. Give
components deterministic identities and Merkle-style parent identities so an
affected component can be resealed without rediscovering unrelated roots.
Distributed fixed-point discovery may operate on private semantic cores, but it
seals one complete `SemanticProgram` only after the fixed point stabilizes.

`boon_verify` remains the sole owner of canonical obligations, accepted proof
evidence, and `ContractVerifiedProgram`. Compiler invalidation may narrow work
only through a generic dependency rule whose result is equivalent to a full
verification. Cached proof evidence remains immutable, exactly keyed, and
revalidated according to the formal plan.

### Backend, Hashing, And Reports

- Lower directly from the verified compact representation without
  reconstructing source ownership or recursive semantic trees.
- Stream canonical hashing, plan serialization, and report serialization.
  Do not build a second full in-memory tree merely to hash or print it.
- Preserve deterministic artifact order and exact stable-contract identities.
  Full `MachinePlan` hashes must match across cold modes and repeated runs of
  one unchanged candidate, but an old whole-artifact hash is not allowed to
  override a proven source behavior or persistence type. Change an accepted
  fixture oracle only through the controlled V3 migration protocol above.
- Keep debug/provenance sidecars detachable and content-addressed. The runnable
  plan contains the compact identity required to locate them, not duplicated
  diagnostic trees.

### Compiler Service

Provide one compiler service with the following conceptual interface; exact
Rust ownership and error types follow the existing crate boundaries:

```text
open_project(SourceBundle) -> ProjectId
apply_update(ProjectId, UnitUpdate) -> Revision
request(ProjectId, Revision, CompileIntent, CancellationToken) -> CompileResult

CompileIntent = Diagnostics | VerifiedCheck | VerifiedPreview | Handoff
```

- `Diagnostics` produces complete checked diagnostics but no executable
  artifact.
- `VerifiedCheck` follows the mandatory semantic/proof boundary without
  requiring a preview or report sidecar.
- `VerifiedPreview` returns the verified runnable plan needed to replace the
  preview.
- `Handoff` additionally materializes requested reports, provenance, or
  distributable sidecars.

Editor feedback uses `Diagnostics`; `boon check` uses `VerifiedCheck`; preview
and run requests use `VerifiedPreview`; handoff/report commands use `Handoff`.
They share the same project revision and owned compiler database rather than
performing serial whole-project compilations.

Use one foreground compiler worker and at most one low-priority handoff/report
worker. Newer revisions cancel older foreground work. A preview keeps the last
verified runnable plan until its replacement verifies; invalid or canceled
source never replaces it. Cancellation checks occur at bounded worklist,
component, proof, and lowering boundaries sufficient to meet the 8 ms gate.

### Immutable Reuse

Add persistent content-addressed reuse only after both cold modes pass. Never
persist the mutable compiler session or runtime `Session`. Reusable artifacts
are immutable and exactly keyed by at least:

- compiler and artifact schema versions;
- complete source-bundle identity and relevant unit/component identities;
- language, verification, semantic-schema, and persistence-migration catalog
  digests;
- target/profile and compile intent;
- every solver, proof-policy, and backend option that can affect the result.

A missing, malformed, stale, partially written, or mismatched entry is rejected
closed and recomputed. Cache-hit measurements are reported separately and
never mixed into cold results.

## Optimization Loop And Harness Ladder

Repeat this loop until the plan's Clear End Condition passes:

1. Measure the same revision and producer. Record source digest, binary hash,
   intent, fixture, total/phase time, RSS, work counters, diagnostics digest,
   and artifact hash. Never optimize from wall time alone.
2. Select the dominant failing owner. State the repeated work or allocation,
   the generic architectural change, the correctness invariants, and the work
   counter expected to fall. Do not start several unrelated micro-optimizations
   in one measurement batch.
3. Implement one coherent owner-level slice. Hot-loop instrumentation must use
   allocation-free local/batched counters merged at phase boundaries rather
   than a shared `Cell`, lock, map lookup, or timer on every node visit.
4. Run the smallest unit/integration tests that cover the changed owner plus a
   deterministic malformed-source or negative-proof oracle where applicable.
   A broad workspace suite is a milestone check, not an edit-loop command.
5. Invoke the already-built producer directly for Counter and NovyWave, and
   add physical TodoMVC when the changed owner affects its path. Compare
   diagnostics and artifact hashes as well as time, RSS, and work.
6. Keep the change only when it preserves semantics and either materially
   reduces the targeted work/time/RSS or establishes a required measured
   ownership boundary. If the targeted counter does not fall, reassess the
   architecture instead of stacking another local patch on the same theory.
7. Before claiming a numbered phase exit, give one fresh-context read-only
   subagent the phase contract, live revision, exact diff/checkpoint range, and
   report paths. It must try to disprove completion and return an evidence-
   backed pass/fail checklist. Close every finding before the exit claim.
8. If a coherent checkpoint is authorized, commit its exact scope, then begin
   the next failing gate in the same goal run.

Use three harness levels so verification cost stays proportional to confidence:

- **Edit loop:** one direct debug or existing release sample per relevant
  intent/fixture, plus focused correctness tests. It is directional only and
  never produces acceptance evidence.
- **Milestone preflight:** build the current release `boon_cli` once with
  `cargo build --locked --release --jobs 2 -p boon_cli --bin boon_cli`, then run
  the xtask collectors with a small explicitly non-acceptance sample count.
  Fix failures before spending time on 30-sample reports.
- **Acceptance:** from one clean unchanged revision and current producer, run
  the manifest protocol of three setup plus 30 scored observations for all
  fixtures and both cold modes, followed by the complete interaction/scaling
  collector and `--check-existing` validation. Do this only for a candidate
  that passed preflight.

The concrete collector sequence is:

```bash
# Only when xtask itself changed or target/debug/xtask is missing.
cargo build --locked --jobs 2 -p xtask

# One release producer build for the coherent candidate.
cargo build --locked --release --jobs 2 -p boon_cli --bin boon_cli

# Fast cold preflight during Phases 1-3.
target/debug/xtask verify-compiler-performance \
  --report target/reports/compiler-performance/preflight-cold.json \
  --setup-samples 1 --scored-samples 5

# Add the interaction preflight when Phase 4 session/cancellation work exists.
target/debug/xtask verify-compiler-interactions \
  --report target/reports/compiler-performance/preflight-interactions.json \
  --setup-samples 1 --scored-samples 5

# Run each full collector only after its corresponding preflight passes.
target/debug/xtask verify-compiler-performance
target/debug/xtask verify-compiler-interactions
target/debug/xtask verify-compiler-performance --check-existing
target/debug/xtask verify-compiler-interactions --check-existing

# Phase 6 adds this manifest-backed closure after the three reviews exist.
target/debug/xtask verify-compiler-performance-closure --check-existing
```

Run each Cargo or collector command sequentially. During cold Phases 1-3, the
interaction collector's explicit missing session/native evidence remains red
and does not block work on the cold owner; do not repeatedly run it before its
implementation phase. A failing relevant preflight report is useful blocker
evidence and must not overwrite the default acceptance report. The one-sample
edit loop may call `boon_cli compiler-sample` directly; it does not call either
collector and does not claim a percentile.

The collectors must remain orchestration-only: they may start direct producer
processes, but they must never invoke Cargo. An unavailable sampling profiler
does not block work; deterministic phase timers, work/allocation counters, and
targeted trace modes must be sufficient to choose an owner. External profilers
are corroborating evidence when available, not an excuse to stop.

## Implementation Order

### Phase 0: Documentation And Measurement Contract

1. Reconcile this plan into `steps.md`, `GOAL_PROMPT.md`, the active recovery
   plan, and the owning older plans without changing current architecture docs
   to claim unimplemented behavior.
2. Add a compiler budget manifest and revision-identified JSON report schema.
3. Add deterministic phase timers, work/allocation counters, cancellation
   counters, source and artifact digests, tree fingerprint, binary hash, and
   peak-RSS capture.
4. Establish the two cache-disabled cold runners and the warm edit/switch
   runner using prebuilt binaries.

Exit: the report exposes all named phases and identities, rejects stale or
cache-enabled evidence, and either reproduces an accepted fixture artifact or
requires the controlled differential-oracle migration for an intentional
representation/correctness change. A known-invalid historical artifact cannot
become the exit merely because its bytes are reproducible.

Current implementation checkpoint (2026-08-02):

- `boon_parser` exposes deterministic profiled entrypoints whose counters
  measure attempted/parsed units, inspected bytes/tokens/symbols, statement and
  expression visits, rebasing, and validation work on both success and failure.
- `boon_typecheck::TypeCheckProfile` carries timer-free inference,
  scheme-worklist, cache/index, and diagnostic-replay counters with exact
  changed-plus-no-op and hit-plus-miss accounting invariants.
- The compiler preserves both complete counter sets through monolithic and
  staged compilation. Producer schema v3 and report schemas v4/v2 carry the
  counters without relabeling artifact cardinalities as work.
- Call-depth/call-site scaling is owned by actual inference call visits and
  source-unit scaling by actual parser unit attempts. Contextual/static and
  dependency-cone scaling retain their current owners until semantic/proof work
  counters land in their owning phases.
- Performance and interaction collection consume one explicitly prebuilt
  two-job release producer. They no longer launch nested Cargo builds, and fail
  when the producer is missing or older than a Rust/workspace build input.
- Directional evidence from the directly invoked debug producer measured
  Counter at about 15 ms and NovyWave diagnostics at about 744 ms (about 332 ms
  parse and 412 ms typecheck). This is diagnostic evidence, not release
  acceptance. It identifies repeated whole-program parser
  validation/assembly as the first measured optimization target.

Phase 0 is a rolling measurement contract, not permission to postpone measured
optimization until session code exists. The existing cold producer and exact
frontend counters are sufficient to begin the current Phase 1 tranche. Add
semantic/proof/backend counters with their owning phases and add session reuse,
in-flight cancellation, and supersession counters when those mechanisms exist.
Until then, their explicit missing-evidence fields remain red. The producer
still requires cryptographic source-to-binary cross-binding rather than only
build-input freshness, and current release reports remain required before the
corresponding exits; those gaps do not justify another documentation-only or
counter-only loop before changing the measured parser/typechecker owners.

### Current Resumption Point: Dense Demand-First Image And Sealed Runnable

Continue from local checkpoint `9540262`, which preserves `174eb4b`'s checked/
execution ownership boundary, `c870358`'s compilation database,
compact-proof/sealed-plan checkpoint `38e6541`, and
activation/effect checkpoint `32bcf40`, and follow the detailed sequence in
[`BOON_COMPILER_ARCHITECTURE_REFACTOR_PLAN.md`](BOON_COMPILER_ARCHITECTURE_REFACTOR_PLAN.md).
Do not return to the historical contextual-scheme micro-tranche while verified
semantic/proof multiplication, duplicate representation lifetimes, whole-
project invalidation, and rebuild fan-out dominate.

The post-`ac2b234` high-level audit makes the next tranche more precise than
"speed up finalization." The live pipeline gives the resource builder mutable
execution columns so it can synthesize inline list-authority rows and backpatch
materialization list/scope/lineage fields. It then copies those bindings into a
resource table, validates execution three times across the boundary, rescans and
hashes every execution row into an execution handoff, reimports the handoff and
all remaining rich graphs into Manifest V7, maps the graphs into a second
canonical core, and later rebuilds runnable indexes per trusted consumer.

Execute the phase-seal architecture in the refactor plan before further local
reactive/container work:

1. normalize inline list authorities during execution construction and publish
   immutable `ExecutionSealed` columns;
2. give the resource table exclusive ownership of materialization source/
   target row bindings and predecessor lineage, migrating all consumers and
   deleting the execution copies;
3. make execution/resource and then every remaining domain builder emit final
   typed rows, component digests, entity routes, and CSR relocations once;
4. link those compact seals directly, deleting the post-hoc execution handoff,
   rich-domain Manifest inventories, duplicate canonical-core mapping/hash, and
   superseded rich retained owners as their independent oracles pass;
5. follow with the shared all-domain plan-code linker and consuming
   `SealedRunnableMachine`, then retain those exact requests in the existing
   `CompilationDb` across `CompilerSession` revisions.

Do not count deleting only the third validation, reusing a hash buffer, packing
`DependencyCollector`, or moving files into crates as this tranche. Those may
fall out of the owner deletion but do not establish its exit. Split crates only
at the resulting one-way semantic-image and runnable-image seams, with measured
Rust rebuild closure; keep that evidence separate from Boon latency.

1. Preserve the landed canonical list-dataflow and sparse-overlay lifecycle.
   The headless `boon_behavior_harness`, `boon_local_host`, and retained
   document hit testing are now extracted from the native shell. Their normal
   forward closure is 28 workspace crates versus native playground's 38. This
   is an iteration boundary, not a Boon latency result.
2. Preserve checkpoint `32bcf40`'s exact activation turn, atomic reset/
   activation persistence, deterministic effect transcript, and pre-commit
   pruning. Preserve the real-host NovyWave migration/restart/provenance oracle
   while ownership changes and complete its remaining negatives before phase
   acceptance; normalize only the store-local epoch. V4 is already the
   production proof schema at checkpoint `38e6541`. The remaining oracle does
   not delay the active compiler architecture cut and is not permission to
   restore V3 production.
3. Preserve `9540262`'s dense checked/execution V2 and Manifest V6 snapshot
   boundary without mistaking its raw DTO receipts for currentness. Separate
   canonical snapshot routes, session syntax lineage, public semantic/
   persistence identity, and dense IDs. Normalize typed row payloads to stable
   references while the demand builder replaces the post-hoc scanner; do not
   tune the checkpoint containers or add a production compatibility adapter.
4. Retain immutable parser-owned unit snapshots with atomic upsert/remove/
   rename and exact cached project assembly. After complete checking, collect
   verified intents and demand canonical definition variants before OUT or
   contextual occurrence expansion. Carry arguments, substitutions, PASSED,
   OUT, owner/resource/effect/render bindings in compact invocation frames.
   Delete eager body cloning and the late backend demand owner as parity gates
   pass.
5. Migrate OUT/resource/reactive/lowering/storage/view/memory into the same
   image in dependency order. Manifest V7's construction-owned lowering rows
   are only the first transition edge: replace the lowering checked-type-table
   round trip with diagnostic source-map rows, typed interface rows, and the
   existing execution/storage type owners, then delete those DTOs and proof
   inventories. Delete each rich graph and the duplicate canonical-core mapping
   immediately after its independent source-driven omission/mutation oracle and
   final consumer move. Iterate distributed closure over compact summaries and
   relocations, never full semantic role re-elaboration. Give verification only
   borrowed proof views and a receipt after bundle link closure.
6. Carry ordinary executable definitions through one shared plan-code linker
   across document, row/scalar, and migration domains. Key verified variants by
   execution domain, resolved layout, overlay/control shape, and capability
   contract; encode occurrences as dense resolved frames. Delete all matching
   recursive root lowering and cache-scope owners without a flat fallback.
7. Replace full-plan clone/rewrite/compact/hash finalization and per-consumer
   executor metadata reconstruction with one consuming runnable-image builder.
   Normal compilation returns `SealedRunnableMachine`; explicit debug or
   serialization intents own extra products, and untrusted deserialization
   verifies/builds dense indexes exactly once. The builder and runnable seal
   are one tranche; a wrapper around the current completed plan is rejected.
   Reprofile after every coherent
   owner deletion and keep the 250/350/300/100 ms envelopes honest.
8. Retain these exact source/shard/link/proof/plan-code/runnable requests across
   revisions. Separate public and implementation currentness; require exact
   reverse cones, backdating, add/delete/rename, error recovery, worklist-level
   cancellation, atomic latest-generation publication, and clean-full parity.
   Use at most two deterministic workers only for graph-proven independent
   requests.
9. Pull compiler/runtime adapters out of runtime cores, migration tooling out
   of host core, and semantic/runnable model crates away from their builders
   only at stable one-way seams with measured closure/rebuild improvements. Do
   not accept a cosmetic split or compatibility re-export.
10. Return to a local container or hashing optimization only when a fresh trace
    proves it is the largest remaining owner. Complete the cold/warm protocol,
    migration/restart evidence, and three adversarial reviews before advancing
    to the later unified-goal phases.

Keep cold and warm Boon latency separate from Rust rebuild latency. A crate cut
must improve a measured dependency/rebuild boundary; only direct producer
measurements can claim Boon compiler speed. Full release acceptance remains a
milestone operation after focused debug/integration tests and bounded direct
preflight pass.

### Phase 1: Cold Parse And Type Core

1. Preserve the landed independent-unit parser and finish stable source-unit,
   source-revision, and declaration identities without restoring concatenated
   parsing.
2. Preserve the landed `boon_syntax` flag-day boundary and keep it free of
   parser implementation dependencies or compatibility exports.
3. Replace borrowed checker state and duplicate checked construction with the
   owned checked database.
4. Move the immutable checked-program model to `boon_checked` while keeping
   safe construction exclusive to successful checking.
5. Introduce compact interned terms, dense tables, dependency indexes, and one
   bounded solver/worklist path.
6. Delete superseded global sweeps, deep copies, parser semantic side channels,
   and backend name rediscovery as their replacements land.

Exit: both cold checked-diagnostics gates and their RSS/scaling gates pass with
complete unchanged diagnostics.

### Phase 2: Semantic Sealing And Verification

1. Store ordinary definitions once and add contextual overlays.
2. Add the test-only flat/specialized oracle and migrate invalid fixture
   oracles through the recorded differential protocol.
3. Collect reachable retained plan definitions plus compact invocation frames;
   prune static branches before instance/proof expansion and do not recompile
   each exact call into a fresh backend cache scope.
4. Emit row fingerprints and owner references during semantic construction,
   fold owner-local Merkle roots, and seal exact callable dependency closure on
   the compact cross-owner graph. Retain the exhaustive entity proof only as a
   test materializer during the controlled V3-to-V4 migration.
5. Use the same owner/projection request graph for canonical proof construction
   and revision currentness/backdating without weakening verification.

Exit: semantic/proof work counters scale within budget, exact dependency
closures, flat-oracle behavior, stable-contract/persistence identities, and
negative proof cases pass, and no unsealed artifact is executable.

### Phase 3: Backend, Hash, And Memory Closure

1. Isolate the large machine-plan backend from compiler service/session
   orchestration without weakening its verified input boundary.
2. Lower directly from compact verified tables.
3. Stream hashes and serialization and detach optional debug/report data.
4. Remove old recursive/duplicated representations after parity is proven.

Exit: both cold verified-runnable time and RSS gates pass for all fixtures, and
each fixture has deterministic revision-local artifact hashes plus a passing
accepted stable-contract/differential oracle. No fixture is pinned to a known
under-approximated historical artifact.

### Phase 4: Persistent Session And Editor Cutover

1. Replace the current project-wide optional checked slot with the compiler-
   service semantic-surface database: stable component identities, separate
   implementation/public-result fingerprints, exact forward/reverse dependency
   cones, `changed_at`/`verified_at` currentness, unchanged-result backdating,
   revisioned requests, and bounded cancellation.
2. Remove the fixed editor debounce as a correctness/scheduling boundary and
   eliminate the second preview compile.
3. Route diagnostics, checking, preview, run, and handoff through the one
   service while retaining the last verified preview. A current revision may
   reuse only green verified components and publishes only after all reachable
   diagnostics/proof/backend regions are current; clean full-compilation parity
   remains the oracle.

Exit: every warm latency, cancellation, latest-generation, and no-stale-
publication gate passes under edit and example-switch storms.

### Phase 5: Optional Artifact Reuse And Native Presentation

1. Add exactly keyed immutable artifact reuse and fail-closed corruption/stale
   entry handling.
2. Report cache hits independently from cold and warm compiler-session work.
3. Reconcile the compiler-completion and final-presented-frame measures with
   the native verifier; update the native budget only when the verifier proves
   the separate stages.

Exit: disabling or deleting the cache leaves every cold gate green, cache
negative cases pass, and the native verifier proves the loaded-switch
presentation budget without weakening its input or WGPU evidence.

### Phase 6: Final Acceptance

Run the complete protocol from one unchanged revision, audit the code for
duplicate compiler owners and compatibility paths, and then refresh every
downstream report invalidated by compiler changes. Old native, Wasm, proof,
packed, or product evidence is not relabeled as current.

Add a manifest-backed `verify-compiler-performance-closure` xtask command that
cross-checks the cold report, interaction/scaling report, three adversarial
review sidecars, budget identity, producer/source/worktree identities, and
their status without running Cargo or regenerating evidence. It fails closed on
any missing, stale, mismatched, malformed, or non-passing input.

After the reports pass, run the independent adversarial closure below. A review
finding reopens the owning phase. Any corrective tracked edit makes affected
compiler and downstream reports stale, so rerun them before repeating review.

## Measurement And Verification Protocol

- Build the optimized compiler once. Cargo/Rust build time is not compiler
  time. Run only one Cargo build or test suite at a time on the reference
  machine; repeat fixture measurements by invoking the built binary directly.
- Start one producer process for every observation in both cold modes. The
  `empty-session` producer constructs one new in-process `CompilerSession` and
  serves one request; setup samples never share allocator high-water state or
  process-local compiler state with scored samples.
- Record three unscored setup runs followed by 30 sequential scored runs for
  each cold mode and fixture. Do not flush the OS page cache.
- Report Linux process peak RSS in KiB, sampled immediately after the checked
  result or runnable `MachinePlan` exists and before plan-validation, pretty-
  serialization, hashing, or report allocation can raise the high-water mark.
- Report p50, p95, p99 where meaningful, maximum, peak RSS, phase work,
  allocations, cache status, cancellation latency, source digest, compiler
  revision, worktree fingerprint, binary hash, and artifact hash.
- Fail if the worktree fingerprint or binary hash differs from the revision
  named by the report, if a required phase is absent, if a cold run observes a
  compiler cache hit, or if samples were run concurrently.
- Use focused debug checks and small integration tests during implementation.
  Run optimized end-to-end measurements only for coherent milestone
  candidates; do not repeatedly rebuild all crates between local edits.
- Preserve complete malformed-source diagnostics, stable spans and identities,
  recursion rejection, exact dependency closure, proof failures, deterministic
  plans, migration identities, and last-good-preview behavior.
- For an intentional plan-representation change, compare the test-only flat
  oracle and exact stable-contract sections, record old/new artifact identities
  and source digest, and run plan verification plus migration/restart and
  focused negative cases before changing an accepted budget oracle. Ordinary
  repeat and cold-mode comparisons remain byte-for-byte.
- Add invalidation tests proving unrelated units, callables, semantic
  components, obligations, and backend regions are not recomputed.
- Add cancellation races at parse, constraint, semantic, manifest, proof, and
  lowering boundaries. A canceled generation publishes nothing.
- Compare fresh-process and empty-database results byte-for-byte and compare
  incremental results against a clean full compile of the same revision.

## Independent Adversarial Closure

Performance completion requires three fresh-context, read-only subagent reviews
with disjoint charters. Give each reviewer the current contracts, live `HEAD`,
budget manifest, report paths, and relevant source/tests, but not an implementer
summary that presupposes success:

1. **Implementation-completeness reviewer:** map every numbered non-optional
   implementation item, optimization, crate boundary, deletion, and harness
   obligation in this plan to live code and focused evidence. It must identify
   silent omissions, aliases/fallbacks, duplicated owners, cosmetic crate
   splits, planned indexes or reuse that are declared but not used, and old hot
   paths still reachable in production. An item may be marked unnecessary only
   when this plan explicitly makes it conditional and the reviewer cites the
   passing evidence that satisfies that condition.
2. **Measurement-integrity reviewer:** independently validate budget/report
   schemas, sample counts and ordering, cold cache state, compiler thread count,
   process isolation, producer/source/worktree/binary hashes, RSS scope,
   percentiles, deterministic diagnostics/artifacts, scaling-counter ownership,
   cancellation evidence, and `--check-existing` behavior. It must look for
   stale producers, warmed state, nested builds, concurrent samples, debug or
   custom profiles mislabeled as release, fixture shortcuts, and report fields
   derived from cardinalities instead of measured work.
3. **Semantic-and-architecture reviewer:** try to prove that speed came from
   skipped diagnostics, weakened verification, changed artifacts, fixture-
   specific branches, extra compiler concurrency, hidden caches, incomplete
   invalidation, stale publication, or a bypass around the verified artifact
   spine. It also checks that crate boundaries reduce the measured rebuild set
   or establish their claimed ownership/invalidation seam without exposing a
   forgeable executable product.

The primary agent coordinates all execution. Review subagents may inspect the
tree and existing reports concurrently, but they must not start Cargo, compiler
producers, collectors, or other resource-heavy commands independently. The
primary runs any requested command sequentially with the plan's two-job Cargo
limit and returns the exact output/report identity to all reviewers.

Each reviewer returns a machine-checkable checklist or bounded Markdown table
with `pass`, `fail`, or `not-applicable`, exact file/report references, and no
unsupported human-observation claims. All three must pass independently. A
majority vote, an implementer rebuttal without evidence, a passing time number
with missing implementation, or implemented architecture with failing time/RSS
does not close the plan. After all findings are fixed and all stale evidence is
regenerated, rerun the three reviews against the final unchanged revision.

Persist the final checklists as three bounded JSON sidecars under
`target/reports/compiler-performance/`. Each sidecar records its charter,
status, live revision, worktree fingerprint, reviewed producer/report hashes,
every checklist item, findings, and cited evidence paths. Extend the final
performance aggregate and `--check-existing` validation to require all three
passing, mutually distinct charters and reject missing, stale, malformed, or
pre-fix review sidecars. Machine-checking identity and completeness does not
turn a reviewer assertion into evidence; every pass still needs its cited live
code, test, counter, or report support.

## Clear End Condition

This plan is complete only when all of the following are true on one unchanged
revision:

- both cache-disabled cold modes pass every time, RSS, determinism, and scaling
  gate for Counter, physical TodoMVC, and NovyWave;
- all warm diagnostic, verified-preview, switch, presentation, cancellation,
  and latest-generation gates pass;
- the public verified-artifact spine and proof acceptance remain mandatory, and
  no internal snapshot/core or cached evidence can bypass them;
- one owned checked database and one compiler service replace the duplicate
  whole-project editor/preview paths;
- ordinary definitions are stored once, dead static branches create no later
  work, and unrelated dependency components are not recomputed;
- disabling all compiler caches leaves the cold gates green, while immutable
  cache corruption and mismatch cases fail closed;
- all three fixture plans are deterministic for the final unchanged revision;
  each passes its accepted stable-contract and flat-oracle differential gates,
  no accepted oracle is a known semantic under-approximation, all affected
  semantic/proof/compiler/migration tests pass, and fresh reports name the
  final worktree and binaries;
- superseded representations, global-sweep fallbacks, compatibility wrappers,
  duplicate compiler owners, and temporary fixture-specific diagnostics are
  deleted;
- downstream evidence invalidated by the compiler change is rerun rather than
  declared current by documentation.
- all three independent adversarial reviewers pass every applicable checklist
  item against that same unchanged revision, and every finding from an earlier
  review is closed with regenerated affected evidence.

Passing only a smoke test, increasing a timeout, warming a cache, running more
compilers concurrently, publishing partial diagnostics, skipping proof work,
or preserving the old path behind a fallback does not satisfy this plan.
Neither does ending a `/goal` run after an intermediate commit while any item
above is missing or failing.
