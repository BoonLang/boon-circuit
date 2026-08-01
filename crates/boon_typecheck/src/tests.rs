use super::*;

fn found_payload_type(ty: &Type) -> Option<&Type> {
    let Type::VariantSet(variants) = ty else {
        return None;
    };
    variants.iter().find_map(|variant| match variant {
        Variant::Tagged { tag, fields } if tag == "Found" => fields.fields.get("value"),
        _ => None,
    })
}

// Typecheck tests are grouped by language surface while staying in this module for private helper access.
include!("tests/reactive_collections.rs");
include!("tests/flush.rs");
include!("tests/maps_sets.rs");
include!("tests/bits.rs");
include!("tests/numbers.rs");
include!("tests/pulses.rs");
include!("tests/styles.rs");
