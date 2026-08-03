# Boon Cold-First Compiler Performance Plan

Date: 2026-08-02

Status: authoritative blocking implementation contract for compiler latency,
memory, invalidation, cancellation, and compiler-service ownership.

Under the combined order in [`steps.md`](steps.md), this plan is implemented
before the remaining native-recovery exit and before later language, formal,
packed-runtime, console, product, or game work. Documentation reconciliation is
the first slice; passing the cold compiler gates is the first implementation
exit.

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

| Fixture | Package / compiler-input lines | Source to `MachinePlan` | Peak RSS | Plan SHA-256 |
| --- | ---: | ---: | ---: | --- |
| Counter | 140 / 140 | 0.09 s | 29,992 KiB | `dc1fe51b659d1746a0b0b4ae2dcba21d50a9426499eb2bde28dbed988e6cfb08` |
| Physical TodoMVC | 3,647 / 3,576 | 2.02 s | 146,340 KiB | `c9a12cd0a1bcf748a20e3a072afa09d0f923c2c9dbd664f2343d343494404f96` |
| NovyWave | 11,994 / 11,923 | 20.68 s | 1,000,416 KiB | `4d3c284a9240cdc68c70aff7f30c570367e285cc1e8f823585900829bafd8ff7` |

NovyWave's package count includes its separate 71-line `BUILD.bn`; the compiler
input count names the source bundle actually passed to the Client compiler.

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
the ordinary `release` producer. `boon_parser` currently remains at the default
unoptimized dev level even though parser latency is part of every interactive
compile. Perform one bounded A/B of a package-local dev `opt-level = 2` with
line tables; record its Rust rebuild cost and direct debug Counter/NovyWave
latency, and keep it only if it improves the development loop. This setting
cannot satisfy or weaken the release cold gates. Do not add LTO,
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
- Continue with compact/canonical structural type terms that avoid rebuilding
  the still-growing aggregate chain without eager whole-shape hashing, then the
  measured 24.3 ms contextual-scheme owner, remaining construction/diagnostic
  work, and measured name/type interning. Finish scaling/parity evidence and a
  fresh adversarial review before regenerating the full cold protocol after the
  final Phase 1 edit.

Development profiles and focused debug tests remain directional tools. The
acceptance producer remains the revision-identified `release` binary required
by the budget manifest; a faster Rust profile cannot be relabeled as cold
compiler evidence.

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
- Preserve deterministic artifact order and the three baseline `MachinePlan`
  hashes throughout this semantics-preserving plan. Any intentional future
  format change belongs to its owning plan and must update this invariant
  explicitly.
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
cache-enabled evidence, and reproduces the current fixture artifacts.

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

### Current Resumption Point: Phase 1 Frontend Tranche

At the structured-counter checkpoint, perform this tranche before general
compiler-service work or any later repository plan:

1. Build one current two-job release producer and run bounded direct samples
   for Counter and NovyWave diagnostics plus NovyWave verified output in both
   cold modes. Record phase/work/RSS and parity without running the full
   percentile protocol. Use this optimized baseline to confirm or replace the
   debug-profile hypothesis before changing a large owner.
2. Run the one-time `boon_parser` dev `opt-level = 2` A/B described above and
   inspect one Cargo timing for the frontend edit path. If full `boon_cli`
   relinking, rather than the changed frontend crate, dominates iteration, add
   one thin development-only frontend probe that calls the exact shared source-
   bundle/parser/typechecker entrypoints and emits the same work schema. It may
   not copy compiler logic, become a second compiler, or satisfy acceptance;
   keep it only when the measured rebuild/iteration loop improves.
3. If the release profile confirms the measured parser owner, replace repeated
   parser-wide lookup scans with one non-diagnostic validation index built
   without changing error order. Provide direct token-start lookup, per-line
   semantic-token ranges, literal/text spans, and the other indexes justified
   by measured validators. Preserve the existing validator sequence and
   malformed-source diagnostics.
4. Make project assembly reserve once, move/append unit-owned tables where
   ownership permits, and rebase each dense identity/span exactly once. Keep
   independent source units and the opaque `ParsedProgram` boundary; do not
   reintroduce concatenated-source parsing.
5. Batch parser work counters so measurement does not dominate the million-
   visit debug path. Prove counter totals and profiled/unprofiled result parity.
6. Replace recursive production diagnostic replay with deterministic projection
   from the finalized checked graph and ordered obligations. Retain the old
   recursive path only as a test oracle until unsorted diagnostics, constraints,
   render slots, deferred styles, and deferred-style diagnostics match exactly,
   then delete it from production.
7. Establish the owned checked database and extract the stable immutable
   checked boundary to `boon_checked` using the crate-split gates above. The
   semantic, verification, and IR crates must depend on the checked product,
   not on solver implementation details; no forgeable executable bypass or
   compatibility re-export may remain.
8. Reprofile after every owner-level slice and continue Phase 1 until Counter,
   physical TodoMVC, and NovyWave pass both cache-disabled cold diagnostics
   modes and their RSS/scaling gates. A crate checkpoint or a parser-only win
   does not permit moving to semantic work while this exit is red.

Current resumption after the first passing cold-diagnostics candidate: item 7's
single owned `CheckedProgramDatabase` is complete inside `boon_typecheck`; do
not recreate a checker/builder handoff or a latent recursive production engine.
Its hot reverse dependencies are now compact immutable offset/edge arrays; do
not restore fragmented row vectors or retain construction-only columns. The
checked worklist now preserves unchanged widened nodes and coalesces only after
two evidence-only input visits, with mandatory pre-hook refresh and fail-closed
solver repair; do not replace that boundary with order-dependent recursive
publication or per-call cache flushing. Next introduce compact/canonical type
terms for the growing aggregate chain, then reduce contextual/structural work
and complete measured name/type interning before the Phase 1 scaling,
malformed-source/parity, and fresh adversarial checks. Use
the unchanged 1m35s release rebuild after both the net source deletion and this
runtime win as evidence when evaluating the next ownership/dependency crate
boundary, but accept a split only under the measured split gates above.
Reprofile every owner-level slice and rerun the complete three-setup/30-scored
cold protocol after the final Phase 1 edit; checkpoint `677d09d`'s narrow
empty-session result cannot be reused across these source changes.

Once diagnostics pass, use the same loop for semantic component retention,
single manifest sealing, verified lowering, streaming/hash memory, and backend
isolation until the full verified-plan gates pass. Only then implement and
measure persistent session reuse, warm invalidation, switching, and bounded
cancellation.

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
2. Prune static branches before graph expansion.
3. Build indexed semantic components and seal the exact callable manifest once.
4. Integrate component invalidation with canonical proof construction and
   accepted evidence without weakening verification.

Exit: semantic/proof work counters scale within budget, exact dependency
closures and negative proof cases pass, and no unsealed artifact is executable.

### Phase 3: Backend, Hash, And Memory Closure

1. Isolate the large machine-plan backend from compiler service/session
   orchestration without weakening its verified input boundary.
2. Lower directly from compact verified tables.
3. Stream hashes and serialization and detach optional debug/report data.
4. Remove old recursive/duplicated representations after parity is proven.

Exit: both cold verified-runnable time and RSS gates pass for all fixtures, and
their plan hashes remain unchanged.

### Phase 4: Persistent Session And Editor Cutover

1. Implement the compiler-service interface, dependency-cone invalidation,
   revisioned requests, and bounded cancellation.
2. Remove the fixed editor debounce as a correctness/scheduling boundary and
   eliminate the second preview compile.
3. Route diagnostics, checking, preview, run, and handoff through the one
   service while retaining the last verified preview.

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
- the three baseline fixture plans retain their exact SHA-256 values, all
  affected semantic/proof/compiler/migration tests pass, and fresh reports name
  the final worktree and binaries;
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
