# Boon Web Effect Host

This crate owns browser File and Content capabilities, bounded IndexedDB
content storage, multishot stream backpressure, operation deadlines, and
writer commit/cancellation ordering. It deliberately does not depend on the
document or WGPU renderer, keeping real-browser lifecycle tests bounded.

## Gates

```bash
cargo check -p boon_web_effect_host --target wasm32-unknown-unknown
CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner \
  cargo test -p boon_web_effect_host --target wasm32-unknown-unknown --lib
```
