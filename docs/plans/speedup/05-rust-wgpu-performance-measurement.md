# Rust And WGPU Performance Measurement

Date: 2026-06-12

## Purpose

This file catalogs libraries, tools, and measurement rules for proper FPS,
frame-time, latency, CPU, GPU, and graphics-performance measurement in Rust and
WGPU applications. It is aimed at Boon and the NovyWave example, but the shape
applies to any Rust graphics app with a real event loop and GPU renderer.

This is not approval to add dependencies. Each profiler, crate, or external
tool should be chosen for a specific question, run on a representative
workload, and kept out of the normal interaction hot path unless its overhead
has been measured and accepted.

Local constraints:

- `docs/architecture/NATIVE_GPU_PIPELINE.md` remains the active native GPU
  contract.
- Release-mode reports are authoritative for speed gates. Debug profiles are
  useful for diagnosis, not acceptance.
- FPS alone is not a pass/fail metric. Use frame-time percentiles, missed
  frames, input-to-visible latency, stage timings, and tail latency.
- Proof-mode readback, PNG writes, hashing, and full report serialization must
  be excluded from normal interaction measurements unless that proof path is
  the thing being measured.
- A report is current evidence only when it records the current binary,
  worktree/config identity, backend, scenario, and sample count. Stale native
  GPU reports are historical hints, not proof.

## Measurement Principles

### Prefer Frame Time Over FPS

FPS hides stalls. A UI can average 60 FPS while still feeling bad if it has one
100 ms frame every few seconds. The primary report shape should be:

- `frame_time_ms_p50`;
- `frame_time_ms_p95`;
- `frame_time_ms_p99`;
- `frame_time_ms_max`;
- `missed_frame_count`;
- `sample_frame_count`;
- longest visible stall and the stage that caused it.

FPS can be derived for dashboards, but it should not replace frame-time
histograms. For Boon, a 60 Hz-class target means p95/p99 around 16.7 ms and a
bounded max around 33.4 ms for the relevant interaction path.

### Measure Latency As A Flow

Interaction speed should be measured from the user event or source event to
visible output, not only by timing one function. A good flow has stable ids
across:

```text
host input or source event
-> input queue
-> runtime turn
-> document patch
-> layout or scroll state
-> render scene build
-> GPU upload
-> command encode
-> queue submit
-> present
-> optional proof/readback acknowledgement
```

Record stage timings separately so the report can distinguish runtime slowness
from layout rebuilds, upload pressure, GPU work, dev-window IPC, and proof-mode
readback.

### Separate Modes

Keep these modes labeled separately:

- **interaction mode:** what a user feels during hover, scroll, drag, resize,
  source edit, and cursor movement;
- **proof mode:** app-owned readback, screenshot artifacts, PNG writes, hashes,
  schema reports, and verifier evidence;
- **diagnostic mode:** profiler-heavy runs such as Tracy, RenderDoc, heap
  profilers, debug assertions, and verbose tracing.

Never mix proof-mode or diagnostic-mode overhead into interaction-mode budgets
unless the report explicitly says the goal was to measure that overhead.

### Make Reports Reproducible

Every performance report should include:

- build profile, optimization flags, binary hash, and current worktree/config
  identity;
- CPU model, GPU model if available, backend, adapter, driver, display refresh,
  scale factor, and present mode if available;
- scenario name, fixture size, input sequence, warmup policy, sample count, and
  duration;
- p50/p95/p99/max for the key metrics;
- profiler/tool overhead state;
- whether proof-mode readback or report writes were enabled.

## Local Boon Baseline

The current native GPU budget file already defines the minimum measurement
shape that this guide should support.

Important local budgets include:

- preview frame p95 and p99: `16.7 ms`;
- preview frame max: `33.4 ms`;
- missed frame count: `0`;
- release idle preview CPU p95: `1.0%`;
- release idle dev CPU p95: `2.0%`;
- release idle combined CPU p95: `3.0%`;
- release post-idle input-to-present p95: `33.0 ms`;
- release dev-editor scroll wheel-to-visible p95: `16.7 ms`;
- release dev-editor scroll wheel-to-visible max: `25.0 ms`;
- release example switch sync ACK p95: `16.7 ms`;
- release NovyWave input-to-visible p95: `16.7 ms`;
- release NovyWave input-to-visible max: `33.4 ms`;
- release NovyWave hot-path PNG writes: `0`;
- release NovyWave hot-path report writes: `0`;
- release Cells draw calls p95: `16`;
- release Cells queue writes p95: `8`;
- upload bytes p95: `262144`.

`NATIVE_GPU_PIPELINE.md` also requires release frame reports to include p50,
p95, p99, max frame time, missed frames, sample count, upload bytes, draw
calls, visible nodes, text runs shaped, and dropped debug telemetry.

For scroll paths, reports should include:

- runtime dispatch count for passive scroll;
- graph rebuild count;
- wheel events coalesced;
- input queue depth;
- layout rebuild scope;
- newly materialized ranges;
- sustained scroll duration;
- visible range before/after samples;
- wheel-to-visible latency by axis.

For text and GPU batching, reports should include:

- draw calls, queue writes, upload bytes, and pipeline switches;
- visible and uploaded instance counts;
- visible text runs;
- shaped text runs;
- text shape cache hits, misses, and evictions;
- glyph atlas upload bytes and evictions.

## Tool And Library Map

| Tool or library | Use it for | Avoid using it for | Boon and NovyWave fit |
| --- | --- | --- | --- |
| `std::time::Instant` | Cheap CPU wall-clock spans around app-owned stages | GPU completion timing or cross-process clock assumptions | Good default for input, runtime, layout, encode, submit-call, and report timing |
| `hdrhistogram` | p50/p95/p99/max latency histograms over many samples | Explaining causality by itself | Strong fit for frame time, input-to-visible, ACK, queue, and stage histograms |
| `tracing` and `tracing-subscriber` | Structured spans, event ids, flow ids, and low-overhead counters | Always-on verbose per-frame logs | Good fit for connecting input, runtime, layout, render, IPC, and reports |
| `tracy-client` and `tracing-tracy` | Timeline profiling, frame scopes, lock contention, thread behavior | Acceptance reports or always-on release builds | Good diagnostic profile for preview/dev interaction stalls |
| `puffin` and `puffin_egui` | Lightweight frame scopes and in-app profiling views | Final speed gates or GPU timing | Useful for local developer iteration if overhead is measured |
| `wgpu-profiler` | WGPU timer scopes backed by timestamp queries | Immediate CPU readback on every interactive frame | Good fit for GPU pass timing in proof or diagnostic mode |
| WGPU timestamp queries | GPU time between command points | CPU-side stage timing, unless paired with CPU spans | Needed to separate CPU-bound from GPU-bound frames |
| `Queue::on_submitted_work_done` | Knowing when submitted GPU work has completed | Blocking the normal interaction loop | Useful for verifier/proof scheduling and occasional diagnostics |
| `criterion` | Stable Rust microbenchmarks and small hot-kernel comparisons | End-to-end UI latency acceptance | Good for parser, runtime op, layout kernel, and data-structure A/B tests |
| `divan` | Lower-friction Rust benchmarks | Replacing full scenario gates | Good for quick local A/B tests before adding heavier benches |
| `iai-callgrind` | Deterministic instruction/cache-style CI benchmarks | Real GPU, OS, or interactive latency | Good for pure parser/runtime/layout kernels |
| `hyperfine` | Command-level benchmark timing | Per-frame or per-stage graphics diagnosis | Good for xtask, CLI, parser, and report command comparisons |
| `cargo flamegraph` and `perf` | CPU sampling and native flamegraphs on Linux | GPU timing or high-level latency causality alone | Good for finding CPU hot functions during NovyWave interactions |
| `samply` | Firefox Profiler-compatible CPU profiles | GPU pass timing by itself | Good for timeline-friendly CPU profiles |
| `pprof` | Programmatic CPU profiling in Rust processes | Always-on production measurement without overhead checks | Useful for targeted diagnostics when external profiler setup is awkward |
| `dhat`, heaptrack, Valgrind DHAT | Heap allocation count, bytes, and lifetime diagnosis | Measuring normal interactive frame time directly | Good for allocation-churn questions from the speedup docs |
| RenderDoc | Frame capture, draw inspection, pipeline state, texture/resource inspection | Measuring user-perceived latency or CPU runtime cost | Good for one-frame correctness and GPU state diagnosis |
| NVIDIA Nsight Graphics | GPU trace, frame debugging, queue/driver-level diagnosis on NVIDIA | Cross-vendor acceptance proof | Useful when NVIDIA hardware is the reproducer |
| PIX on Windows | DirectX timing captures and GPU/CPU event correlation | Linux/Wayland or non-DX backend evidence | Useful for Windows backend-specific investigations |
| Xcode Metal GPU Capture | Metal command and shader diagnosis | Non-Apple backend evidence | Useful for macOS/iOS backend-specific investigations |
| PresentMon | Present timing, frame pacing, and GPU busy context on Windows | Replacing app-owned frame ids and reports | Useful as an external sanity check for presentation pacing |

## CPU Measurement Playbook

Use CPU tools to answer these questions:

- Which stage owns the wall time: input, runtime, layout, scene build, upload
  prep, command encoding, IPC, report writing, or idle wake?
- Which functions dominate the hot frame?
- Is the cost allocation, hashing, cloning, locking, serialization, or actual
  computation?
- Does dev-window work steal time from preview interaction?

Recommended sequence:

1. Add or use existing `Instant` spans for coarse stage timing and write them
   into a report histogram.
2. Use `tracing` spans with a frame id or flow id to connect stages.
3. Run a sampling profiler such as `perf`, `cargo flamegraph`, `samply`, or
   Tracy on the same representative scenario.
4. If allocation churn appears, switch to allocation tools such as DHAT,
   heaptrack, or allocator callsite instrumentation.
5. Move small pure kernels into `criterion`, `divan`, or `iai-callgrind` only
   after the end-to-end profile proves the kernel matters.

Do not accept a microbenchmark win as a UI win until the end-to-end report
improves on the real scenario.

## WGPU And GPU Measurement Playbook

Use GPU tools to answer these questions:

- Is the frame CPU-bound, GPU-bound, upload-bound, present-bound, or blocked on
  synchronization?
- How much time is spent in each render/compute pass?
- How many draw calls, queue writes, pipeline switches, bind group changes, and
  upload bytes happen after warmup?
- Are timestamp query readbacks or proof readbacks adding synchronization?

Recommended sequence:

1. Count CPU-side renderer work first: draw calls, queue writes, upload bytes,
   instance counts, bind/pipeline switches, text cache hits/misses, and atlas
   bytes.
2. Add timestamp query scopes around GPU passes through WGPU or `wgpu-profiler`.
3. Resolve query results into buffers and consume them through a delayed
   readback ring, not an immediate per-frame CPU wait.
4. Compare CPU stage timing against GPU timestamps and present timing.
5. Use RenderDoc, Nsight, PIX, or Xcode captures to inspect a small number of
   representative frames when counters show a GPU-side mystery.

GPU timestamp measurements should be labeled with backend and adapter metadata.
Cross-backend comparisons are useful only when the report records the backend
and hardware.

## Frame Pacing And Idle Measurement

Good frame pacing needs both active and idle measurements.

Active measurements:

- frame time p50/p95/p99/max;
- missed frames;
- input-to-present and input-to-visible latency;
- event coalescing counts;
- queue depth and dropped telemetry;
- frames in flight;
- present mode and refresh rate where available.

Idle measurements:

- preview idle CPU p95;
- dev idle CPU p95;
- combined idle CPU p95;
- rendered frame delta over a fixed idle window;
- post-idle input-to-present latency.

For event-driven UI, idle should not continuously render just to keep an FPS
counter alive. FPS counters must not create the workload they claim to measure.

## Interaction Scenario Matrix

Use at least these scenario classes when measuring Boon and NovyWave:

| Scenario | Main metric | Secondary metrics |
| --- | --- | --- |
| Cold launch to first stable frame | first-frame latency | shader/pipeline warmup, first upload bytes, report freshness |
| Warm idle for 5 seconds | idle CPU and rendered-frame delta | post-idle wake latency |
| Hover/cursor movement | input-to-visible p95/max | runtime dispatch count, overlay-only invalidation, upload bytes |
| Passive scroll | wheel-to-visible p95/max | coalescing, graph rebuilds, materialized ranges, text cache reuse |
| Divider drag or resize | drag/resize-to-present p95/max | layout scope, scene rebuild scope, uploads |
| Source edit or example switch | ACK latency and new-frame-present latency | payload bytes, worker latest-wins behavior, stale result discard |
| NovyWave timeline navigation | input-to-visible p95/max | waveform page hits/misses, upload bytes, dirty region area |
| Proof capture | proof latency and artifact correctness | readback size, row padding, PNG/hash/write time |

The proof capture scenario is intentionally separate from interaction
scenarios.

## Common Mistakes

- Reporting average FPS without frame-time percentiles.
- Starting measurement before warmup and mixing shader/pipeline compilation
  into steady-state interaction results.
- Measuring debug builds and claiming release-mode performance.
- Including proof-mode readback, PNG writes, report writes, or heavy tracing in
  interaction-mode measurements.
- Reading GPU timestamp query buffers immediately and accidentally measuring
  synchronization overhead.
- Comparing reports across different backends, GPUs, display refresh rates, or
  present modes without recording those fields.
- Trusting a report whose worktree, binary hash, xtask hash, or budget hash no
  longer matches the current checkout.
- Using profiler UI screenshots as acceptance evidence instead of app-owned
  reports and native GPU proof artifacts.

## Source Appendix

Rust measurement and profiling:

- Rust Performance Book, profiling:
  <https://nnethercote.github.io/perf-book/profiling.html>
- Criterion:
  <https://bheisler.github.io/criterion.rs/book/>
- Divan:
  <https://docs.rs/divan/>
- Iai-Callgrind:
  <https://docs.rs/iai-callgrind/>
- Hyperfine:
  <https://github.com/sharkdp/hyperfine>
- hdrhistogram:
  <https://docs.rs/hdrhistogram/>
- tracing:
  <https://docs.rs/tracing/>
- tracing-subscriber:
  <https://docs.rs/tracing-subscriber/>
- tracing-tracy:
  <https://docs.rs/tracing-tracy/>
- tracy-client:
  <https://docs.rs/tracy-client/>
- Puffin:
  <https://docs.rs/puffin/>
- cargo-flamegraph:
  <https://github.com/flamegraph-rs/flamegraph>
- Samply:
  <https://github.com/mstange/samply>
- pprof:
  <https://docs.rs/pprof/>
- DHAT:
  <https://valgrind.org/docs/manual/dh-manual.html>
- heaptrack:
  <https://github.com/KDE/heaptrack>

WGPU and graphics measurement:

- wgpu profiler:
  <https://docs.rs/wgpu-profiler/>
- wgpu `QueryType`:
  <https://docs.rs/wgpu/latest/wgpu/enum.QueryType.html>
- wgpu `Queue`:
  <https://docs.rs/wgpu/latest/wgpu/struct.Queue.html>
- wgpu `Buffer`:
  <https://docs.rs/wgpu/latest/wgpu/struct.Buffer.html>
- wgpu `CommandEncoder`:
  <https://docs.rs/wgpu/latest/wgpu/struct.CommandEncoder.html>
- WebGPU specification:
  <https://www.w3.org/TR/webgpu/>
- WGSL specification:
  <https://www.w3.org/TR/WGSL/>
- RenderDoc quick start:
  <https://renderdoc.org/docs/getting_started/quick_start.html>
- NVIDIA Nsight Graphics GPU Trace:
  <https://docs.nvidia.com/nsight-graphics/UserGuide/index.html#gpu-trace>
- PIX timing captures:
  <https://devblogs.microsoft.com/pix/timing-captures/>
- Xcode Metal debugger:
  <https://developer.apple.com/documentation/xcode/metal-debugger>
- PresentMon:
  <https://github.com/GameTechDev/PresentMon>
- Android Frame Pacing:
  <https://developer.android.com/games/sdk/frame-pacing>

Event loop and frame pacing:

- winit event loop:
  <https://docs.rs/winit/latest/winit/event_loop/>
- winit window redraw requests:
  <https://docs.rs/winit/latest/winit/window/struct.Window.html#method.request_redraw>
- Perfetto FrameTimeline:
  <https://perfetto.dev/docs/data-sources/frametimeline>
- Vulkan tutorial, frames in flight:
  <https://docs.vulkan.org/tutorial/latest/03_Drawing_a_triangle/03_Drawing/03_Frames_in_flight.html>

Local docs:

- `docs/architecture/NATIVE_GPU_PIPELINE.md`
- `budgets/native-gpu.toml`
- `examples/novywave.budget.toml`
- `docs/plans/speedup/01-inspiration.md`
- `docs/plans/speedup/02-novel-research-ideas.md`
- `docs/plans/speedup/03-rust-speed-libraries.md`
- `docs/plans/speedup/04-rust-and-shader-slow-patterns.md`
