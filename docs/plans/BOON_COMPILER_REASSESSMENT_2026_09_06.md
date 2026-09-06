# Compiler Reassessment Evidence — 2026-09-06

Status: dated research and source-audit record, not performance acceptance or
an execution prompt. The active sequence is in
[the compiler architecture plan](BOON_COMPILER_TENS_OF_MILLISECONDS_ARCHITECTURE_PLAN.md).
This record preserves the findings outside ignored `target/` files so cleanup
or a future goal does not erase the reasoning behind the next cut.

## Identity and Confidence

- Audited clean source: `37a576b6048a7da1e6c48b846bd1130c98084897`,
  `compiler: consume one packed checked topology`.
- Branch at audit: `main`, 38 commits ahead of its local `origin/main` ref.
- Stored producer: Rust 1.97.1, commit
  `8bab26f4f68e0e26f0bb7960be334d5b520ea452`, LLVM 22.1.6,
  `x86_64-unknown-linux-gnu`, generic CPU, one compiler thread, release opt 3,
  16 codegen units, no cross-crate LTO. This is not a claim about the latest
  available Rust release.
- Reference hardware: Intel i7-9700K, eight physical cores, 12 MiB L3.
- Product uses exact Microsoft mimalloc 3.5.0, upstream
  `18b08671c9302247bfb682286e6bf3cc1773f801`, static Rust global allocator
  without libc override; no allocation instrumentation or preload.
- Product SHA-256:
  `d0ae5548ef5c1b98fadfb6843988e1bad34645286fc3da352e9379f037158b90`.
- Evidence-producer SHA-256:
  `d33390b320742b1d03f9188095fc5e4bf6482423f1cc8fefc75d80f2bd5d8cf3`.
- Both producer metadata records name dirty parent
  `25a354d4724b44b12c4bc28b150938c376652b0a`, not clean `37a576b6`.
  The binary hashes match the stored reports. This does not make those reports
  clean-HEAD acceptance. Rebuild and recollect before an implementation claim.

Local evidence under `target/reports/compiler-performance/`:

| File | SHA-256 | Interpretation |
| --- | --- | --- |
| `packed-topology-final-preflight.json` | `5222c7bd9c2f05bcce925006d3eaeded4b8a57798535f3a864ede592b0d87ee4` | one setup plus five scored samples, development/non-acceptance, status fail |
| `packed-topology-final-allocator.json` | `3c4b14990ac4a5584388f871cbb316b73756875d619a09c0b9de88799b6a9cdf` | one setup plus two scored samples; allocator comparison pass is not performance closure |
| `preflight-cold-m0.json` | `47811ab9cde9eea4b2f3d93c80f0ab8b214c785a9a4bf905ee5acc91a8496bc5` | earlier M0 comparison, not a controlled new A/B |

The old `compiler-interactions.json` uses source checkpoint `f41a07f2` and
one scored sample. It is not current warm-edit or in-flight cancellation
acceptance. Three new one-sample trace invocations during the read-only audit
used the prebuilt product; their timings below are directional and include
tracing overhead. They were not saved as acceptance reports.

## Stored Product Measurements

Milliseconds, p50 / p95. Both cold modes disable compiler caches; OS page cache
remains natural. A process is isolated per observation.

| Fixture | Fresh diagnostics | Fresh verified artifact | Empty-session diagnostics | Empty-session verified artifact |
| --- | ---: | ---: | ---: | ---: |
| Counter | 8.113 / 8.835 | 17.139 / 18.578 | 7.212 / 7.265 | 16.162 / 16.801 |
| Physical TodoMVC | 913.209 / 923.712 | 1,250.864 / 1,263.719 | 905.285 / 931.134 | 1,276.973 / 1,281.673 |
| NovyWave | 500.478 / 516.612 | 1,810.570 / 1,818.527 | 492.799 / 524.732 | 1,809.450 / 1,836.508 |

NovyWave evidence-lane allocations:

| Intent | Allocation calls | Cumulative allocated bytes |
| --- | ---: | ---: |
| Diagnostics | 2,670,369 | 369,292,006 |
| Verified artifact | 7,727,429 | 1,234,321,769 |

These are Rust global-allocator events on the compiler thread, not every
native allocation in the process. The product binary reports no counters;
zero product counters do not mean zero allocations. Cumulative bytes are not
simultaneously live bytes. NovyWave stored fresh-process peak RSS is about
149,416 KiB for diagnostics and 306,768 KiB for verified output; empty-session
peaks are about 176,140 and 359,976 KiB respectively.

The earlier M0 product report recorded approximately 512.580 ms diagnostics,
2,024.128 ms verified, and 13,956,187 verified allocation calls. Across these
checkpoints the recorded change is about -2.4% diagnostics time, -10.5%
verified time, and -44.6% verified allocation calls. Different checkpoint/run
conditions prevent attributing this as a controlled causal A/B. It does show
why allocation count alone is an inadequate progress gate.

The NovyWave plan digest remains
`e2d673e3116f03622731f695cf2fb598d3313077845013f776930d38cc2acbea`.

## Phase Attribution

Stored NovyWave verified phase medians, milliseconds:

| Phase | Time |
| --- | ---: |
| Parse | 62.522 |
| Typecheck, including checked publication | 670.928 |
| Semantic | 773.490 |
| Explicit-contract bootstrap verification | 0.050 |
| IR lower | 17.108 |
| IR validation | 6.926 |
| Backend | 205.681 |
| Plan validation | 66.253 |

These spans are non-overlapping, but independently computed medians need not
sum exactly to the total. Detailed timers below are nested within these spans.
Do not add them again. Diagnostics typechecking is about 428 ms; its product
does not perform all the verified-path publication work.

One NovyWave semantic/lowering trace reported:

- OUT 64.9 ms; contextual materialization 12.1 ms;
- execution derivation 123.0 ms, normalization 46.2 ms, finalization 39.5 ms;
- resource 22.0, reactive 43.8, lowering-contract 31.3, storage 36.7, view 8.4,
  and memory 4.1 ms;
- canonical core/receipts 218.9 ms, including canonical execution 146.7 ms
  and receipts 50.2 ms;
- dependency manifest 104.1 ms;
- backend 214.7 ms, including derived values 111.6 and document lowering
  61.0 ms.

The proof/dependency graph has 7,974 nodes, 28,484 edges, 7,651 components,
three cyclic components, and a largest component of 296 nodes. The backend
handles 31,872 document expressions, 304 functions, 1,444 templates, 1,430
variant demands, 280 shared functions, 1,150 inlined single-use variants, and
6,460 cache scopes. These are cardinalities, not direct work counters.

One NovyWave kernel trace reported component evaluation 183.9 ms and packed
finalization 153.0 ms. Receipt time 43.1 ms is INCLUDED in finalization. Input
construction 47.2 ms, projection 83.8 ms, checked row construction 52.4 ms,
metadata 21.9 ms, and SOURCE ABI projection 8.9 ms identify additional owners.
One TodoMVC trace reported component evaluation 743.9 ms and packed
finalization 34.5 ms, including 9.4 ms receipts. Solver work is therefore still
a major cost, especially for TodoMVC; it is not all compatibility overhead.

Reproduction entrypoints, after explicitly building a fresh producer:

```bash
BOON_KERNEL_TRACE=1 target/release/boon_cli compiler-sample examples/todo_mvc_physical/RUN.bn --intent verified --mode fresh-process
BOON_KERNEL_TRACE=1 target/release/boon_cli compiler-sample examples/novywave/RUN.bn --intent verified --mode fresh-process
BOON_SEMANTIC_TRACE=1 BOON_COMPILER_LOWER_TRACE=1 target/release/boon_cli compiler-sample examples/novywave/RUN.bn --intent verified --mode fresh-process
```

## Invocation Work Amplification

Stored diagnostics counters:

| Counter | TodoMVC | NovyWave |
| --- | ---: | ---: |
| Parsed expressions | 4,858 | 17,721 |
| Definition modules | 155 | 1,389 |
| TodoMVC residual modules / stored operations | 470 / 10,939 | not the comparison denominator |
| TodoMVC residual frames | 8,194 | not the comparison denominator |
| Linked logical operations | 282,393 | 59,057 |
| Solver activations | 620,555 | 102,150 |
| Direct-summary definition nodes / invoke nodes | 236 / 0 | 12,866 / 540 |

NovyWave verified activations are 105,122; do not mix intent counters. TodoMVC
creates 8,039 invocation frames and reuses 302. Its top five residual module
variants account for about 49.9% of logical operations:

| Audit-local owner ID | Module operations | Frames | Logical operations |
| --- | ---: | ---: | ---: |
| 91 | 199 | 179 | 35,621 |
| 115 | 168 | 179 | 30,072 |
| 127 | 160 | 179 | 28,640 |
| 103 | 152 | 179 | 27,208 |
| 80 | 110 | 175 | 19,250 |

`program.rs::add_residual_frame` already shares an `Arc<ComponentProgram>`;
physical byte sharing has landed. `owner.rs::instantiate_owner` keys reuse by
actual variable/mode identity and deliberately avoids reusing state-owning
invocations. Nested calls can instantiate more frames. Direct result summaries
have a conservative support predicate (`direct_result_summary_supported`);
pattern reads and several contextual/stateful cases are not supported.

The missing attribution is each ranked variant's source definition, first
summary rejection, distinct actual-variable tuples, and equivalent resolved
semantic input tuples. The audit does NOT establish that HOLD or pattern reads
cause the five ranked cases. Owner numbers are diagnostic evidence only, never
production specialization keys or fixture-specific branches.

The architectural hypothesis is reusable parametric type/requirement transfers,
separate from state/resource occurrence identity. Count summary-node evaluation
as well as residual activation: moving repeated work into summaries must not
make it disappear from telemetry.

## Source Findings and Corrections to Earlier Research

Paths below are relative to `crates/`; symbol names are more durable than line
numbers in the audited source.

| Finding | Source anchor at the audit | Consequence |
| --- | --- | --- |
| Prepared payloads move rather than clone | `boon_compiler/src/kernel_oracle.rs:1162` | preserve landed work; do not repeat the old cloning diagnosis |
| Rich `DefinitionArtifact` is test-gated; packed store owns production facts | `boon_compiler_kernel/src/owner.rs:3681`, `definition_code.rs:213` | finish consumers, not another packed sidecar |
| Public diagnostics convenience API uses lean checking | `boon_compiler/src/lib.rs:911` | earlier full-checked-route claim is stale |
| OUT ancestry counter is cumulative bookkeeping, not literal repeated traversal | `boon_semantic/src/out_net.rs:2194`, `parent` at 301 | do not claim 1.96 million current parent walks; local rich substitutions still exist |
| Publication claims still clone keys and use rich sets | `boon_compiler_kernel/src/link.rs:8760` and following publication code | real verified-only cleanup; bundle into complete checked-boundary deletion |
| Session clears whole checked/diagnostic slots; lean diagnostics retains presentation | `boon_compiler/src/session.rs:488`, `731`, `768` | one retained packed revision should serve demands |
| Adapter constructs new kernel sessions per request | `boon_compiler/src/kernel_oracle.rs:2431`, `2840` | existing kernel same-revision promotion is not retained by the facade |
| Kernel replacement clears reusable state | `boon_compiler_kernel/src/session.rs:739`; stronger-demand check at 791 | no exact inter-revision reuse yet |
| Definition demand finalizes project-wide stores before filtering | `boon_compiler_kernel/src/owner.rs:4504` | definition chunks must eliminate whole-project repacking for small edits |
| Native editor requests EditorDiagnostics, then preview | `boon_native_playground/src/compile.rs:540`, `598` | actual rich editor path can reuse checked data; do not claim every preview double-checks |
| Error path switches to EditorRich and ordering errors can recursively rerun checking | `boon_compiler/src/kernel_oracle.rs:2880`, `3052` | diagnose from retained facts; measure invalid and incomplete edits |
| Shared semantic function eligibility rejects effects/context/state and propagates rejections | `boon_semantic/src/contextual_expansion.rs:6298`, `6390` | shared code needs separate per-occurrence state/capture data |
| Backend function keys contain call/stable-owner IDs and cloned contextual maps | `boon_compiler/src/document_executable_backend.rs:113`, `1632` | separate code compatibility from instance identity, not merely delete key fields |
| Runtime-packed checked handoff still becomes checked fields | `boon_semantic/src/lib.rs:2807` | topology consolidation is not full rich-boundary removal |
| Plan seal still invokes full verifier | `boon_plan/src/lib.rs:10014` | replace duplicate rich verification with an independent packed audit, not no audit |
| WHERE is planned/rejected, and source obligation vectors are empty | `boon_syntax/src/lib.rs:361`, `boon_parser/src/lib.rs:7498`, `boon_verify/src/lib.rs:851` | bootstrap timing is not authored proof performance |

Only coarse phase-boundary cancellation is presently visible; the audit did
not find cooperative checks inside kernel/semantic work batches. A pre-canceled
request does not prove the eight-millisecond in-flight cancellation budget.

## Product and Harness Gaps

- Exact mimalloc is selected by CLI entrypoints, not the in-process native
  playground compiler. Benchmark and integrate the actual product separately.
- `Cargo.toml` sets `lto = false`, but `boon_cli/build.rs` hardcodes `lto=off`
  and 16 codegen units in profile identity. Cargo permits thin-local LTO with
  `false`; it is not equivalent to `"off"`. Configuration A/B needs truthful
  effective metadata and mismatch-negative tests first.
- Build identity incorporates HEAD/diff, so committing identical build inputs
  can make evidence stale. Keep provenance and build-input content identity
  distinct; never accept a genuinely changed source under an old binary hash.
- `budgets/compiler.toml` encodes NovyWave 250/1,000 ms and 512 MiB. Older prose
  says 384 MiB and the previous stretch plan says 128 MiB. Warm preview is
  100 ms in the manifest versus a 50 ms stretch proposal. These are different
  policy levels, not interchangeable passing gates.
- `verify-compiler-performance`, `verify-compiler-allocator`, and
  `verify-compiler-interactions` exist in `crates/xtask/src/main.rs`.
  `verify-compiler-performance-closure` is not registered or implemented there;
  the previous prompt treated a planned aggregate as available.
- Existing scaling dimensions need invocation-transfer and shared-stateful-code
  work counters; final artifact cardinalities alone cannot prove less work.
- `perf` was unavailable in PATH. Deterministic timers/counters remain enough
  to proceed; hardware profiling is useful corroboration, not a prerequisite
  for every architectural edit.

## Architecture Judgment and External Research

Keep Rust, the dense kernel, and the existing repository. Rewriting in another
language or repository would not itself remove repeated evaluation. Preserve
one logical identity/fact authority but allow immutable per-definition chunks,
stable session indexes, borrowed slices, and derived CSR indexes. Do not force
whole-project compaction to achieve a superficially single physical store.

Use references and IDs first, shared chunks at revision boundaries, and arenas
only at correct scratch lifetimes. Consider persistent containers for snapshot
indexes, not as blanket replacements for dense vectors. Small strings help
remaining boundary text; eliminating/interning repeated identity is stronger.
Bounded capacity growth is legitimate; an extra exact-sizing pass over every
input is not automatically faster.

Primary references used in the audit:

- [rust-analyzer architecture](https://rust-analyzer.github.io/book/contributing/architecture.html): immutable unit boundaries, body-insensitive indexes, malformed-source handling.
- [Salsa algorithm](https://salsa-rs.github.io/salsa/reference/algorithm.html) and [rustc incremental queries](https://rustc-dev-guide.rust-lang.org/queries/incremental-compilation.html): dependency validation and unchanged-result reuse; no requirement to adopt their implementation wholesale.
- [Cargo profiles](https://doc.rust-lang.org/cargo/reference/profiles.html), [rustc PGO](https://doc.rust-lang.org/rustc/profile-guided-optimization.html), [codegen options](https://doc.rust-lang.org/rustc/codegen-options/): separate Rust build time from Boon runtime; test cross-crate ThinLTO/native CPU early, representative PGO after architecture stabilizes.
- [allocator_api](https://doc.rust-lang.org/nightly/unstable-book/library-features/allocator-api.html) and [portable SIMD](https://doc.rust-lang.org/nightly/unstable-book/library-features/portable-simd.html): optional measured facilities, not automatic compiler speedups.
- [Bumpalo](https://github.com/fitzgen/bumpalo), [im](https://docs.rs/im/latest/im/), [compact_str](https://github.com/ParkMyCar/compact_str): different lifetime/layout tradeoffs; none justifies mechanical container replacement.
- [mimalloc 3.5.0](https://github.com/microsoft/mimalloc/tree/v3.5.0): exact selected allocator, not a claim about the newest release.
- [TigerBeetle architecture](https://github.com/tigerbeetle/tigerbeetle/blob/main/docs/ARCHITECTURE.md): bounded work, memory ownership and batching are useful lessons; arbitrary compiler inputs do not imply a literal fixed-memory/no-allocation guarantee.

Neither allocation elimination nor eight-core parallelism establishes a cold
40--85 ms compiler. Moving 1.81 seconds into that envelope requires roughly
21--45x improvement across several owners. Warm tens-of-milliseconds is a more
credible near-term route for small measured dependency cones. Both remain
unaccepted until measured at their actual product endpoints.
