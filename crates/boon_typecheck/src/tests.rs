use super::*;

// Typecheck tests are grouped by language surface while staying in this module for private helper access.
include!("tests/host_ports.rs");
include!("tests/distributed.rs");
include!("tests/reactive_collections.rs");
include!("tests/calls.rs");
include!("tests/flush.rs");
include!("tests/maps_sets.rs");
include!("tests/bits.rs");
include!("tests/numbers.rs");
include!("tests/pulses.rs");
