# Human-Like Scenario Testing

Date: 2026-06-12

## Purpose

This file defines how to test Boon and NovyWave with human-like, OS-agnostic
end-to-end scenarios. The goal is to verify the same user stories a human would
try, while still using app-owned automation, app-owned pixels, deterministic
fixtures, and current report evidence.

Human-like does not mean fake human observation, whole-desktop screenshots, or
private runtime calls. In this repo, the portable automated tier means:

```text
scenario intent
-> BoonDriver or operator host input
-> hit, focus, and scroll routing
-> source intent
-> public runtime path
-> document/layout update
-> renderer update
-> app-owned WGPU readback
-> semantic, visual, timing, and honesty report
```

Local constraints:

- `docs/architecture/NATIVE_GPU_PIPELINE.md` remains the active native GPU
  evidence contract.
- `docs/architecture/BOON_DRIVER.md` is the OS-agnostic automation direction.
- `docs/architecture/LINUX_HUMAN_LIKE_TESTING.md` is a platform-specific
  upgrade tier, not the portable default.
- Existing `.scn` files remain the scenario source format. Do not invent a
  parallel scenario DSL for this docs pass.
- Manual human testing stays a separate follow-up. Automated reports must not
  claim `human` or `real-window` unless that stronger evidence was actually
  collected.

## Definitions

### Evidence Tiers

Use explicit evidence tiers so reports cannot blur what was proven:

- `runtime`: direct runtime calls. Useful for semantic tests only.
- `boon-driver`: app-owned automation through target resolution, host input,
  source binding, runtime, render, and readback.
- `operator-host-input`: current native verifier shape that injects host input
  at the public app boundary and records app-owned proof.
- `real-window`: OS/compositor delivered input to the exact preview/dev window
  and process.
- `human`: manual observation with explicit observer, session, artifact, and
  provenance fields.

The OS-agnostic path in this document should write honesty fields equivalent to
`operator_host_input = true`, `real_os_input = false`, and
`human_observation = false`.

### Human-Like OS-Agnostic Scenario

A scenario is human-like when:

- it starts from a named user story;
- actions are written as user-visible intent;
- targets are resolved through semantic selectors, not private Rust state;
- input travels through the same host/input route as the app;
- pass/fail checks observe user-visible state and app-owned pixels;
- internal runtime data explains the result but does not replace visual proof.

## Scenario Authoring Rules

Write scenarios as executable user stories:

```text
Given deterministic fixture state
When the user performs a visible action
Then the user-visible surface and semantic state match expectations
```

Keep each story short enough to diagnose. Long workflows should be split into
named phases that can still be replayed together for regression coverage.

Preferred selectors:

- `role` plus `name`, for example `button[name="Load Files"]`;
- stable source binding path;
- manifest scenario alias;
- control id exposed by the document model;
- selected row, signal, scope, time range, or grid address;
- scroll region or hit region identity;
- state constraints such as selected, expanded, focused, checked, disabled.

Forbidden as authored selectors:

- raw screen coordinates;
- GPU instance ids;
- Rust variable names;
- private runtime field offsets;
- example-specific enum variants;
- visible text alone when multiple controls could match;
- source file paths as architecture decisions.

Coordinates may appear in reports only after the harness resolves a semantic
target. A report should record both the authored selector and the resolved
hit point, hit region, focus target, and source binding.

## Waits And Flake Policy

Use auto-waiting around app-owned conditions instead of sleeps:

- next committed frame revision;
- source intent routed;
- runtime assertion satisfied;
- document/render patch observed;
- accessibility-style tree snapshot stabilized;
- app-owned readback available;
- queue depth below threshold.

Retries are diagnostic evidence, not success laundering. If a scenario passes
only after retry, mark it flaky, keep the first-failure artifacts, and fail any
readiness gate whose flake budget is zero.

## Assertion Layers

Each important step should combine several assertions:

- **Input route:** the action went through BoonDriver/operator host input, not a
  private runtime shortcut.
- **Hit/focus route:** the target was resolved to a real document node, hit
  region, scroll region, and focus target where relevant.
- **Source intent:** the expected source binding and source event were routed.
- **Semantic state:** root text, runtime deltas, list state, selected row, active
  file, active signal, cursor, marker, viewport, or format state changed as
  expected.
- **Render state:** document/render patch summaries and frame revision prove
  the renderer saw the change.
- **Visual state:** app-owned readback, crop, or masked comparison proves the
  surface looks correct enough for the scenario.
- **Performance state:** frame time, input-to-visible latency, and missed frames
  are recorded when speed is part of the story.

Internal state is useful for diagnosis. It should not be the only pass/fail
oracle for an interactive UI story.

## Required Per-Step Evidence

Every human-like automated step should record:

- example id and manifest path;
- source path, source hash, scenario path, scenario hash, fixture hash;
- worktree fingerprint, binary hash, command argv, generated timestamp;
- evidence tier and input provenance;
- authored selector;
- resolved node, bounds, hit region, scroll region, focus before/after;
- input event sequence and timestamps;
- source binding id/path and source intent;
- runtime semantic delta;
- document patch summary;
- render patch summary;
- before/after frame revision and frame hash;
- app-owned readback artifact paths and SHA-256 hashes;
- visual crop/diff artifacts where used;
- timing samples for input-to-visible and frame time;
- pass/fail list where every pass field points to concrete evidence.

Reports should fail closed when evidence is missing. They must not silently
downgrade from user-visible evidence to runtime-only evidence.

## NovyWave Story: Open And Inspect A Waveform

The first story to document and later harden is:

```text
Story: open-vcd-and-inspect-core-waveform

As a waveform viewer user,
I want to open a VCD file, inspect the hierarchy and selected signals,
move the cursor, and verify displayed values,
so that I can trust that NovyWave is loading and rendering waveform data.
```

### Given

- NovyWave starts from empty or reset state.
- Viewport, scale, theme, locale, clock, animation policy, and fixture seed are
  fixed.
- The scenario source is `examples/novywave/RUN.bn`.
- The existing scenario source is `examples/novywave.scn`.
- The deterministic public fixture is `simple.vcd`.
- The report records source, scenario, fixture, binary, budget, and worktree
  identities.

### When And Then

1. Activate `Load Files`.
   - Action: click the visible load/open-file control through semantic target
     resolution.
   - Expect: load dialog is open, title/hint/path selection are visible, and
     source intent routes through `open_load_files_dialog`.

2. Supply the deterministic waveform fixture.
   - Action: choose or accept `simple.vcd` through the host bridge path.
   - Expect: loaded descriptor state includes active file, VCD format, content
     identity or descriptor reference, `FILE_READY`, hierarchy page, signal
     count, and bounded page limits.

3. Assert bridge policy.
   - Expect: Boon receives comparable descriptors/pages only; no Rust or
     Wellen handles are exposed as Boon semantic values.
   - Expect: full waveform payloads do not enter Boon; pages are bounded by
     visible rows, transition count, and payload bytes.

4. Select scope and signals.
   - Action: select `simple_tb.s`, then select `A[3:0]` and `B[3:0]` rows.
   - Expect: selected row count and order, active signal label, active scope
     label, timeline range, cursor label, and waveform segment labels/counts
     match the fixture.

5. Move through the waveform like a user.
   - Action: click or hover the waveform, use keyboard cursor movement, pan,
     zoom, or format controls.
   - Expect: cursor, viewport, format, marker, and visible values update through
     source intent routing and app-owned frame changes.

6. Verify stale-response behavior.
   - Action: issue a page-changing operation and then a newer page request.
   - Expect: old bridge responses are rejected or ignored, and the report names
     the accepted current page.

7. Verify visual output.
   - Expect: app-owned readback is nonblank, correct surface/crop regions are
     present, waveform rows are aligned with labels, cursor/marker overlays are
     visible, and no unrelated blank region covers the UI.

### Current-State Honesty

Today, the NovyWave scenario already models much of this flow with root-text,
source-event, and fixture metadata assertions. The real waveform file-open
bridge is still represented as a planned/draft `wellen.v1` pure-data contract.
Do not claim live Wellen parsing or real file-open bridge completion until that
host effect exists.

The planned future bridge contract should be:

```text
host validates file
-> host computes content identity
-> host returns canonical descriptor/page data
-> Boon receives bounded pure data
-> renderer displays only visible windows/pages
```

## Adding More Stories Later

Each new story should add:

- a stable story id;
- user goal and user-visible success criteria;
- deterministic fixture state;
- semantic selectors for each action;
- per-step expected source intent;
- semantic assertions;
- visual assertions and crop regions;
- speed/resource expectations where relevant;
- negative or metamorphic variant;
- report fields required for freshness and provenance.

Candidate future NovyWave stories:

- `open-ghw-and-return-to-vcd`;
- `select-uart-scope-and-search-tx`;
- `pan-zoom-and-preserve-cursor-value`;
- `reject-stale-waveform-page`;
- `toggle-marker-and-jump`;
- `format-cycle-preserves-selected-signal`;
- `switch-theme-with-readable-waveform`;
- `large-fst-bounded-page-only`.

## Anti-Cheating Rules

The scenario is not honest if it can pass by ignoring the source, branching on
the example name, replaying fixed pixels, or editing a report.

Required defenses:

- self-invalidating reports: stale source, scenario, fixture, binary, budget,
  worktree, timestamp, or artifact hashes fail;
- every `pass` field links to evidence and artifact SHA-256 values;
- genericity scanners reject example-specific render branches, source-path
  dispatch, fixture-name logic, private runtime dispatch, and hardcoded output
  in runtime/native/render layers;
- hidden/generated fixtures vary file path, fixture id, labels, symbols,
  viewport, theme, declaration order where legal, and scenario values;
- metamorphic checks require equivalent visible behavior after legal source
  rename, nonsemantic reformat, path move, and fixture-id changes;
- negative checks reject missing artifacts, copied pixel hashes, fake
  real-OS-input claims, private runtime dispatch, browser/Xvfb/Ply
  substitution, full waveform payloads entering Boon, and source-event-only
  shortcuts;
- mutation tests flip report status, remove an artifact, alter one pixel,
  change one source hash, skip input dispatch, bypass render, or swap fixture
  names and require the verifier to fail.

If NovyWave becomes a hard native gate, extend genericity scanners to include
NovyWave-specific forbidden strings and fixture names outside allowed examples,
scenarios, docs, and report labels.

## Visual Assertions

Use app-owned readback first. Whole-desktop screenshots are diagnostic only.

Recommended visual checks:

- nonblank frame and non-single-color frame;
- expected crop contains content above a minimum contrast/edge threshold;
- label row and waveform row alignment;
- cursor/marker line visible in the waveform region;
- text readability in dark and light modes;
- no large blank region over the main UI;
- bounded visual diff against a deterministic baseline where the renderer is
  stable enough;
- tolerant or perceptual diff for GPU/backend differences when exact pixel
  equality is too strict.

Every visual artifact should record surface size, scale factor, color format,
backend, adapter, crop rect, mask policy, and SHA-256.

## Tool And Library Map

| Use case | Tool or crate | Fit | Notes |
| --- | --- | --- | --- |
| App-owned GPU readback | `wgpu` render target plus copy to buffer | High | Foundation for native proof; normalize row padding, scale, format, and backend metadata |
| PNG decode/encode and exact pixels | `image` | High | Good for readback artifacts and deterministic crops |
| Renderer-tolerant image diff | `rendiff` | High | Useful when small color/spatial differences are acceptable |
| Perceptual visual diff | `dssim` | High | Good similarity score; thresholds must be calibrated |
| Metric visual checks | `image-compare`, `image-similarity` | Medium | Useful for SSIM/RMS/MSE/PSNR style checks after maturity review |
| Coarse visual smoke | `image_hasher` | Low-medium | Good for stale/blank/wrong-composition hints, not correctness gates |
| Structured snapshots | `insta`, `goldenfile`, `snapbox` | High | Snapshot report metadata, traces, and display summaries; store images separately |
| Scenario-file harnessing | `datatest-stable`, `libtest-mimic` | High | Good for `.bn` plus `.scn` corpora |
| Fixture matrices | `rstest` | Medium | Good for viewport, theme, backend, fixture, and scale-factor matrices |
| Process orchestration | `assert_cmd`, `wait-timeout` | High | Wait on sockets, reports, and app-owned readiness, not sleeps |
| Bounded polling | small local poll loop, `again`, `backoff`, `tokio-retry` | Medium | Use for app-owned readiness; do not hide visual assertion failures |
| Generated fixtures | `proptest`, `arbitrary`, `bolero` | High | Persist seed and minimized failing case |
| Reproducible randomness | `rand_chacha` | High | Record seed, generator version, and fixture hash |
| CI orchestration | `cargo-nextest` | High | Retries are flake evidence, not acceptance for native gates |
| Report schema checks | `schemars`, `jsonschema` | High | Keep schemas strict; do not weaken to pass |
| Mutation checks | `cargo-mutants` | Medium-high | Useful for verifier/report logic and pure Rust parts |

## Review Checklist

Before accepting a new human-like scenario:

- Does it describe a user story rather than an implementation script?
- Does every action use a semantic selector?
- Does the report prove the current source, scenario, fixture, binary, and
  artifact hashes?
- Does it exercise public host input and source intent routing?
- Does it include user-visible semantic assertions?
- Does it include app-owned visual evidence?
- Does it reject stale, missing, copied, or fabricated artifacts?
- Does it still pass after legal source rename/reformat/path changes?
- Does at least one generated or hidden fixture make hardcoding risky?
- Does failure output show enough artifacts to debug the cause?
- Does it avoid claiming human or real OS input in the OS-agnostic route?

## Source Appendix

Local docs and files:

- `docs/architecture/NATIVE_GPU_PIPELINE.md`
- `docs/architecture/BOON_DRIVER.md`
- `docs/architecture/LINUX_HUMAN_LIKE_TESTING.md`
- `docs/plans/EXAMPLE_VERIFICATION_PLAN.md`
- `docs/plans/MANUAL_TESTING_RUNBOOK.md`
- `examples/manifest.toml`
- `examples/novywave.scn`
- `examples/novywave/RUN.bn`
- `examples/novywave/Bridge/NovyBridge.bn`
- `docs/architecture/BOON_RUST_BRIDGE.md`

E2E and scenario testing:

- Cucumber Gherkin reference:
  <https://cucumber.io/docs/gherkin/reference/>
- Testing Library guiding principles:
  <https://testing-library.com/docs/guiding-principles/>
- Playwright best practices:
  <https://playwright.dev/docs/best-practices>
- Playwright actionability:
  <https://playwright.dev/docs/actionability>
- Playwright visual comparisons:
  <https://playwright.dev/docs/test-snapshots>
- Playwright ARIA snapshots:
  <https://playwright.dev/docs/aria-snapshots>
- W3C WebDriver interactability:
  <https://www.w3.org/TR/webdriver2/#interactability>
- Cypress retry-ability:
  <https://docs.cypress.io/app/core-concepts/retry-ability>

Honesty, provenance, and anti-cheating:

- SLSA build provenance:
  <https://slsa.dev/spec/v1.2/build-provenance>
- Google Testing Blog, test behavior not implementation:
  <https://testing.googleblog.com/2013/08/testing-on-toilet-test-behavior-not.html>
- NIST CAISI cheating examples:
  <https://www.nist.gov/caisi/cheating-ai-agent-evaluations/2-examples-cheating-caisis-agent-evaluations>
- CockroachDB metamorphic testing:
  <https://www.cockroachlabs.com/blog/metamorphic-testing-the-database/>
- Metamorphic testing survey:
  <https://arxiv.org/abs/1802.07361>
- cargo-mutants:
  <https://mutants.rs/>

Rust tools:

- wgpu:
  <https://docs.rs/wgpu/>
- wgpu buffer readback:
  <https://docs.rs/wgpu/latest/wgpu/struct.Buffer.html>
- Learn Wgpu windowless rendering:
  <https://sotrh.github.io/learn-wgpu/showcase/windowless/>
- image:
  <https://docs.rs/image/>
- rendiff:
  <https://docs.rs/rendiff/>
- dssim:
  <https://crates.io/crates/dssim>
- image-compare:
  <https://docs.rs/image-compare/>
- insta:
  <https://docs.rs/insta/>
- datatest-stable:
  <https://github.com/nextest-rs/datatest-stable>
- libtest-mimic:
  <https://docs.rs/libtest-mimic/>
- rstest:
  <https://docs.rs/rstest/>
- assert_cmd:
  <https://docs.rs/assert_cmd/>
- proptest:
  <https://altsysrq.github.io/proptest-book/>
- rand_chacha:
  <https://docs.rs/rand_chacha/>
- cargo-nextest:
  <https://nexte.st/>
- schemars:
  <https://graham.cool/schemars/>
- jsonschema:
  <https://docs.rs/jsonschema/>
