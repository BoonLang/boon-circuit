# TodoMVC Browser Reference

These fixtures are the current browser-rendered TodoMVC reference for native GPU
pixel parity work.

- `reference_screenshot.png`
  - Source: `/home/martinkavik/repos/raybox/assets/todomvc/reference_screenshot.png`
  - SHA-256: `4eed3835c50064087a378cae337df2a5e4b3499afd638e7e1afed79b6647d1d5`
  - Size: 1400 x 1400 pixels for a 700 x 700 CSS-pixel viewport.
- `reference_metadata.json`
  - Source: `/home/martinkavik/repos/raybox/assets/todomvc/reference_metadata.json`
  - SHA-256: `f2a913ebd35f08363ab24d3d17aaec6e4d053036b01834bf587b3c94c45e7852`
  - Contains DOM/CSS bounds, text bounds, computed styles, and browser metadata
    used by `verify-native-todomvc-reference-parity`.

The verifier compares native app-owned WGPU readback pixels against these
fixtures. Desktop screenshots are intentionally not part of this workflow.
