# Boon Web Host

`boon_web_host` is the generic Rust/Wasm browser boundary for Boon document
programs. It does not contain application routes, drawing policy, or a second
HTML/CSS renderer.

## Rendering

- A minimal host mounts one `<canvas>` and a normally hidden unsupported-state
  message.
- `WebGpuCanvasHost` requests only `wgpu::Backends::BROWSER_WEBGPU`; there is no
  WebGL product fallback.
- Product pixels use `boon_native_gpu::VisibleLayoutRenderer`, the same retained
  renderer and physical material/light scene consumed by native WGPU.
- `requestAnimationFrame` pacing is demand-driven with bounded interaction
  bursts and a hard stop.
- MapViewport uses the shared renderer's retained tile request, CPU cache, GPU
  cache, stale-result, and event interfaces. Browser raster decoding uses
  bounded `createImageBitmap` plus an offscreen decode canvas; that canvas is
  never a product renderer.
- `BrowserMapViewportHost` binds descriptor capabilities to the asynchronous
  tile pump, cancels stale generations, prewarms accepted textures before the
  next product frame, and rebuilds retained GPU resources after device loss.
  `MapViewportHostController` maps pointer, wheel, touch, pinch, keyboard, and
  resize input to retained descriptor patches and emits generic camera/overlay
  events for the owning program.

## Browser Mechanics

- Pointer, wheel, touch/pinch, keyboard, focus, IME, clipboard, resize, scale,
  visibility, connectivity, reduced-motion, page lifecycle, and URL changes are
  translated into public generic host events.
- The accessibility DOM is a keyed projection of
  `SemanticWebBridgeSnapshot`. It carries roles, names, values, focus, actions,
  and IME endpoints but no independently authored visual layout.
- Fetch capabilities bind method, origin, path prefix, headers, timeout, and
  byte limits. External access is fixed to named HTTPS DNS endpoints; arbitrary
  URLs and literal IP destinations are rejected.
- WebSocket capabilities are same-origin, protocol/path constrained, and bound
  inbound and outbound messages and bytes. Dropping the adapter closes the
  socket.
- History changes remain under a declared path prefix. Clipboard access is
  bounded and may require current user activation.
- IndexedDB preference storage uses an explicit versioned schema, typed logical
  namespaces, bounded keys/values/entry counts, and atomic per-operation
  transactions. It has no LocalStorage fallback and does not replace Boon's
  semantic persistence engine.

## Explicitly Unsupported

App-owned presented-frame WebGPU readback is not yet exposed as a browser-safe
transaction by the shared renderer. `BrowserHostSupport` and
`request_presented_frame_readback` report this explicitly. Normal product
rendering never substitutes a screenshot, DOM renderer, WebGL renderer, or
human observation.

Native tests cover the platform-neutral descriptor interaction contract. Wasm
build and Clippy gates cover the WebGPU host integration. Browser presented
frame evidence remains explicitly unsupported until a browser runner can prove
the exact app-owned frame; no alternate proof is inferred from successful
compilation.

## Gates

```bash
cargo test -p boon_web_host
cargo clippy -p boon_web_host --all-targets --no-deps -- -D warnings
cargo check -p boon_web_host --target wasm32-unknown-unknown
cargo clippy -p boon_web_host --target wasm32-unknown-unknown --no-deps -- -D warnings
CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner \
  cargo test -p boon_web_host --target wasm32-unknown-unknown --test storage_wasm
CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner \
  cargo test -p boon_web_effect_host --target wasm32-unknown-unknown --lib
```
