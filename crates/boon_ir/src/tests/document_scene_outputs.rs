// Included by `../tests.rs`; kept in the parent test module for private IR helper access.

#[test]
fn stripe_view_binding_uses_neutral_kind_metadata() {
    assert_eq!(canonical_view_node_kind("Element/stripe"), "Stripe");
}
