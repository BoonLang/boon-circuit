// Included by `../tests.rs`; kept in the parent test module for private IR helper access.

#[test]
fn stripe_view_binding_uses_neutral_kind_metadata() {
    assert_eq!(canonical_view_node_kind("Element/stripe"), "Stripe");
}

#[test]
fn output_roots_carry_typed_generic_contract_and_demand_metadata() {
    let visual = boon_parser::parse_source(
        "counter-output-root.bn",
        include_str!("../../../../examples/counter.bn"),
    )
    .unwrap();
    let visual = lower(&visual).unwrap();

    assert_eq!(visual.output_values.len(), 1);
    assert_eq!(visual.output_values[0].root, "document");
    assert_eq!(
        visual.output_values[0].contract,
        SemanticOutputContractKind::RetainedVisual {
            kind: SemanticRetainedVisualKind::Document,
        }
    );
    assert_eq!(
        visual.output_values[0].demand,
        SemanticOutputDemandPolicy::HostDemanded
    );

    let server = boon_parser::parse_source(
        "server-outputs.bn",
        include_str!("../../../../examples/server_outputs.bn"),
    )
    .unwrap();
    let server = lower(&server).unwrap();
    assert_eq!(
        server
            .output_values
            .iter()
            .map(|output| output.root.as_str())
            .collect::<Vec<_>>(),
        ["api_response", "pending_priorities"]
    );
    assert!(server.output_values.iter().all(|output| {
        output.contract == SemanticOutputContractKind::HostValue
            && output.demand == SemanticOutputDemandPolicy::HostDemanded
            && output.typed_contract_known
            && output.data_type.is_some()
    }));
    assert!(
        server
            .semantic_memory
            .iter()
            .all(|memory| !memory.identity.semantic_path.starts_with("outputs."))
    );
}
