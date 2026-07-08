// Included by `../machine_plan_backend_tests.rs`; kept in the parent test module for private invariant access.

// Nested behavior-area shards keep broad test groups navigable without widening production APIs.
include!("bytes/core_bytes_ops.rs");
include!("bytes/numeric_ops_and_verifier.rs");
include!("bytes/plan_identity_and_payload.rs");
include!("bytes/row_and_indexed_bytes.rs");
include!("bytes/search_and_encoding.rs");
include!("bytes/text_and_conversion.rs");
include!("bytes/verifier_tamper.rs");
