// Retained quad tests are grouped by renderer behavior area while sharing
// private renderer helpers in this cfg(test) module.
include!("retained_quads/core_upload_ring.rs");
include!("retained_quads/scene_adaptation_and_world.rs");
include!("retained_quads/document_primitive_painting.rs");
include!("retained_quads/text_font_helpers.rs");
include!("retained_quads/assets_cache_and_dirty_uploads.rs");
