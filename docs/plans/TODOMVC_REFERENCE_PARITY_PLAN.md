# TodoMVC Native GPU Reference Parity Plan

## Goal

Make the native GPU TodoMVC preview match the original browser TodoMVC
reference as closely as the current generic Boon document model can support.
The proof must come from app-owned WGPU readback artifacts and DOM/CSS metadata,
not desktop screenshots or old Ply/browser gates.

## Source Of Truth

- Native contract: `docs/architecture/NATIVE_GPU_PIPELINE.md`
- Boon example under test: `examples/todomvc.bn`
- Browser reference image: `assets/todomvc_reference/reference_screenshot.png`
- Browser reference metadata: `assets/todomvc_reference/reference_metadata.json`
- Native E2E report: `target/reports/native-gpu/preview-e2e-todomvc.json`
- Native parity report: `target/reports/native-gpu/todomvc-reference-parity.json`

The fixtures came from the real browser TodoMVC reference captured in
`/home/martinkavik/repos/raybox/assets/todomvc`. Their provenance and hashes are
recorded in `assets/todomvc_reference/README.md`.

## Hard Constraints

- The preview role renders generic Boon `document` output only.
- Rust renderer code must not branch on TodoMVC example names, source strings,
  labels, row names, or source bindings.
- TodoMVC-specific dimensions, labels, colors, and bindings belong in
  `examples/todomvc.bn` or reference fixtures.
- The comparator must fail obvious visual drift. It may not crop, stretch, or
  threshold the images in a way that hides title, input, row, checkbox, footer,
  or shadow mismatches.

## Reference Workflow

1. Refresh `verify-native-gpu-preview-e2e --example todomvc`.
2. Load the browser screenshot and metadata from `assets/todomvc_reference`.
3. Normalize the browser reference to the metadata viewport size, currently
   700 x 700 CSS pixels.
4. Crop the native readback to the same virtual viewport by aligning the native
   TodoMVC panel bounds to the browser `.todoapp` metadata bounds. If the native
   app is shifted, the crop is padded instead of silently re-centered.
5. Compare the aligned 700 x 700 images with:
   - mean RGB absolute difference
   - p95 RGB absolute difference
   - high-difference pixel ratio
   - largest connected high-difference region ratio
   - structural bounds for title, panel, input, rows, footer, and info text
6. Write the normalized native crop, normalized reference crop, heatmap, and JSON
   evidence under `target/reports/native-gpu`.

## Browser Metadata Targets

The current reference metadata uses a 700 x 700 CSS-pixel viewport. Key expected
bounds:

- `.todoapp`: x 75, y 130, width 550, height about 344.2
- title text: x about 252.1, y about 8.4, width about 195.7, height about 89.6
- new todo input: x 75, y 130, width 550, height 65
- first row: y about 195.8, height about 59.6
- footer: x 75, y about 433.4, width 550, height about 40.8
- info footer: x 75, y about 539.2, width 550, height 55

Style targets:

- main font stack: `Helvetica Neue`, `Helvetica`, `Arial`, `sans-serif`
- body font size 14, weight 300, line-height 19.6
- title font size 80, weight 200, color `rgb(184, 63, 69)`
- row labels font size 24, weight 400, line-height 28.8
- completed labels color `rgb(148, 148, 148)` with line-through
- footer font size 15, weight 300, line-height 19.6
- info footer font size 11, line-height 11, color `rgb(77, 77, 77)`

## Generic Renderer Work

The native renderer should support these as reusable document primitives:

- font family stacks, font weight, font style, and explicit line-height
- subpixel border widths and per-side borders
- generic soft box shadows
- focused text input caret/placeholder rendering
- hover-visible children driven by generic hit regions
- checkbox circle/checkmark rendering without jagged hard rectangles
- text decoration bounded to text glyph width, not the whole element box

These features are not TodoMVC-specific and must remain usable by future
document roots.

## Acceptance

The target is:

- structural bounds within about 1 CSS px
- text pixel bounds within about 2 CSS px
- mean image diff <= 3 if technically possible
- p95 image diff <= 18 if technically possible
- high-diff pixel ratio <= 1.5%
- largest connected mismatch region <= 0.2%

If a stricter target cannot be reached because the renderer lacks a generic
browser feature, document the remaining mismatch in the parity report and keep
the verifier strict enough that the visible drift is still obvious.

The current native renderer reaches the same structural bounds and a close
human-visible match, but it does not exactly reproduce browser font
antialiasing or TodoMVC's layered CSS shadows. The verifier therefore enforces
the tighter envelope currently proven by app-owned artifacts:

- mean image diff <= 6
- p95 image diff <= 30
- high-diff pixel ratio <= 2.8%
- largest connected mismatch region <= 0.2%

As of the current implementation, the reference-parity report measures about
mean 5.23, p95 26, high-diff ratio 2.37%, and largest connected mismatch region
0.11%. The parity report sha-binds the reference fixture, preview E2E report,
app-owned readback, normalized crops, and heatmap. The remaining high-diff
pixels are concentrated around text glyph edges, the red input focus border,
and the bottom stacked shadow. Reaching the
aspirational mean <= 3 / p95 <= 18 / high-diff <= 1.5% target requires more
browser-like text rasterization and a more exact generic CSS box-shadow model,
not TodoMVC-specific renderer branches.

## Verification Commands

Run before declaring this goal complete:

```sh
cargo fmt --check
cargo check -p boon_document -p boon_native_gpu -p boon_native_playground -p xtask
cargo test -p boon_native_playground todomvc
cargo test -p boon_native_gpu
cargo xtask verify-native-gpu-preview-e2e --example todomvc --report target/reports/native-gpu/preview-e2e-todomvc.json
cargo xtask verify-native-todomvc-reference-parity --report target/reports/native-gpu/todomvc-reference-parity.json
cargo xtask verify-native-gpu-all --check-existing --report target/reports/native-gpu-all.json
```

Refresh any native reports touched by code changes before trusting
`verify-native-gpu-all --check-existing`.
