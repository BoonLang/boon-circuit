# NovyWave Rust Reference Parity Ledger

This ledger tracks the clean-room comparison between the original Rust
NovyWave UI and the Boon/native NovyWave example.

## Evidence

- 2026-06-08: Inspected the Rust NovyWave startup/config path in
  `/home/martinkavik/repos/NovyWave/frontend/src/config.rs`,
  `/home/martinkavik/repos/NovyWave/shared/src/lib.rs`, and
  `/home/martinkavik/repos/NovyWave/backend/src/main.rs`. The Rust contract
  restores opened files, expanded scopes, selected scope, selected variables,
  signal groups, markers, and timeline state from `WorkspaceSection` /
  `SharedAppConfig`, delivered through `WorkspaceLoaded` or `ConfigLoaded`.
- 2026-06-08: Replaced the Boon reset-state `simple_vcd_*` waveform helper
  fields with config-like startup records: opened workspace files, expanded
  scopes, selected-variable config, signal-group config, timeline config,
  marker config, file-keyed waveform transition records, and file-keyed
  waveform segment records. The reset file remains `simple.vcd` because that
  is the selected startup config record, not a separate simple-specific
  projection variable.
- 2026-06-08: Converted `active_file`, `active_scope`, and
  `selected_signal_family` from source-event transforms into `HOLD ... LATEST`
  state cells so default-file restore reads `store.startup_*` config fields
  instead of storing the literal token text. This fixes the restored light-mode
  file tree so `simple.vcd -> simple_tb.s -> A[3:0] B[3:0]` is expanded again.
- 2026-06-08: Added runtime restore assertions for the simple.vcd reset path:
  active scope is `simple_tb_s`, `simple_tb.s` is expanded after empty ->
  load-default, selected variables remain `A[3:0]` / `B[3:0]`, and waveform
  segments expose the 50s/100s projection widths from the startup file range.
- 2026-06-08: Verification passed:
  `cargo test -p boon_runtime novywave_ -- --nocapture`,
  `cargo check -p boon_native_playground`,
  `cargo test -p boon_native_playground novywave_selected_visible_items_align_name_value_and_wave_columns -- --nocapture`,
  and `cargo test -p boon_native_playground novywave_dark_light_material_readbacks_cover_visual_regions -- --nocapture`.
- 2026-06-08: Fixed native relayout viewport preservation and root `Fill`
  lowering.
- 2026-06-08: `cargo test -p boon_native_playground novywave_live_input_relayout_preserves_current_viewport -- --nocapture`
  passed.
- 2026-06-08: Reworked the Boon selected variables panel from a tall toolbar
  stack into header, name/value/ruler header, bounded waveform body, and compact
  footer bands.
- 2026-06-08: `cargo test -p boon_native_playground novywave_dark_light_material_readbacks_cover_visual_regions -- --nocapture`
  passed and generated fresh app-owned readbacks under
  `target/artifacts/native-gpu/tests/1227497-*`.
- 2026-06-08: Added row-local remove buttons in the selected-variable name
  cells, reusing existing signal-specific Boon events.
- 2026-06-08: `cargo test -p boon_native_playground novywave_dark_light_material_readbacks_cover_visual_regions -- --nocapture`
  passed again and generated fresh app-owned readbacks under
  `target/artifacts/native-gpu/tests/1260324-*`.
- 2026-06-08: `cargo xtask verify-native-gpu-preview-e2e --example novywave --report target/reports/native-gpu/preview-e2e-novywave.json`
  passed.
- 2026-06-08: `cargo check -p boon_native_playground` passed without warnings.
- 2026-06-08: Added `selected_visible_items` as the shared Boon row stream and
  refactored the selected variables body into sibling name, value, and wave
  columns generated from that stream.
- 2026-06-08: `cargo test -p boon_native_playground novywave_dark_light_material_readbacks_cover_visual_regions -- --nocapture`
  passed after the sibling-column refactor and generated fresh app-owned
  readbacks under `target/artifacts/native-gpu/tests/1414084-*`.
- 2026-06-08: Added and passed
  `cargo test -p boon_native_playground novywave_selected_visible_items_align_name_value_and_wave_columns -- --nocapture`
  to prove name/value/wave row y-alignment from app-owned layout data.
- 2026-06-08: Re-ran
  `cargo xtask verify-native-gpu-preview-e2e --example novywave --report target/reports/native-gpu/preview-e2e-novywave.json`
  after the sibling-column refactor; status `pass`, `native_gpu_contract: true`,
  and artifact freshness `pass`.
- 2026-06-08: Converted enum-like NovyWave Boon state from `TEXT { ... }`
  literals to bare tags for dialogs, panel sizing/arrangement, waveform status,
  format/zoom/pan/cursor state, marker visibility, row size, analog validity,
  group state, workspace selection, external-file active state, and selected-row
  profile state. UI copy, file names, paths, signal names, labels, and text
  search needles remain `TEXT`.
- 2026-06-08: Updated generic runtime evaluation to preserve `Enum`/tag values
  as `BoonValue::Enum` internally while keeping protocol/document/json output
  string-compatible.
- 2026-06-08: Tag/runtime verification passed:
  `cargo check -p boon_runtime`,
  `cargo test -p boon_runtime novywave_format_cycle_updates_summary_and_rendered_values -- --nocapture`,
  `cargo test -p boon_runtime root_latest_shared_key_skip_branches_do_not_abort_other_targets -- --nocapture`,
  `cargo test -p boon_runtime root_derived_list_literal_when_matches_runtime_values -- --nocapture`,
  `cargo test -p boon_runtime live_runtime_applies_root_text_key_payload_source_events -- --nocapture`,
  `cargo test -p boon_ir then_call_to_pure_match_function_lowers_to_match_const -- --nocapture`,
  `cargo test -p boon_ir pure_when_over_root_state_lowers_as_pure_derived_value -- --nocapture`,
  `cargo check -p boon_native_playground`,
  `cargo test -p boon_native_playground novywave_live_input_relayout_preserves_current_viewport -- --nocapture`,
  `cargo test -p boon_native_playground novywave_selected_visible_items_align_name_value_and_wave_columns -- --nocapture`,
  and `cargo test -p boon_native_playground novywave_dark_light_material_readbacks_cover_visual_regions -- --nocapture`.
- 2026-06-08: Replaced the flat `selected_visible_items:
  selected_signal_rows` alias with a mixed row stream containing `GroupHeader`
  and `VariableRow` records. The stream now keeps the group header inline, hides
  grouped members when collapsed, restores them when expanded, and removes the
  header independently on `group_remove`.
- 2026-06-08: Updated the active selected-variable name/value/wave column
  renderers to branch on `row.item_kind`: variable rows keep
  `row_height + 3`, group headers render as 30px rows with no divider,
  grouped variables gain the Rust-style additional name indentation, and group
  header controls include expand/collapse, rename affordance, member count, and
  remove/ungroup affordance.
- 2026-06-08: Fixed row-local value action wiring so digital rows use the real
  `format_cycle` source and analog rows use the real
  `analog_limits_manual` source.
- 2026-06-08: Added and passed
  `cargo test -p boon_runtime novywave_selected_visible_items_model_group_headers_and_collapse -- --nocapture`
  for the mixed visible-row model, collapse/expand behavior, and group removal.
- 2026-06-08: Selected-row batch verification passed:
  `cargo test -p boon_runtime novywave_selected_visible_items_model_group_headers_and_collapse -- --nocapture`,
  `cargo test -p boon_runtime novywave_format_cycle_updates_summary_and_rendered_values -- --nocapture`,
  `cargo check -p boon_native_playground`,
  `cargo test -p boon_native_playground novywave_selected_visible_items_align_name_value_and_wave_columns -- --nocapture`,
  and `cargo test -p boon_native_playground novywave_live_input_relayout_preserves_current_viewport -- --nocapture`.
  The slower `verify-native-gpu-preview-e2e --example novywave` run was not
  counted as fresh evidence for this batch because it was intentionally stopped
  after the embedded readback test had already passed but before the xtask
  process returned a final report.
- 2026-06-08: Added explicit Boon waveform file metadata records for the
  built-in VCD/FST/GHW files and selected-variable file metadata on selected
  rows. Bridge file format/digest/ref/stats/window descriptors now derive from
  metadata fields instead of repeated filename branches, while external host
  metadata still routes through explicit external metadata sink fields.
- 2026-06-08: Added and passed
  `cargo test -p boon_runtime novywave_waveform_metadata_drives_selected_file_and_timeline_window -- --nocapture`
  proving selected files switch metadata range/window fields for VCD, FST, and
  GHW.
- 2026-06-08: Fixed selected-row remove source lowering by adding
  domain-specific `remove_kind` tags to selected row records instead of
  text-pattern dispatch in the view.
- 2026-06-08: Metadata/source-routing batch verification passed:
  `cargo test -p boon_runtime novywave_ -- --nocapture`,
  `cargo check -p boon_native_playground`, and
  `cargo test -p boon_native_playground novywave_search_and_waveform_keyboard_work_from_real_preview_input -- --nocapture`.
- 2026-06-08: Reworked the Files and Scopes browser from two separate sections
  into one Rust-style file/scope tree. Verification passed:
  `cargo test -p boon_runtime novywave_ -- --nocapture`,
  `cargo check -p boon_native_playground`,
  `cargo test -p boon_native_playground novywave_controls_lower_hover_material_and_pointer_contracts -- --nocapture`,
  and
  `cargo test -p boon_native_playground novywave_search_and_waveform_keyboard_work_from_real_preview_input -- --nocapture`.
- 2026-06-08: Checked the Rust reference
  `/home/martinkavik/repos/NovyWave/test_files/simple.vcd`: timescale `1s`,
  scope `simple_tb.s`, variables `A[3:0]` and `B[3:0]`, transitions
  `A=0xa/B=0x3 at 0s`, `A=0xc/B=0x5 at 50s`, and `A=0x0/B=0x0 at 150s`
  with file end at `250s`.
- 2026-06-08: Reworked Boon reset-state metadata, file/scope tree rows,
  variable rows, selected-variable defaults, waveform segment labels, cursor
  values, bridge request/response labels, and timeline axis labels to match
  that `simple.vcd` content instead of the old synthetic `top.cpu` demo.
- 2026-06-08: Simple VCD reset-state verification passed:
  `cargo test -p boon_runtime novywave_ -- --nocapture`,
  `cargo check -p boon_native_playground`,
  `cargo test -p boon_native_playground novywave_controls_lower_hover_material_and_pointer_contracts -- --nocapture`,
  `cargo test -p boon_native_playground novywave_selected_visible_items_align_name_value_and_wave_columns -- --nocapture`,
  `cargo test -p boon_native_playground novywave_search_and_waveform_keyboard_work_from_real_preview_input -- --nocapture`,
  and
  `cargo test -p boon_native_playground novywave_dark_light_material_readbacks_cover_visual_regions -- --nocapture`.
- 2026-06-08: Added explicit Boon transition/projection metadata for the
  active `simple.vcd` reset waveform: A/B transition records cover `0-50s`,
  `50-150s`, and `150-250s`, with a public projection summary documenting the
  current `50/250*360=72px` and `100/250*360=144px` segment mapping.
- 2026-06-08: Transition/projection metadata verification passed:
  `cargo test -p boon_runtime novywave_ -- --nocapture`,
  `cargo check -p boon_native_playground`, and
  `cargo test -p boon_native_playground novywave_selected_visible_items_align_name_value_and_wave_columns -- --nocapture`.
- 2026-06-08: Re-ran
  `cargo test -p boon_native_playground novywave_dark_light_material_readbacks_cover_visual_regions -- --nocapture`
  after the transition/projection metadata change; the app-owned dark/light
  visual readback remained passing.
- 2026-06-08: Removed the duplicated reset-selected simple.vcd segment list.
  `selected_waveform_segments` now derives from the file-keyed
  `waveform_segment_records` list by filtering on `active_file` and mapping
  records through `new_waveform_segment`.
- 2026-06-08: Fixed the generic runtime path that blocked that projection:
  root `ListView` values now participate in summaries/dependencies without
  scalar root materialization, and record-shaped Boon function bodies now return
  records through generic `List/map`.
- 2026-06-08: File-keyed selected-segment projection verification passed:
  `cargo test -p boon_runtime novywave_waveform_metadata_drives_selected_file_and_timeline_window -- --nocapture`,
  `cargo test -p boon_runtime novywave_ -- --nocapture`,
  `cargo check -p boon_native_playground`,
  `cargo test -p boon_native_playground novywave_selected_visible_items_align_name_value_and_wave_columns -- --nocapture`,
  and
  `cargo test -p boon_native_playground novywave_dark_light_material_readbacks_cover_visual_regions -- --nocapture`.
  The IR dump now shows `waveform_segment_records` as the only waveform segment
  list memory and `store.selected_waveform_segments` as a `ListView`.
- 2026-06-08: Added numeric range/window/canvas metadata fields to the
  waveform file metadata records and numeric transition start/end fields to the
  simple.vcd segment records. Added generic `Number/project_width` and
  `Number/project_offset` runtime/typecheck/parser support as groundwork for
  replacing pre-sized segment widths with runtime time-to-pixel projection.
- 2026-06-08: Re-tested after the numeric projection groundwork:
  `cargo test -p boon_runtime novywave_ -- --nocapture`,
  `cargo check -p boon_native_playground`,
  `cargo test -p boon_native_playground novywave_selected_visible_items_align_name_value_and_wave_columns -- --nocapture`,
  and
  `cargo test -p boon_native_playground novywave_dark_light_material_readbacks_cover_visual_regions -- --nocapture`.
  The full selected-row transition-to-pixel mapper remains open because the
  current root `ListView` map path still needs hardening before it can carry the
  more complex projection expression without breaking summaries/zoom evidence.
- 2026-06-08: Removed preprojected `width` fields from all
  `waveform_segment_records`. The startup records now carry file, signal,
  start/end time, state, label, and click time; `new_waveform_segment` derives
  visible pixel widths at runtime with `Number/project_width` against the
  selected file's window/canvas metadata. Cursor-line x offset derives from the
  live cursor time so keyboard movement still changes the rendered cursor.
- 2026-06-08: Runtime and native verification passed after the projection
  change:
  `cargo test -p boon_runtime novywave_waveform_metadata_drives_selected_file_and_timeline_window -- --nocapture`,
  `cargo test -p boon_runtime novywave_ -- --nocapture`,
  `cargo check -p boon_native_playground`,
  `cargo test -p boon_native_playground novywave_selected_visible_items_align_name_value_and_wave_columns -- --nocapture`,
  `cargo test -p boon_native_playground novywave_search_and_waveform_keyboard_work_from_real_preview_input -- --nocapture`,
  and
  `cargo test -p boon_native_playground novywave_dark_light_material_readbacks_cover_visual_regions -- --nocapture`.
- 2026-06-08: Promoted NovyWave visual parity into a native GPU gate:
  `cargo xtask verify-native-gpu-novywave-visual --report target/reports/native-gpu/novywave-visual.json`.
  The report passed with app-owned WGPU readbacks, three promoted native tests,
  zero clipping failures, zero metric-threshold failures, footer/ruler
  separation passing, stale-viewport metrics passing, and runtime projection
  source-policy passing. The source-policy check rejects
  `simple_vcd_*_segment_width`, `width_source`, and preprojected `width` fields
  in `waveform_segment_records`.
- 2026-06-08: Compared the Rust NovyWave config/startup contract again and
  moved the Boon reset browser toward the same shape: one opened-file tree with
  file roots, nested scopes, selected startup file `simple.vcd`, expanded
  startup scopes `simple_tb.s` and `ghw.simple`, selected variables
  `A[3:0]`/`B[3:0]`, and runtime-projected timeline widths from selected file
  metadata.
- 2026-06-08: Fixed a generic runtime list-append/pass-through bug exposed by
  waveform marker insertion. Mapped rows whose field is declared as a child
  expression pass-through, such as `label: marker.label`, now preserve appended
  trigger text instead of recomputing to an empty string. The waveform click
  marker row now stores `150 s` in `store.markers[1].label`.
- 2026-06-08: Updated the file-tree view to avoid text identity checks for the
  simple scope row. The explicit simple-scope renderer branches on
  `Expanded`/`Collapsed` tags and renders `- simple_tb.s` in restored startup
  state; GHW expectations now match the restored expanded scope as
  `- ghw.simple`.
- 2026-06-08: Fresh verification after the startup tree/marker/runtime fixes
  passed:
  `cargo xtask verify-example-semantic novywave`,
  `cargo test -p boon_runtime source_const_text_appends_structural_list_row_from_ir -- --nocapture`,
  `cargo test -p boon_runtime novywave_ -- --nocapture`,
  `cargo check -p boon_native_playground`,
  `cargo test -p boon_native_playground novywave_controls_lower_hover_material_and_pointer_contracts -- --nocapture`,
  `cargo test -p boon_native_playground novywave_selected_visible_items_align_name_value_and_wave_columns -- --nocapture`,
  `cargo test -p boon_native_playground novywave_search_and_waveform_keyboard_work_from_real_preview_input -- --nocapture`,
  `cargo test -p boon_native_playground novywave_dark_light_material_readbacks_cover_visual_regions -- --nocapture`,
  `cargo xtask verify-native-gpu-novywave-visual --report target/reports/native-gpu/novywave-visual.json`,
  and
  `cargo xtask verify-native-gpu-preview-e2e --example novywave --report target/reports/native-gpu/preview-e2e-novywave.json`.
- 2026-06-08: Re-ran
  `cargo xtask verify-native-gpu-all --check-existing --report target/reports/native-gpu-all.json`
  after refreshing the NovyWave reports. The aggregate remained blocked by
  stale base native GPU reports and the existing failing/stale
  `scroll-speed-cells.json`; no NovyWave-specific blocker was reported.
- 2026-06-08: Wired `novywave-visual` into
  `verify-native-gpu-all --check-existing` required reports and added schema
  validation for `novywave_visual_spatial_evidence` on both the dedicated
  report and `preview-e2e-novywave`.
- 2026-06-08: Refreshed
  `cargo xtask verify-native-gpu-preview-e2e --example novywave --report target/reports/native-gpu/preview-e2e-novywave.json`.
  The new `novywave_visual_spatial_evidence` section passed, but the report
  still failed on older preview/scenario blockers: stale scenario coverage for
  marker/group actions, content expectations that were still tied to the older
  synthetic `top.cpu/data_bus` reset state, and preview runtime assertion
  interpretation around dynamic-layout source bindings. The scenario labels for
  current marker/group controls were updated from `[Jump]`, `[Del]`, and
  `[NewGrp]` to `Jump`, `Delete`, and `New group`, but the full semantic
  scenario remains stale because `initial-loaded-file` still expects the old
  `top_cpu` startup state while the current reset state uses `simple_tb_s`.
- 2026-06-08: Current aggregate status:
  `cargo xtask verify-native-gpu-all --check-existing --report target/reports/native-gpu-all.json`
  still fails. The dedicated `target/reports/native-gpu/novywave-visual.json`
  passes and is fresh; the remaining aggregate work is the stale
  `preview-e2e-novywave`/`novywave.scn` contract, not the visual spatial gate.

## Fixed

- Native live-event relayout keeps the current surface viewport instead of
  falling back to the source-path default viewport after clicks or keyboard
  interactions.
- Native scroll relayout keeps the current surface viewport when rebuilding a
  document window from runtime state.
- Root document children preserve `height: Fill` instead of being coerced to a
  fixed `720` height.
- The selected variables area no longer puts zoom, pan, cursor, marker, group,
  format, row, and analog controls in a large toolbar above the waveform.
- The selected variables area now exposes a stable name header, value header,
  waveform ruler header, bounded row body, and compact footer/status controls.
- Selected-variable rows now expose per-row remove controls in the name column.
- The selected variables body now renders sibling name, value, and wave columns
  from `selected_visible_items` instead of rendering each row as one fused
  name/value/wave stripe.
- Enum-like NovyWave state no longer uses `TEXT` literals in source comparisons;
  bare tags now carry the intended variant semantics.
- The generic runtime no longer immediately erases `AstExprKind::Enum` and
  `AstExprKind::Tag` into `BoonValue::Text`; it preserves them as enum values
  internally and serializes them compatibly at protocol boundaries.
- `selected_visible_items` now carries explicit mixed row state with
  `GroupHeader` and `VariableRow` item kinds rather than aliasing the flat
  selected-signal list.
- Group headers render inline across name/value/wave columns at the Rust
  contract height of 30px, without the variable-row divider.
- Collapsing the group hides grouped member rows inline while preserving the
  header; expanding restores them; removing the group removes the header while
  keeping the signal rows.
- Grouped variable rows now have additional name-column indentation, matching
  the Rust grouped-row visual contract.
- Active selected-row value cells now expose row-local digital format and
  analog limit actions wired to real Boon sources.
- Built-in bridge file identity, stats, range labels, timescale labels, and GHW
  request-window selection now flow through waveform metadata records rather
  than repeated hard-coded event branches.
- Selected variable rows now carry their source file and `remove_kind` metadata,
  making the row-local remove source a domain tag lookup rather than a text
  comparison or hard-coded fallback in the view.
- External file metadata host events are still routed via explicit sink fields,
  so the native host can publish bounded metadata without materializing waveform
  data into Boon state.
- The Files and Scopes browser now matches the Rust `TreeView` structure as one
  hierarchy: loaded file rows are roots, scope rows are children, and the CPU
  scope's signal summaries are nested under that scope instead of living in a
  separate Scopes section.
- The reset state now opens `simple.vcd` as the selected file, expands the
  `simple_tb.s` scope under that file, fills the Variables panel with
  `A[3:0]` and `B[3:0]`, preselects those same two variables, and shows their
  real simple.vcd cursor values (`0xc` and `0x5` at `50 s`) in the Selected
  Variables panel.
- The reset-state waveform view now uses the simple.vcd time unit and file
  range: axis/cursor/marker/window labels are seconds, and the visible A/B
  waveform segments represent the file transitions at `0 s`, `50 s`, `150 s`,
  and `250 s`.
- The active simple.vcd reset waveform now exposes transition/projection
  metadata in Boon state instead of leaving the segment math implicit. The
  rendered document primitives still consume width values, but those widths are
  now tied to a documented file-range projection from the real transition
  intervals.
- Visible waveform rows now derive from file-keyed all-file segment records
  instead of a duplicated reset-selected `simple.vcd` segment list.
- Visible waveform segment widths now derive from startup transition times plus
  selected file window/canvas metadata at runtime; source segment records no
  longer store precomputed pixel widths.
- NovyWave now has a promoted native GPU visual report,
  `verify-native-gpu-novywave-visual`, that proves app-owned WGPU readbacks,
  row alignment, clipping bounds, footer/ruler separation, stale viewport
  freshness, and runtime-projected waveform widths.
- Runtime list summaries reserve internal row identity under `$boon` instead
  of injecting `key` or `generation` beside user-authored fields. A Boon record
  may safely define a `key` field without colliding with runtime row identity.
- Viewport/request/response labels now derive from selected waveform metadata
  at runtime instead of label-specific tables or NovyModel shortcut helpers.
- Cursor and marker visuals now match the Rust canvas contract as full-height
  wave-surface overlays rather than row-local segment decorations.
- 2026-06-09: `simple_vcd_long_segment_width` / `simple_vcd_segment_width`
  are absent from Boon NovyWave source. `waveform_segment_records` store
  `start_time_value` / `end_time_value`; `new_waveform_segment` projects width
  from selected timeline metadata at runtime with `Number/project_width`.
- 2026-06-09: startup cursor values and axis ticks are now stored with
  per-file waveform metadata (`cursor_left`, `cursor_default`,
  `cursor_right`, `tick_0`..`tick_4`) and labels are composed from those
  numeric values plus the selected file unit. `cargo xtask
  verify-example-semantic novywave`, `cargo test -p boon_runtime novywave_`,
  and `cargo test -p boon_native_playground
  novywave_search_and_waveform_keyboard_work_from_real_preview_input` passed.
- 2026-06-09: fresh native NovyWave reports passed:
  `cargo xtask verify-native-gpu-novywave-visual --report
  target/reports/native-gpu/novywave-visual.json` and `cargo xtask
  verify-native-gpu-preview-e2e --example novywave --report
  target/reports/native-gpu/preview-e2e-novywave.json`.
- 2026-06-09: Moved runtime row identity out of user row summaries. Runtime
  summary rows now expose non-colliding internal metadata as
  `"$boon": { "row_key": ..., "generation": ... }`; user fields such as
  `key` remain pure Boon data. `cargo test -p boon_runtime
  list_user_key_fields_do_not_collide_with_runtime_identity -- --nocapture`
  and `cargo test -p boon_runtime
  list_filter_text_contains_and_join_field_project_derived_record_lists --
  --nocapture` passed.
- 2026-06-09: Replaced the remaining NovyWave label/window shortcuts with
  metadata-derived runtime labels. Viewport, request-window, response-window,
  cursor, marker, and timeline tick labels now compose from selected file
  range/window/cursor metadata through `Text/time_range_label` and
  `Number/project_*`; obsolete helpers `NovyModel/viewport_label_for_unit`,
  `NovyModel/page_window_label`, `Text/join3`, and
  `List/time_range_label_by_key3` are absent.
- 2026-06-09: Compared Rust
  `/home/martinkavik/repos/NovyWave/frontend/src/visualizer/canvas/rendering.rs`
  again and changed Boon cursor/marker rendering to full-height wave-surface
  overlays. Row-local `CursorLine`/`MarkerLine` segment records now return
  `NoElement`; the surface draws a full-height cursor overlay at
  `waveform_cursor_offset` and marker overlay at `waveform_marker_offset`.
- 2026-06-09: Fresh verification after the runtime namespace, metadata label,
  and full-height overlay changes passed:
  `cargo check -p boon_parser -p boon_typecheck -p boon_ir -p boon_runtime
  -p boon_native_playground -p xtask`,
  `cargo test -p boon_runtime
  list_user_key_fields_do_not_collide_with_runtime_identity -- --nocapture`,
  `cargo test -p boon_runtime
  list_filter_text_contains_and_join_field_project_derived_record_lists --
  --nocapture`,
  `cargo test -p boon_runtime
  novywave_waveform_metadata_drives_selected_file_and_timeline_window --
  --nocapture`,
  `cargo test -p boon_runtime novywave_ -- --nocapture`,
  `cargo xtask verify-example-semantic novywave`,
  `cargo test -p boon_native_playground novywave_ -- --nocapture`,
  `cargo xtask verify-native-gpu-novywave-visual --report
  target/reports/native-gpu/novywave-visual.json`,
  `cargo xtask verify-native-gpu-preview-e2e --example novywave --report
  target/reports/native-gpu/preview-e2e-novywave.json`, and
  `cargo xtask verify-native-gpu-scroll-speed --example novywave --report
  target/reports/native-gpu/scroll-speed-novywave.json`.

## Open Must-Fix Items

- None currently tracked for NovyWave visual/reference parity after the
  2026-06-09 native visual, preview E2E, scroll-speed, semantic, runtime, and
  compile verification batch.

## Rust Reference Contracts

- Reference image priority:
  - `/home/martinkavik/repos/NovyWave/docs/Tasks_9_and_11__media/novywave_full_ui.png`
    is the best all-around later full-UI reference.
  - `/home/martinkavik/repos/NovyWave/docs/Task 4 - media/docked_to_bottom.png`
    is the strongest bottom-docked selected variables workbench reference.
  - `/home/martinkavik/repos/NovyWave/docs/Task 5 - media/novywave_dark_linux.png`
    shows the optimized waveform viewer with row-aligned name/value/wave triplets.
  - `/home/martinkavik/repos/NovyWave/docs/Tasks_9_and_11__media/analog_waveform_tooltip.png`,
    `analog_limits_auto.png`, `analog_manual_applied.png`,
    `create_group_dialog.png`, and `markers_dialog_dark.png` are the specific
    references for analog, grouping, and marker workflows.
- `/home/martinkavik/repos/NovyWave/frontend/src/main.rs` defines the main
  layout as a fill-height, overflow-hidden shell. Bottom dock uses files and
  variables in the top row, a horizontal divider, then the selected variables
  panel as the fill-height bottom region. Right dock uses files over variables
  in the left column and selected variables as the fill-width right region.
- `/home/martinkavik/repos/NovyWave/frontend/src/selected_variables_panel.rs`
  splits selected variables into three sibling columns: name, value, and wave.
  Each column consumes the same visible item stream and row metrics.
- The Rust name column has per-row remove buttons, grouped indentation, group
  headers, and a footer with keyboard hints `Z W S R T`.
- The Rust value column has row-local digital format dropdowns, analog limit
  controls for real-valued rows, and a footer with `A Q E D` plus formatted
  viewport start/cursor/end times.
- `/home/martinkavik/repos/NovyWave/frontend/src/selected_variables_layout.rs`
  defines the shared row metric contract: group header height `30`, variable
  divider height `3`, and footer height `30`.
- `/home/martinkavik/repos/NovyWave/frontend/src/visualizer/canvas/rendering.rs`
  renders from `RenderingParameters`, not pre-sized UI chunks. Inputs include
  `viewport_start_ps`, `viewport_end_ps`, `cursor_position_ps`,
  `zoom_center_ps`, typed `RenderRowSnapshot` rows, and marker time/name data.
- Outside the selected-variable surface, Rust still has parity contracts Boon
  does not yet match: workspace bar/path input/theme toggle, persisted bottom
  vs right dock dimensions, continuous divider dragging with save-on-release,
  dynamic file/workspace picker dialogs, real variables search empty states,
  and shared panel styling.

## Intentional Differences

- Boon should keep using document primitives and native GPU material roles. Do
  not port Rust Fast2D, MoonZoon, Tauri, or DOM implementation code.
- Boon may approximate the continuous Rust canvas with structured document
  surfaces until the native document API grows a dedicated canvas-like primitive.
