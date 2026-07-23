use super::*;

fn empty_plan() -> MachinePlan {
    let application = ApplicationPlan::new(ApplicationIdentity::compiler_default()).unwrap();
    let persistence = PersistencePlan::new(
        &application,
        DEFAULT_PERSISTENCE_SCHEMA_VERSION,
        Vec::new(),
        Vec::new(),
        Vec::new(),
    )
    .unwrap();
    MachinePlan {
        version: PlanVersion::default(),
        target_profile: TargetProfile::SoftwareDefault,
        program_role: ProgramRole::Client,
        distributed_endpoint: None,
        producer_function_instances: Vec::new(),
        application,
        persistence,
        effects: Vec::new(),
        outputs: Vec::new(),
        host_ports: Vec::new(),
        list_indexes: Vec::new(),
        demand: DemandPlan {
            root_derived_outputs: RootOutputDemand::Selected(Vec::new()),
        },
        document: None,
        row_expressions: PlanRowExpressionArena::new(),
        constants: Vec::new(),
        source_routes: Vec::new(),
        storage_layout: StorageLayout {
            scalar_slots: Vec::new(),
            list_slots: Vec::new(),
            byte_banks: Vec::new(),
        },
        regions: Vec::new(),
        dirty_plan: DirtyPlan {
            dependency_edges: 0,
            unresolved_dependency_edges: 0,
        },
        commit_plan: CommitPlan {
            state_update_count: 0,
            unresolved_state_update_count: 0,
        },
        delta_plan: DeltaPlan { deltas: Vec::new() },
        capability_summary: CapabilitySummary {
            executable: true,
            typed_lowering_executable: true,
            cpu_plan_executor_complete: true,
            constant_count: 0,
            source_route_count: 0,
            scalar_storage_count: 0,
            list_storage_count: 0,
            byte_bank_storage_count: 0,
            operation_count: 0,
            typed_value_ref_count: 0,
            executable_string_path_count: 0,
            unresolved_executable_ref_count: 0,
            unknown_plan_op_count: 0,
            cpu_plan_executor_unsupported_op_count: 0,
            runtime_ast_dependency_count: 0,
            graph_rebuild_count: 0,
            graph_clones_per_item: 0,
        },
        debug_map: DebugMap {
            source_units: Vec::new(),
            source_routes: Vec::new(),
            state_slots: Vec::new(),
            list_slots: Vec::new(),
            derived_values: Vec::new(),
            fields: Vec::new(),
            unresolved_executable_refs: Vec::new(),
        },
    }
}

fn encoded_serde_string(value: &impl Serialize, expected: &str) {
    let mut encoded = vec![6];
    encoded.extend_from_slice(&(expected.len() as u64).to_le_bytes());
    encoded.extend_from_slice(expected.as_bytes());
    assert_eq!(super::binary::encode(value).unwrap(), encoded);
}

fn row_call_arg(name: &str, value: PlanRowExpressionId) -> PlanRowCallArg {
    PlanRowCallArg {
        name: name.to_owned(),
        value,
    }
}

fn row_call_args(names: &[&str], value: PlanRowExpressionId) -> Vec<PlanRowCallArg> {
    names.iter().map(|name| row_call_arg(name, value)).collect()
}

fn intern_empty_row_call(arena: &mut PlanRowExpressionArena) -> PlanRowExpressionId {
    arena
        .interner()
        .intern(PlanRowExpressionNode::BuiltinCall {
            function: PlanRowBuiltin::TextEmpty,
            input: None,
            args: Vec::new(),
        })
        .unwrap()
}

#[test]
fn plan_row_expression_arena_interns_shared_nodes_in_linear_storage() {
    let mut arena = PlanRowExpressionArena::new();
    let (source, trimmed, root) = {
        let mut builder = arena.builder();
        let source = builder.value(ValueRef::State(StateId(7))).unwrap();
        let trimmed = builder
            .intern(PlanRowExpressionNode::TextTrim { input: source })
            .unwrap();
        assert_eq!(
            builder
                .intern(PlanRowExpressionNode::TextTrim { input: source })
                .unwrap(),
            trimmed
        );
        let root = builder
            .intern(PlanRowExpressionNode::TextConcat {
                parts: vec![trimmed, trimmed],
            })
            .unwrap();
        (source, trimmed, root)
    };

    assert_eq!(arena.len(), 3);
    assert_eq!(
        arena.walk_postorder(root).unwrap(),
        vec![source, trimmed, root]
    );
    assert_eq!(
        arena.node(root).unwrap(),
        &PlanRowExpressionNode::TextConcat {
            parts: vec![trimmed, trimmed],
        }
    );
}

#[test]
fn plan_row_expression_arena_rejects_invalid_and_forward_ids() {
    let arena = PlanRowExpressionArena::new();
    assert!(
        arena
            .walk_postorder(PlanRowExpressionId(9))
            .unwrap_err()
            .to_string()
            .contains("invalid for arena length 0")
    );

    let forward = PlanRowExpressionArena::from_nodes(vec![
        PlanRowExpressionNode::TextTrim {
            input: PlanRowExpressionId(1),
        },
        PlanRowExpressionNode::Field {
            input: ValueRef::State(StateId(0)),
        },
    ])
    .unwrap_err();
    assert!(
        forward
            .to_string()
            .contains("children must exist and precede parents")
    );

    let mut arena = PlanRowExpressionArena::new();
    arena
        .push(PlanRowExpressionNode::Field {
            input: ValueRef::State(StateId(0)),
        })
        .unwrap();
    assert!(
        arena
            .push(PlanRowExpressionNode::TextTrim {
                input: PlanRowExpressionId(2),
            })
            .unwrap_err()
            .to_string()
            .contains("invalid future id 2")
    );
}

#[test]
fn plan_row_expression_arena_serde_and_plan_hash_are_deterministic() {
    let build = || {
        let mut arena = PlanRowExpressionArena::new();
        let source = arena.builder().value(ValueRef::State(StateId(3))).unwrap();
        let root = arena
            .builder()
            .intern(PlanRowExpressionNode::TextLength { input: source })
            .unwrap();
        (arena, root)
    };
    let (first, first_root) = build();
    let (second, second_root) = build();

    assert_eq!(first_root, second_root);
    assert_eq!(
        super::binary::encode(&first).unwrap(),
        super::binary::encode(&second).unwrap()
    );

    let mut first_plan = empty_plan();
    first_plan.row_expressions = first;
    let mut second_plan = empty_plan();
    second_plan.row_expressions = second;
    assert_eq!(
        plan_sha256(&first_plan).unwrap(),
        plan_sha256(&second_plan).unwrap()
    );

    let mut changed = first_plan.clone();
    changed
        .row_expressions
        .intern(PlanRowExpressionNode::TextIsEmpty { input: first_root })
        .unwrap();
    assert_ne!(
        plan_sha256(&first_plan).unwrap(),
        plan_sha256(&changed).unwrap()
    );
}

#[test]
fn plan_row_builtin_inventory_uses_stable_string_serde() {
    let expected_new_builtins = [
        ("Text/join_lines", PlanRowBuiltin::TextJoinLines),
        ("Text/to_uppercase", PlanRowBuiltin::TextToUppercase),
        ("Text/time_range_label", PlanRowBuiltin::TextTimeRangeLabel),
        ("Number/bit_width", PlanRowBuiltin::NumberBitWidth),
        ("Number/to_ascii_text", PlanRowBuiltin::NumberToAsciiText),
    ];
    let mut names = BTreeSet::new();
    for builtin in PlanRowBuiltin::ALL {
        let name = builtin.function_name();
        assert!(names.insert(name), "duplicate builtin name `{name}`");
        assert_eq!(PlanRowBuiltin::from_function_name(name), Some(*builtin));
        encoded_serde_string(builtin, name);
        let deserializer = serde::de::value::StrDeserializer::<serde::de::value::Error>::new(name);
        let decoded = <PlanRowBuiltin as serde::Deserialize>::deserialize(deserializer).unwrap();
        assert_eq!(decoded, *builtin);
    }
    for (name, builtin) in expected_new_builtins {
        assert_eq!(PlanRowBuiltin::from_function_name(name), Some(builtin));
    }
    for excluded in [
        "List/page",
        "Url/encode",
        "Text/trim",
        "Text/substring",
        "Text/to_bytes",
        "Bytes/to_text",
    ] {
        assert_eq!(PlanRowBuiltin::from_function_name(excluded), None);
    }
}

#[test]
fn plan_infix_op_inventory_uses_stable_operator_strings() {
    let expected = [
        ("+", PlanInfixOp::Add),
        ("-", PlanInfixOp::Subtract),
        ("*", PlanInfixOp::Multiply),
        ("/", PlanInfixOp::Divide),
        ("%", PlanInfixOp::Remainder),
        ("==", PlanInfixOp::Equal),
        ("!=", PlanInfixOp::NotEqual),
        ("<", PlanInfixOp::Less),
        ("<=", PlanInfixOp::LessOrEqual),
        (">", PlanInfixOp::Greater),
        (">=", PlanInfixOp::GreaterOrEqual),
    ];
    assert_eq!(PlanInfixOp::ALL.len(), expected.len());
    for (symbol, operator) in expected {
        assert_eq!(PlanInfixOp::from_symbol(symbol), Some(operator));
        assert_eq!(operator.as_str(), symbol);
        encoded_serde_string(&operator, symbol);
        let deserializer =
            serde::de::value::StrDeserializer::<serde::de::value::Error>::new(symbol);
        let decoded = <PlanInfixOp as serde::Deserialize>::deserialize(deserializer).unwrap();
        assert_eq!(decoded, operator);
        assert_eq!(
            operator.is_comparison(),
            matches!(
                operator,
                PlanInfixOp::Equal
                    | PlanInfixOp::NotEqual
                    | PlanInfixOp::Less
                    | PlanInfixOp::LessOrEqual
                    | PlanInfixOp::Greater
                    | PlanInfixOp::GreaterOrEqual
            )
        );
    }
    assert_eq!(PlanInfixOp::from_symbol("&&"), None);
}

#[test]
fn plan_row_builtin_signatures_are_complete_and_queryable() {
    for builtin in PlanRowBuiltin::ALL {
        let signature = builtin.signature();
        let mut names = BTreeSet::new();
        let mut receiver_count = 0;
        for parameter in signature.parameters {
            assert!(
                names.insert(parameter.name),
                "{} repeats parameter `{}`",
                builtin,
                parameter.name
            );
            assert_eq!(builtin.parameter(parameter.name), Some(parameter));
            assert_ne!(parameter.is_required(), parameter.is_optional());
            receiver_count += usize::from(parameter.is_receiver());
        }
        assert!(receiver_count <= 1, "{} has multiple receivers", builtin);
        assert_eq!(builtin.receiver_parameter(), signature.receiver_parameter());
        assert!(
            builtin.fixed_result_type().is_some()
                || matches!(
                    builtin,
                    PlanRowBuiltin::ListGet | PlanRowBuiltin::ListLatest | PlanRowBuiltin::ListTake
                ),
            "{} lacks fixed result metadata",
            builtin
        );
    }

    let receiver = PlanRowBuiltin::TextTimeRangeLabel
        .receiver_parameter()
        .unwrap();
    assert_eq!(receiver.name, "input");
    assert!(receiver.required);
    assert!(
        PlanRowBuiltin::NumberToAsciiText
            .parameter("width")
            .unwrap()
            .is_optional()
    );
    assert_eq!(
        PlanRowBuiltin::TextJoinLines.fixed_result_type(),
        Some(PlanValueType::Text)
    );
    assert_eq!(
        PlanRowBuiltin::NumberBitWidth.fixed_result_type(),
        Some(PlanValueType::Number)
    );
}

#[test]
fn plan_row_builtin_call_validation_enforces_canonical_shape() {
    let mut arena = PlanRowExpressionArena::new();
    let input = intern_empty_row_call(&mut arena);
    PlanRowBuiltin::TextContains
        .validate_call(Some(input), &row_call_args(&["needle"], input))
        .unwrap();
    PlanRowBuiltin::TextJoin
        .validate_call(Some(input), &row_call_args(&["separator", "empty"], input))
        .unwrap();
    PlanRowBuiltin::NumberInterpolate
        .validate_call(
            None,
            &row_call_args(
                &["start", "end", "numerator", "denominator", "fallback"],
                input,
            ),
        )
        .unwrap();

    let missing_input = PlanRowBuiltin::TextContains
        .validate_call(None, &row_call_args(&["needle"], input))
        .unwrap_err();
    assert!(missing_input.to_string().contains("required input `input`"));

    let named_receiver = PlanRowBuiltin::TextContains
        .validate_call(None, &row_call_args(&["input", "needle"], input))
        .unwrap_err();
    assert!(
        named_receiver
            .to_string()
            .contains("must be stored as input")
    );

    let duplicate_receiver = PlanRowBuiltin::TextContains
        .validate_call(Some(input), &row_call_args(&["input", "needle"], input))
        .unwrap_err();
    assert!(
        duplicate_receiver
            .to_string()
            .contains("duplicates its input")
    );

    let duplicate = PlanRowBuiltin::TextContains
        .validate_call(Some(input), &row_call_args(&["needle", "needle"], input))
        .unwrap_err();
    assert!(
        duplicate
            .to_string()
            .contains("duplicate argument `needle`")
    );

    let unknown = PlanRowBuiltin::TextContains
        .validate_call(Some(input), &row_call_args(&["needle", "extra"], input))
        .unwrap_err();
    assert!(unknown.to_string().contains("unknown argument `extra`"));

    let unexpected_input = PlanRowBuiltin::TextEmpty
        .validate_call(Some(input), &[])
        .unwrap_err();
    assert!(
        unexpected_input
            .to_string()
            .contains("does not accept an input")
    );
}

#[test]
fn plan_verifier_rejects_malformed_row_builtin_calls() {
    let mut plan = empty_plan();
    let input = intern_empty_row_call(&mut plan.row_expressions);
    let expression = plan
        .row_expression_builder()
        .intern(PlanRowExpressionNode::BuiltinCall {
            function: PlanRowBuiltin::TextContains,
            input: Some(input),
            args: row_call_args(&["needle", "needle"], input),
        })
        .unwrap();
    plan.storage_layout.scalar_slots.push(ScalarStorageSlot {
        id: PlanStorageId(0),
        state_id: StateId(0),
        owner: PlanOwner::root(),
        value_type: PlanValueType::Bool,
        scope_id: None,
        indexed: false,
        indexed_field_id: None,
        initializer: ScalarInitializerPlan::Expression { expression },
    });

    let error = validate_plan_row_builtin_calls(&plan).unwrap_err();
    assert!(error.to_string().contains("duplicate argument `needle`"));
    let verification = verify_plan(&plan).unwrap();
    let check = verification
        .checks
        .iter()
        .find(|check| check.id == "row-builtin-calls-match-signatures")
        .unwrap();
    assert!(!check.pass);
    assert!(check.detail.contains("duplicate argument `needle`"));
}

fn empty_indexed_list_plan(profile: TargetProfile, capacity: Option<usize>) -> MachinePlan {
    let mut plan = empty_plan();
    plan.target_profile = profile;
    plan.storage_layout.list_slots.push(ListStorageSlot {
        id: PlanStorageId(0),
        list_id: ListId(0),
        scope_id: None,
        row_fields: Vec::new(),
        capacity,
        hidden_key_type: "u64".to_owned(),
        has_generation: true,
        initializer_kind: ListInitializerKind::Empty,
        range: None,
        initial_rows: Vec::new(),
    });
    plan
}

#[test]
fn typed_index_resource_validation_rejects_inventory_and_capacity_over_profile() {
    let mut inventory = empty_indexed_list_plan(TargetProfile::SoftwareBounded, None);
    inventory.list_indexes = (0..65)
        .map(|index| PlanListIndex {
            id: PlanListIndexId(index),
            source_list: ListId(0),
            keys: Vec::new(),
        })
        .collect();
    assert!(
        validate_typed_list_index_resources(&inventory)
            .unwrap_err()
            .to_string()
            .contains("at most 64 typed indexes")
    );

    let mut capacity = empty_indexed_list_plan(TargetProfile::SoftwareBounded, Some(100_001));
    capacity.list_indexes.push(PlanListIndex {
        id: PlanListIndexId(0),
        source_list: ListId(0),
        keys: Vec::new(),
    });
    assert!(
        validate_typed_list_index_resources(&capacity)
            .unwrap_err()
            .to_string()
            .contains("may retain 100001 entries")
    );
}

fn distributed_declaration(semantic_path: &str) -> DistributedDeclarationId {
    DistributedDeclarationId::from_semantic_path("DistributedFixture", semantic_path).unwrap()
}

fn distributed_graph_fixture() -> (
    ApplicationIdentity,
    DistributedGraphPlan,
    PlanRowExpressionArena,
) {
    let application = ApplicationIdentity::compiler_default();
    let graph =
        DistributedGraphIdentityPlan::new(&application, distributed_declaration("graph"), 1)
            .unwrap();

    let server_declaration = distributed_declaration("endpoint.server");
    let server_endpoint_id = DistributedEndpointId::from_identity(
        graph.graph_id,
        ProgramRole::Server,
        server_declaration,
    )
    .unwrap();
    let server_value = DistributedValueExportPlan::new(
        graph.graph_id,
        server_endpoint_id,
        distributed_declaration("server.value.count"),
        1,
        ProgramRole::Server,
        false,
        ValueRef::Constant(PlanConstantId(0)),
        DataTypePlan::Number,
    )
    .unwrap();
    let function_declaration = distributed_declaration("server.function.double");
    let server_function = DistributedFunctionExportPlan::new(
        graph.graph_id,
        server_endpoint_id,
        function_declaration,
        1,
        ProgramRole::Server,
        vec![("value".to_owned(), DataTypePlan::Number)],
        DataTypePlan::Number,
    )
    .unwrap();
    let server = DistributedEndpointContractPlan::new(
        &graph,
        server_declaration,
        1,
        ProgramRole::Server,
        vec![server_value.clone()],
        Vec::new(),
        Vec::new(),
        Vec::new(),
        vec![server_function.clone()],
        Vec::new(),
    )
    .unwrap();

    let session_declaration = distributed_declaration("endpoint.session");
    let session_endpoint_id = DistributedEndpointId::from_identity(
        graph.graph_id,
        ProgramRole::Session,
        session_declaration,
    )
    .unwrap();
    let session_import = DistributedValueImportPlan::new(
        graph.graph_id,
        session_endpoint_id,
        distributed_declaration("session.import.server_count"),
        1,
        ProgramRole::Session,
        &server_value,
    )
    .unwrap();
    let session_function_declaration = distributed_declaration("session.function.double");
    let session_function = DistributedFunctionExportPlan::new(
        graph.graph_id,
        session_endpoint_id,
        session_function_declaration,
        1,
        ProgramRole::Session,
        vec![("value".to_owned(), DataTypePlan::Number)],
        DataTypePlan::Number,
    )
    .unwrap();
    let session_value = DistributedValueExportPlan::new(
        graph.graph_id,
        session_endpoint_id,
        distributed_declaration("session.value.server_count"),
        1,
        ProgramRole::Session,
        false,
        ValueRef::DistributedImport(session_import.import_id),
        DataTypePlan::Number,
    )
    .unwrap();
    let session = DistributedEndpointContractPlan::new(
        &graph,
        session_declaration,
        1,
        ProgramRole::Session,
        vec![session_value.clone()],
        vec![session_import],
        Vec::new(),
        Vec::new(),
        vec![session_function.clone()],
        Vec::new(),
    )
    .unwrap();

    let client_declaration = distributed_declaration("endpoint.client");
    let client_endpoint_id = DistributedEndpointId::from_identity(
        graph.graph_id,
        ProgramRole::Client,
        client_declaration,
    )
    .unwrap();
    let client_import = DistributedValueImportPlan::new(
        graph.graph_id,
        client_endpoint_id,
        distributed_declaration("client.import.session_count"),
        1,
        ProgramRole::Client,
        &session_value,
    )
    .unwrap();
    let mut row_expressions = PlanRowExpressionArena::new();
    let client_argument = row_expressions
        .builder()
        .value(ValueRef::DistributedImport(client_import.import_id))
        .unwrap();
    let client_call = RemoteCallSitePlan::new(
        graph.graph_id,
        client_endpoint_id,
        distributed_declaration("client.call.double"),
        1,
        ProgramRole::Client,
        PlanOwner::root(),
        &session_function,
        vec![("value".to_owned(), client_argument)],
        Vec::new(),
        DistributedCallMode::Current,
        None,
        Vec::new(),
    )
    .unwrap();
    let client = DistributedEndpointContractPlan::new(
        &graph,
        client_declaration,
        1,
        ProgramRole::Client,
        Vec::new(),
        vec![client_import],
        Vec::new(),
        Vec::new(),
        Vec::new(),
        vec![client_call],
    )
    .unwrap();

    let graph =
        DistributedGraphPlan::new(&application, graph, vec![server, client, session]).unwrap();
    (application, graph, row_expressions)
}

fn session_producer_plan(graph: &DistributedGraphPlan) -> MachinePlan {
    let endpoint = graph.endpoint_plan(ProgramRole::Session).unwrap();
    let edge = endpoint
        .wire_schema
        .call_edges
        .iter()
        .find(|edge| edge.callee_role == ProgramRole::Session)
        .unwrap();
    let function = endpoint
        .endpoint
        .function_exports
        .iter()
        .find(|function| function.export_id == edge.function_export_id)
        .unwrap();
    let result_import =
        ImportId::from_producer_argument(edge.call_site_id, function.parameters[0].argument_id)
            .unwrap();
    let instance = ProducerFunctionInstancePlan::new(
        edge.call_site_id,
        function,
        PlanOwner {
            static_owner: PlanStaticOwnerId(0),
            ancestors: Vec::new(),
        },
        DistributedCallMode::Current,
        None,
        ProducerFunctionOwnershipPlan::new(
            vec![PlanStaticOwnerId(0)],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        ),
        ValueRef::DistributedImport(result_import),
    )
    .unwrap();

    let mut plan = empty_plan();
    plan.program_role = ProgramRole::Session;
    plan.distributed_endpoint = Some(endpoint);
    plan.producer_function_instances = vec![instance];
    plan
}

fn session_producer_plan_with_owned_resources(graph: &DistributedGraphPlan) -> MachinePlan {
    let mut plan = session_producer_plan(graph);
    let instance_owner = PlanStaticOwnerId(0);
    let resource_owner = PlanStaticOwnerId(1);
    let source_id = SourceId(0);
    let state_id = StateId(0);
    let field_id = FieldId(0);
    let list_id = ListId(0);
    let index_id = PlanListIndexId(0);
    let effect_id = EffectId::from_host_operation("Test/producer_owned").unwrap();
    let invocation_id =
        EffectInvocationId::from_result_owner(effect_id, "producer.result").unwrap();
    let argument_import = plan.producer_function_instances[0].arguments[0].import_id;
    let argument_expression = plan
        .row_expression_builder()
        .value(ValueRef::DistributedImport(argument_import))
        .unwrap();
    let resource_plan_owner = PlanOwner {
        static_owner: resource_owner,
        ancestors: Vec::new(),
    };

    plan.source_routes.push(SourceRoute {
        id: PlanSourceRouteId(0),
        source_id,
        owner: resource_plan_owner.clone(),
        path: "producer.source".to_owned(),
        scoped: false,
        scope_id: None,
        interval_ms: None,
        payload_schema: SourcePayloadSchema {
            fields: Vec::new(),
            typed_fields: Vec::new(),
        },
    });
    plan.storage_layout.scalar_slots.push(ScalarStorageSlot {
        id: PlanStorageId(0),
        state_id,
        owner: resource_plan_owner.clone(),
        value_type: PlanValueType::Number,
        scope_id: None,
        indexed: false,
        indexed_field_id: None,
        initializer: ScalarInitializerPlan::Expression {
            expression: argument_expression,
        },
    });
    plan.storage_layout.list_slots.push(ListStorageSlot {
        id: PlanStorageId(1),
        list_id,
        scope_id: None,
        row_fields: Vec::new(),
        capacity: None,
        hidden_key_type: "u64".to_owned(),
        has_generation: true,
        initializer_kind: ListInitializerKind::Empty,
        range: None,
        initial_rows: Vec::new(),
    });
    plan.list_indexes.push(PlanListIndex {
        id: index_id,
        source_list: list_id,
        keys: Vec::new(),
    });
    plan.regions.push(OperationRegion {
        id: PlanRegionId(0),
        kind: RegionKind::DerivedEvaluation,
        ops: vec![PlanOp {
            id: PlanOpId(0),
            kind: PlanOpKind::DerivedValue {
                derived_kind: PlanDerivedKind::Pure,
                startup_recompute: true,
                expression: None,
            },
            inputs: vec![ValueRef::DistributedImport(argument_import)],
            output: Some(ValueRef::Field(field_id)),
            indexed: false,
            unresolved_executable_ref_count: 0,
        }],
    });
    plan.regions.push(OperationRegion {
        id: PlanRegionId(1),
        kind: RegionKind::StateUpdates,
        ops: vec![PlanOp {
            id: PlanOpId(1),
            kind: PlanOpKind::StateUpdate {
                trigger: ValueRef::DistributedImport(argument_import),
                value: None,
                effect: Some(EffectInvocationPlan {
                    invocation_id,
                    effect_id,
                    owner: resource_plan_owner,
                    gate: argument_expression,
                    intent_fields: Vec::new(),
                    idempotency_key: EffectIdempotencyKeyPlan::InvocationTurnIntentSha256,
                    result: EffectResultRoute::Target {
                        target: ValueRef::State(state_id),
                        policy: EffectResultPolicy::ReturnValue,
                    },
                    barrier: EffectBarrier::None,
                }),
            },
            inputs: vec![ValueRef::DistributedImport(argument_import)],
            output: Some(ValueRef::State(state_id)),
            indexed: false,
            unresolved_executable_ref_count: 0,
        }],
    });
    plan.producer_function_instances[0].ownership = ProducerFunctionOwnershipPlan::new(
        vec![resource_owner, instance_owner, resource_owner],
        vec![source_id],
        vec![state_id],
        vec![field_id],
        vec![list_id],
        vec![index_id],
        vec![invocation_id],
    );
    plan.producer_function_instances[0].result = ValueRef::Field(field_id);
    plan
}

fn add_second_session_producer_instance(
    plan: &mut MachinePlan,
    ownership: ProducerFunctionOwnershipPlan,
) {
    let endpoint = plan.distributed_endpoint.as_ref().unwrap();
    let function = endpoint.endpoint.function_exports[0].clone();
    let call_site_id = RemoteCallSiteId::from_identity(
        endpoint.graph.graph_id,
        endpoint.endpoint.endpoint_id,
        distributed_declaration("session.call.second_producer"),
    )
    .unwrap();
    let mut edge = endpoint
        .wire_schema
        .call_edges
        .iter()
        .find(|edge| edge.callee_role == ProgramRole::Session)
        .unwrap()
        .clone();
    edge.call_site_id = call_site_id;
    let result_import =
        ImportId::from_producer_argument(call_site_id, function.parameters[0].argument_id).unwrap();
    let instance = ProducerFunctionInstancePlan::new(
        call_site_id,
        &function,
        PlanOwner {
            static_owner: PlanStaticOwnerId(2),
            ancestors: Vec::new(),
        },
        DistributedCallMode::Current,
        None,
        ownership,
        ValueRef::DistributedImport(result_import),
    )
    .unwrap();

    let endpoint = plan.distributed_endpoint.as_mut().unwrap();
    endpoint.wire_schema.call_edges.push(edge);
    endpoint
        .wire_schema
        .call_edges
        .sort_by_key(|edge| edge.call_site_id);
    plan.producer_function_instances.push(instance);
    plan.producer_function_instances
        .sort_by_key(|instance| instance.call_site_id);
}

fn producer_function_check(plan: &MachinePlan) -> (bool, String) {
    let verification = verify_plan(plan).unwrap();
    let check = verification
        .checks
        .iter()
        .find(|check| check.id == "producer-function-instances-canonical-and-resolved")
        .unwrap();
    (check.pass, check.detail.clone())
}

fn distributed_graph_with_event(
    application: &ApplicationIdentity,
    base: &DistributedGraphPlan,
    export_source_id: SourceId,
    import_source_id: SourceId,
    payload_type: DataTypePlan,
) -> DistributedGraphPlan {
    let mut endpoints = base.endpoints.clone();
    let session_index = endpoints
        .iter()
        .position(|endpoint| endpoint.role == ProgramRole::Session)
        .unwrap();
    let client_index = endpoints
        .iter()
        .position(|endpoint| endpoint.role == ProgramRole::Client)
        .unwrap();
    let event_export = DistributedEventExportPlan::new(
        base.graph.graph_id,
        endpoints[session_index].endpoint_id,
        distributed_declaration("session.event.changed"),
        1,
        ProgramRole::Session,
        export_source_id,
        Some(SourcePayloadField::Named("payload".to_owned())),
        payload_type,
    )
    .unwrap();
    let event_import = DistributedEventImportPlan::new(
        base.graph.graph_id,
        endpoints[client_index].endpoint_id,
        distributed_declaration("client.import.session_changed"),
        1,
        ProgramRole::Client,
        &event_export,
        import_source_id,
    )
    .unwrap();
    endpoints[session_index].event_exports.push(event_export);
    endpoints[client_index].event_imports.push(event_import);
    DistributedGraphPlan::new(application, base.graph.clone(), endpoints).unwrap()
}

#[test]
fn distributed_graph_links_three_roles_and_each_machine_passes_the_distributed_check() {
    let (application, graph, row_expressions) = distributed_graph_fixture();

    assert!(graph.validate(&application).is_ok());
    assert_eq!(
        graph
            .endpoints
            .iter()
            .map(|endpoint| endpoint.role)
            .collect::<Vec<_>>(),
        [
            ProgramRole::Client,
            ProgramRole::Session,
            ProgramRole::Server,
        ]
    );

    for role in [
        ProgramRole::Client,
        ProgramRole::Session,
        ProgramRole::Server,
    ] {
        let mut plan = empty_plan();
        plan.program_role = role;
        plan.distributed_endpoint = graph.endpoint_plan(role);
        plan.row_expressions = row_expressions.clone();

        let verification = verify_plan(&plan).unwrap();
        let distributed_check = verification
            .checks
            .iter()
            .find(|check| check.id == "distributed-endpoint-canonical-and-resolved")
            .unwrap();
        assert!(distributed_check.pass, "{}", distributed_check.detail);
    }

    assert_eq!(
        graph.wire_schema_hash,
        distributed_graph_schema_hash(&graph).unwrap()
    );
    let endpoint_plans = [
        ProgramRole::Client,
        ProgramRole::Session,
        ProgramRole::Server,
    ]
    .map(|role| graph.endpoint_plan(role).unwrap());
    assert!(endpoint_plans.iter().all(|endpoint| {
        endpoint.wire_schema == graph.wire_schema
            && endpoint.wire_schema_hash == graph.wire_schema_hash
    }));
    for edge in &graph.wire_schema.value_edges {
        let consumer = endpoint_plans
            .iter()
            .find(|endpoint| endpoint.endpoint.role == edge.consumer_role)
            .unwrap();
        assert_eq!(consumer.value_import_route(edge.import_id), Some(edge));
    }
    for edge in &graph.wire_schema.call_edges {
        let caller = endpoint_plans
            .iter()
            .find(|endpoint| endpoint.endpoint.role == edge.caller_role)
            .unwrap();
        let callee = endpoint_plans
            .iter()
            .find(|endpoint| endpoint.endpoint.role == edge.callee_role)
            .unwrap();
        assert_eq!(caller.outbound_call_route(edge.call_site_id), Some(edge));
        assert_eq!(callee.inbound_call_route(edge.call_site_id), Some(edge));
        assert!(callee.endpoint.function_exports.iter().any(|function| {
            function.export_id == edge.function_export_id
                && function.parameters == edge.parameters
                && function.result_type == edge.result_type
        }));
    }
}

#[test]
fn producer_function_ownership_constructor_canonicalizes_every_id_set() {
    let low_effect = EffectInvocationId([1; 32]);
    let high_effect = EffectInvocationId([2; 32]);
    let ownership = ProducerFunctionOwnershipPlan::new(
        vec![
            PlanStaticOwnerId(2),
            PlanStaticOwnerId(1),
            PlanStaticOwnerId(2),
        ],
        vec![SourceId(2), SourceId(1), SourceId(2)],
        vec![StateId(2), StateId(1), StateId(2)],
        vec![FieldId(2), FieldId(1), FieldId(2)],
        vec![ListId(2), ListId(1), ListId(2)],
        vec![PlanListIndexId(2), PlanListIndexId(1), PlanListIndexId(2)],
        vec![high_effect, low_effect, high_effect],
    );

    assert_eq!(
        ownership.static_owners,
        vec![PlanStaticOwnerId(1), PlanStaticOwnerId(2)]
    );
    assert_eq!(ownership.sources, vec![SourceId(1), SourceId(2)]);
    assert_eq!(ownership.states, vec![StateId(1), StateId(2)]);
    assert_eq!(ownership.fields, vec![FieldId(1), FieldId(2)]);
    assert_eq!(ownership.lists, vec![ListId(1), ListId(2)]);
    assert_eq!(
        ownership.indexes,
        vec![PlanListIndexId(1), PlanListIndexId(2)]
    );
    assert_eq!(ownership.effects, vec![low_effect, high_effect]);
}

#[test]
fn producer_function_instance_uses_signature_only_exports_and_canonical_argument_imports() {
    let (_, graph, _) = distributed_graph_fixture();
    let plan = session_producer_plan(&graph);
    let instance = &plan.producer_function_instances[0];
    let argument = &instance.arguments[0];

    assert_eq!(
        argument.import_id,
        ImportId::from_producer_argument(instance.call_site_id, argument.argument_id).unwrap()
    );
    assert_eq!(argument.data_type, DataTypePlan::Number);
    assert_eq!(instance.result_type, DataTypePlan::Number);

    let verification = verify_plan(&plan).unwrap();
    let check = verification
        .checks
        .iter()
        .find(|check| check.id == "producer-function-instances-canonical-and-resolved")
        .unwrap();
    assert!(check.pass, "{}", check.detail);
}

#[test]
fn producer_function_ownership_resolves_real_plan_resources_and_results() {
    let (_, graph, _) = distributed_graph_fixture();
    let plan = session_producer_plan_with_owned_resources(&graph);
    let (pass, detail) = producer_function_check(&plan);
    assert!(pass, "{detail}");

    let mut missing_field_result = plan.clone();
    missing_field_result.producer_function_instances[0]
        .ownership
        .fields
        .clear();
    let (pass, detail) = producer_function_check(&missing_field_result);
    assert!(!pass);
    assert!(detail.contains("field result"), "{detail}");

    let mut list_result = plan;
    let list_type = DataTypePlan::List {
        item: Box::new(DataTypePlan::Number),
    };
    let instance = &mut list_result.producer_function_instances[0];
    instance.result = ValueRef::List(ListId(0));
    instance.result_type = list_type.clone();
    let endpoint = list_result.distributed_endpoint.as_mut().unwrap();
    endpoint.endpoint.function_exports[0].result_type = list_type.clone();
    endpoint
        .wire_schema
        .call_edges
        .iter_mut()
        .find(|edge| edge.callee_role == ProgramRole::Session)
        .unwrap()
        .result_type = list_type;
    let (pass, detail) = producer_function_check(&list_result);
    assert!(pass, "{detail}");

    list_result.producer_function_instances[0]
        .ownership
        .lists
        .clear();
    let (pass, detail) = producer_function_check(&list_result);
    assert!(!pass);
    assert!(detail.contains("list result"), "{detail}");
}

#[test]
fn producer_function_ownership_rejects_noncanonical_and_missing_ids() {
    let (_, graph, _) = distributed_graph_fixture();
    let plan = session_producer_plan_with_owned_resources(&graph);

    let mut noncanonical = plan.clone();
    noncanonical.producer_function_instances[0].ownership.states = vec![StateId(0), StateId(0)];
    let (pass, detail) = producer_function_check(&noncanonical);
    assert!(!pass);
    assert!(
        detail.contains("unique and canonically ordered"),
        "{detail}"
    );

    let mut wrong_first_owner = plan.clone();
    wrong_first_owner.producer_function_instances[0]
        .ownership
        .static_owners = vec![PlanStaticOwnerId(1)];
    let (pass, detail) = producer_function_check(&wrong_first_owner);
    assert!(!pass);
    assert!(
        detail.contains("begin with the instance static owner"),
        "{detail}"
    );

    let mut root_owner = plan.clone();
    root_owner.producer_function_instances[0]
        .ownership
        .static_owners
        .push(PlanStaticOwnerId::ROOT);
    let (pass, detail) = producer_function_check(&root_owner);
    assert!(!pass);
    assert!(detail.contains("ROOT static owner"), "{detail}");

    let assert_missing = |plan: &MachinePlan, expected: &str| {
        let (pass, detail) = producer_function_check(plan);
        assert!(!pass);
        assert!(
            detail.contains(expected),
            "expected `{expected}` in `{detail}`"
        );
    };

    let mut missing_owner = plan.clone();
    missing_owner.producer_function_instances[0]
        .ownership
        .static_owners
        .push(PlanStaticOwnerId(3));
    assert_missing(&missing_owner, "missing static owner 3");

    let mut missing_source = plan.clone();
    missing_source.producer_function_instances[0]
        .ownership
        .sources
        .push(SourceId(1));
    assert_missing(&missing_source, "missing source 1");

    let mut missing_state = plan.clone();
    missing_state.producer_function_instances[0]
        .ownership
        .states
        .push(StateId(1));
    assert_missing(&missing_state, "missing state 1");

    let mut missing_field = plan.clone();
    missing_field.producer_function_instances[0]
        .ownership
        .fields
        .push(FieldId(1));
    assert_missing(&missing_field, "missing field 1");

    let mut missing_list = plan.clone();
    missing_list.producer_function_instances[0]
        .ownership
        .lists
        .push(ListId(1));
    assert_missing(&missing_list, "missing list 1");

    let mut missing_index = plan.clone();
    missing_index.producer_function_instances[0]
        .ownership
        .indexes
        .push(PlanListIndexId(1));
    assert_missing(&missing_index, "missing list index 1");

    let mut missing_effect = plan;
    missing_effect.producer_function_instances[0]
        .ownership
        .effects
        .push(EffectInvocationId([255; 32]));
    missing_effect.producer_function_instances[0]
        .ownership
        .effects
        .sort();
    assert_missing(&missing_effect, "missing effect invocation");
}

#[test]
fn producer_function_ownership_is_disjoint_across_distinct_instances() {
    let (_, graph, _) = distributed_graph_fixture();
    for category in [
        "static owner",
        "source",
        "state",
        "field",
        "list",
        "list index",
        "effect invocation",
    ] {
        let mut plan = session_producer_plan_with_owned_resources(&graph);
        let mut second_ownership = ProducerFunctionOwnershipPlan::new(
            vec![PlanStaticOwnerId(2)],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        match category {
            "static owner" => {
                plan.producer_function_instances[0]
                    .ownership
                    .static_owners
                    .push(PlanStaticOwnerId(2));
            }
            "source" => second_ownership.sources.push(SourceId(0)),
            "state" => second_ownership.states.push(StateId(0)),
            "field" => second_ownership.fields.push(FieldId(0)),
            "list" => second_ownership.lists.push(ListId(0)),
            "list index" => second_ownership.indexes.push(PlanListIndexId(0)),
            "effect invocation" => {
                second_ownership
                    .effects
                    .push(plan.producer_function_instances[0].ownership.effects[0]);
            }
            _ => unreachable!(),
        }
        add_second_session_producer_instance(&mut plan, second_ownership);

        let (pass, detail) = producer_function_check(&plan);
        assert!(!pass);
        assert!(
            detail.contains(&format!("overlap on {category}")),
            "{category}: {detail}"
        );
    }
}

#[test]
fn producer_function_instance_validation_rejects_noncanonical_ownership_and_arguments() {
    let (_, graph, _) = distributed_graph_fixture();
    let endpoint = graph.endpoint_plan(ProgramRole::Session).unwrap();
    let edge = endpoint
        .wire_schema
        .call_edges
        .iter()
        .find(|edge| edge.callee_role == ProgramRole::Session)
        .unwrap();
    let function = endpoint
        .endpoint
        .function_exports
        .iter()
        .find(|function| function.export_id == edge.function_export_id)
        .unwrap();

    assert!(
        ProducerFunctionInstancePlan::new(
            edge.call_site_id,
            function,
            PlanOwner::root(),
            DistributedCallMode::Current,
            None,
            ProducerFunctionOwnershipPlan::new(
                vec![PlanStaticOwnerId::ROOT],
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ),
            ValueRef::Constant(PlanConstantId(0)),
        )
        .is_err()
    );
    assert!(
        ImportId::from_producer_argument(
            RemoteCallSiteId([0; 32]),
            function.parameters[0].argument_id,
        )
        .is_err()
    );
    assert!(
        DistributedFunctionExportPlan::new(
            endpoint.graph.graph_id,
            endpoint.endpoint.endpoint_id,
            distributed_declaration("session.function.open_result"),
            1,
            ProgramRole::Session,
            Vec::new(),
            DataTypePlan::Record {
                fields: Vec::new(),
                open: true,
            },
        )
        .is_err()
    );

    let mut missing_instance = empty_plan();
    missing_instance.program_role = ProgramRole::Session;
    missing_instance.distributed_endpoint = Some(endpoint);
    let verification = verify_plan(&missing_instance).unwrap();
    let check = verification
        .checks
        .iter()
        .find(|check| check.id == "producer-function-instances-canonical-and-resolved")
        .unwrap();
    assert!(!check.pass);
    assert!(check.detail.contains("exactly cover"));

    let mut missing_argument = session_producer_plan(&graph);
    missing_argument.producer_function_instances[0]
        .arguments
        .clear();
    let verification = verify_plan(&missing_argument).unwrap();
    let check = verification
        .checks
        .iter()
        .find(|check| check.id == "producer-function-instances-canonical-and-resolved")
        .unwrap();
    assert!(!check.pass);
    assert!(check.detail.contains("exactly match"));

    let mut wrong_import = session_producer_plan(&graph);
    wrong_import.producer_function_instances[0].arguments[0].import_id = ImportId([7; 32]);
    let (pass, detail) = producer_function_check(&wrong_import);
    assert!(!pass);
    assert!(
        detail.contains("exactly match its function signature"),
        "{detail}"
    );
}

#[test]
fn producer_function_instance_validation_rejects_duplicate_calls_and_static_type_mismatch() {
    let (_, graph, _) = distributed_graph_fixture();
    let mut duplicate = session_producer_plan(&graph);
    duplicate
        .producer_function_instances
        .push(duplicate.producer_function_instances[0].clone());
    let verification = verify_plan(&duplicate).unwrap();
    let check = verification
        .checks
        .iter()
        .find(|check| check.id == "producer-function-instances-canonical-and-resolved")
        .unwrap();
    assert!(!check.pass);
    assert!(check.detail.contains("unique canonically ordered"));

    let mut wrong_type = session_producer_plan(&graph);
    wrong_type.constants.push(PlanConstant {
        id: PlanConstantId(0),
        value: PlanConstantValue::Text {
            value: "not a number".to_owned(),
        },
    });
    wrong_type.producer_function_instances[0].result = ValueRef::Constant(PlanConstantId(0));
    let verification = verify_plan(&wrong_type).unwrap();
    let check = verification
        .checks
        .iter()
        .find(|check| check.id == "producer-function-instances-canonical-and-resolved")
        .unwrap();
    assert!(!check.pass);
    assert!(check.detail.contains("incompatible"));
}

#[test]
fn distributed_wire_hash_excludes_local_value_refs_and_source_ids() {
    let (application, graph, _) = distributed_graph_fixture();
    let mut changed_endpoints = graph.endpoints.clone();
    let server = changed_endpoints
        .iter_mut()
        .find(|endpoint| endpoint.role == ProgramRole::Server)
        .unwrap();
    server.value_exports[0].value = ValueRef::State(StateId(41));
    let session = changed_endpoints
        .iter_mut()
        .find(|endpoint| endpoint.role == ProgramRole::Session)
        .unwrap();
    session.value_exports[0].value = ValueRef::State(StateId(73));
    let changed =
        DistributedGraphPlan::new(&application, graph.graph.clone(), changed_endpoints).unwrap();
    assert_eq!(graph.wire_schema, changed.wire_schema);
    assert_eq!(
        distributed_graph_schema_hash(&graph).unwrap(),
        distributed_graph_schema_hash(&changed).unwrap()
    );

    let first_event = distributed_graph_with_event(
        &application,
        &graph,
        SourceId(3),
        SourceId(5),
        DataTypePlan::Text,
    );
    let renumbered_event = distributed_graph_with_event(
        &application,
        &graph,
        SourceId(103),
        SourceId(205),
        DataTypePlan::Text,
    );
    assert_ne!(
        first_event.endpoints[1].event_exports[0].source_id,
        renumbered_event.endpoints[1].event_exports[0].source_id
    );
    assert_ne!(
        first_event.endpoints[0].event_imports[0].local_source_id,
        renumbered_event.endpoints[0].event_imports[0].local_source_id
    );
    assert_eq!(first_event.wire_schema, renumbered_event.wire_schema);
    assert_eq!(
        distributed_graph_schema_hash(&first_event).unwrap(),
        distributed_graph_schema_hash(&renumbered_event).unwrap()
    );
}

#[test]
fn distributed_wire_hash_changes_with_edge_and_boundary_type() {
    let (application, graph, _) = distributed_graph_fixture();
    let with_event = distributed_graph_with_event(
        &application,
        &graph,
        SourceId(3),
        SourceId(5),
        DataTypePlan::Text,
    );
    let changed_type = distributed_graph_with_event(
        &application,
        &graph,
        SourceId(3),
        SourceId(5),
        DataTypePlan::Bytes { fixed_len: None },
    );

    assert_ne!(graph.wire_schema, with_event.wire_schema);
    assert_ne!(
        distributed_graph_schema_hash(&graph).unwrap(),
        distributed_graph_schema_hash(&with_event).unwrap()
    );
    assert_ne!(with_event.wire_schema, changed_type.wire_schema);
    assert_ne!(
        distributed_graph_schema_hash(&with_event).unwrap(),
        distributed_graph_schema_hash(&changed_type).unwrap()
    );
}

#[test]
fn distributed_graph_rejects_direction_type_and_source_revision_mismatches() {
    let (application, graph, _) = distributed_graph_fixture();

    let mut wrong_direction = graph.clone();
    wrong_direction.endpoints[0].value_imports[0].producer_role = ProgramRole::Client;
    assert!(
        wrong_direction
            .validate(&application)
            .unwrap_err()
            .to_string()
            .contains("direction")
    );

    let mut wrong_type = graph.clone();
    wrong_type.endpoints[0].value_imports[0].data_type = DataTypePlan::Text;
    assert!(
        wrong_type
            .validate(&application)
            .unwrap_err()
            .to_string()
            .contains("does not exactly match")
    );

    let mut wrong_revision = graph;
    wrong_revision.endpoints[0].value_imports[0].source_revision += 1;
    assert!(
        wrong_revision
            .validate(&application)
            .unwrap_err()
            .to_string()
            .contains("does not exactly match")
    );
}

#[test]
fn distributed_endpoint_verifier_rejects_a_machine_role_mismatch() {
    let (_, graph, row_expressions) = distributed_graph_fixture();
    let mut plan = empty_plan();
    plan.program_role = ProgramRole::Server;
    plan.distributed_endpoint = graph.endpoint_plan(ProgramRole::Client);
    plan.row_expressions = row_expressions;

    let verification = verify_plan(&plan).unwrap();
    let distributed_check = verification
        .checks
        .iter()
        .find(|check| check.id == "distributed-endpoint-canonical-and-resolved")
        .unwrap();
    assert!(!distributed_check.pass);
    assert!(distributed_check.detail.contains("does not match"));
    assert_eq!(verification.status, "fail");
}

#[test]
fn distributed_endpoint_verifier_rejects_an_unlinked_wire_hash() {
    let (_, graph, row_expressions) = distributed_graph_fixture();
    let mut endpoint = graph.endpoint_plan(ProgramRole::Client).unwrap();
    endpoint.wire_schema_hash[0] ^= 0xff;
    let mut plan = empty_plan();
    plan.program_role = ProgramRole::Client;
    plan.distributed_endpoint = Some(endpoint);
    plan.row_expressions = row_expressions;

    let verification = verify_plan(&plan).unwrap();
    let distributed_check = verification
        .checks
        .iter()
        .find(|check| check.id == "distributed-endpoint-canonical-and-resolved")
        .unwrap();
    assert!(!distributed_check.pass);
    assert!(distributed_check.detail.contains("wire schema hash"));
    assert_eq!(verification.status, "fail");
}

#[test]
fn distributed_endpoint_verifier_rejects_an_unresolved_call_argument_constant() {
    let (_, graph, mut row_expressions) = distributed_graph_fixture();
    let mut endpoint = graph.endpoint_plan(ProgramRole::Client).unwrap();
    let unresolved_constant = row_expressions
        .interner()
        .constant(PlanConstantId(999))
        .unwrap();
    endpoint.endpoint.remote_call_sites[0].arguments[0].value = unresolved_constant;
    let mut plan = empty_plan();
    plan.program_role = ProgramRole::Client;
    plan.distributed_endpoint = Some(endpoint);
    plan.row_expressions = row_expressions;

    let verification = verify_plan(&plan).unwrap();
    let distributed_check = verification
        .checks
        .iter()
        .find(|check| check.id == "distributed-endpoint-canonical-and-resolved")
        .unwrap();
    assert!(!distributed_check.pass);
    assert!(
        distributed_check
            .detail
            .contains("unsupported bounded expression"),
        "unexpected distributed verifier detail: {}",
        distributed_check.detail
    );
}

#[test]
fn distributed_endpoint_verifier_rejects_a_remote_call_result_cycle() {
    let (_, mut graph, mut row_expressions) = distributed_graph_fixture();
    let client = &mut graph.endpoints[0];
    let call_result = client.remote_call_sites[0]
        .result
        .current_import_id()
        .unwrap();
    let cycle = row_expressions
        .builder()
        .value(ValueRef::DistributedImport(call_result))
        .unwrap();
    client.remote_call_sites[0].arguments[0].value = cycle;

    let mut plan = empty_plan();
    plan.program_role = ProgramRole::Client;
    plan.distributed_endpoint = graph.endpoint_plan(ProgramRole::Client);
    plan.row_expressions = row_expressions;
    let verification = verify_plan(&plan).unwrap();
    let distributed_check = verification
        .checks
        .iter()
        .find(|check| check.id == "distributed-endpoint-canonical-and-resolved")
        .unwrap();
    assert!(!distributed_check.pass);
    assert!(distributed_check.detail.contains("call-result cycles"));
}

#[test]
fn root_output_demand_keeps_empty_selected_distinct_from_all() {
    assert_ne!(
        RootOutputDemand::All,
        RootOutputDemand::Selected(Vec::new())
    );
}

#[test]
fn typed_binary_plan_hash_is_deterministic_and_field_sensitive() {
    let plan = empty_plan();
    assert_eq!(plan_sha256(&plan).unwrap(), plan_sha256(&plan).unwrap());
    assert_eq!(plan_binary(&plan).unwrap(), plan_binary(&plan).unwrap());

    let mut changed = plan.clone();
    changed.demand.root_derived_outputs = RootOutputDemand::All;
    assert_ne!(plan_sha256(&plan).unwrap(), plan_sha256(&changed).unwrap());
}

#[test]
fn machine_plan_v2_is_rejected() {
    let mut plan = empty_plan();
    plan.version = PlanVersion { major: 2, minor: 0 };

    let verification = verify_plan(&plan).unwrap();
    assert_eq!(verification.status, "fail");
    assert!(
        verification
            .checks
            .iter()
            .any(|check| { check.id == "plan-version-supported" && !check.pass })
    );
}

#[test]
fn digest_identities_exclude_runtime_and_schema_details() {
    let owner = MemoryOwnerPath {
        canonical_module: "Model".to_owned(),
        named_owner_path: "store.preferences".to_owned(),
    };
    let memory_id =
        MemoryId::from_identity(&owner, "store.preferences.theme", MemoryKind::Scalar).unwrap();
    let same =
        MemoryId::from_identity(&owner, "store.preferences.theme", MemoryKind::Scalar).unwrap();
    let different_kind =
        MemoryId::from_identity(&owner, "store.preferences.theme", MemoryKind::IndexedField)
            .unwrap();

    assert_eq!(memory_id, same);
    assert_ne!(memory_id, different_kind);
    assert_eq!(memory_id.to_string().len(), 64);
}

#[test]
fn recursive_type_fingerprint_is_canonical() {
    let left = DataTypePlan::Record {
        fields: vec![
            DataTypeFieldPlan {
                name: "title".to_owned(),
                data_type: DataTypePlan::Text,
            },
            DataTypeFieldPlan {
                name: "details".to_owned(),
                data_type: DataTypePlan::List {
                    item: Box::new(DataTypePlan::Variant {
                        variants: vec![
                            DataVariantPlan {
                                tag: "Present".to_owned(),
                                fields: vec![DataTypeFieldPlan {
                                    name: "value".to_owned(),
                                    data_type: DataTypePlan::Number,
                                }],
                                open: false,
                            },
                            DataVariantPlan {
                                tag: "Absent".to_owned(),
                                fields: Vec::new(),
                                open: false,
                            },
                        ],
                    }),
                },
            },
        ],
        open: false,
    };
    let DataTypePlan::Record { mut fields, .. } = left.clone().canonicalized() else {
        panic!("expected record type");
    };
    fields.reverse();
    let right = DataTypePlan::Record {
        fields,
        open: false,
    };

    assert_eq!(
        data_type_fingerprint(&left).unwrap(),
        data_type_fingerprint(&right).unwrap()
    );
}

#[test]
fn canonical_schema_hash_excludes_runtime_numeric_links() {
    let application = ApplicationPlan::new(ApplicationIdentity::new(
        "dev.example.notes",
        "test-user",
        "test",
    ))
    .unwrap();
    let owner = MemoryOwnerPath {
        canonical_module: "$root".to_owned(),
        named_owner_path: "store".to_owned(),
    };
    let memory = MemoryPlan::new(
        PlanStorageId(0),
        MemoryKind::Scalar,
        "store.title",
        DataTypePlan::Text,
        InitialProvenance::ReconstructableDefault,
        owner,
    )
    .unwrap();
    let first =
        PersistencePlan::new(&application, 1, vec![memory.clone()], vec![], vec![]).unwrap();
    let mut moved = memory;
    moved.runtime_slot = PlanStorageId(99);
    let second = PersistencePlan::new(&application, 1, vec![moved], vec![], vec![]).unwrap();

    assert_eq!(first.schema_hash, second.schema_hash);
}

#[test]
fn effect_ids_and_builtin_contracts_are_stable_and_safe_by_construction() {
    let first = EffectId::from_host_operation("File/read_bytes").unwrap();
    let second = EffectId::from_host_operation("File/read_bytes").unwrap();
    assert_eq!(first, second);
    assert_ne!(
        first,
        EffectId::from_host_operation("File/read_text").unwrap()
    );

    let read = builtin_effect_contract("File/read_bytes").unwrap().unwrap();
    assert_eq!(read.replay, EffectReplay::ReadOnly);
    assert_eq!(read.barrier, EffectBarrier::None);
    assert!(read.validate().is_ok());

    let write = builtin_effect_contract("File/write_bytes")
        .unwrap()
        .unwrap();
    assert_eq!(write.replay, EffectReplay::ProcessScoped);
    assert_eq!(write.barrier, EffectBarrier::None);
    assert_eq!(
        write.schema.as_ref().unwrap().intent_constraints,
        vec![EffectIntentConstraintPlan::BytesLengthRange {
            field_path: vec!["bytes".to_owned()],
            min_inclusive: 0,
            max_inclusive: boon_effect_schema::FILE_BYTES_MAX_LIMIT,
        }]
    );
    assert!(write.validate().is_ok());
}

#[test]
fn file_read_stream_contract_preserves_delivery_and_intent_bounds() {
    let contract = builtin_effect_contract(boon_effect_schema::FILE_READ_STREAM_OPERATION)
        .unwrap()
        .unwrap();
    assert_eq!(contract.replay, EffectReplay::ReadOnly);
    assert_eq!(contract.barrier, EffectBarrier::None);
    assert_eq!(contract.result_policy, EffectResultPolicy::ReturnValue);
    assert_eq!(
        contract.delivery,
        EffectDeliveryCardinality::Stream {
            initial_credits: boon_effect_schema::FILE_STREAM_INITIAL_CREDITS,
            max_in_flight: boon_effect_schema::FILE_STREAM_MAX_IN_FLIGHT,
            credit_result_tags: vec!["Chunk".to_owned()],
            terminal_result_tags: vec![
                "Cancelled".to_owned(),
                "Failed".to_owned(),
                "Finished".to_owned(),
            ],
        }
    );
    let schema = contract.schema.as_ref().unwrap();
    assert_eq!(
        schema.intent_constraints,
        vec![EffectIntentConstraintPlan::UnsignedIntegerRange {
            field_path: vec!["chunk_bytes".to_owned()],
            min_inclusive: boon_effect_schema::FILE_STREAM_MIN_CHUNK_BYTES,
            max_inclusive: boon_effect_schema::FILE_STREAM_MAX_CHUNK_BYTES,
        }]
    );
    assert_eq!(
        schema.intent_defaults,
        vec![EffectIntentDefaultPlan {
            field_name: "chunk_bytes".to_owned(),
            value: EffectIntentDefaultValuePlan::Number {
                value: FiniteReal::from_i64_exact(
                    boon_effect_schema::FILE_STREAM_DEFAULT_CHUNK_BYTES,
                )
                .unwrap(),
            },
        }]
    );
    let DataTypePlan::Variant { variants } = &schema.result_type else {
        panic!("stream result must be a closed variant");
    };
    assert_eq!(
        variants
            .iter()
            .map(|variant| variant.tag.as_str())
            .collect::<Vec<_>>(),
        ["Cancelled", "Chunk", "Failed", "Finished", "Opened"]
    );
    let finished = variants
        .iter()
        .find(|variant| variant.tag == "Finished")
        .unwrap();
    assert!(finished.fields.iter().any(|field| {
        field.name == "digest"
            && field.data_type
                == DataTypePlan::Bytes {
                    fixed_len: Some(32),
                }
    }));
    assert!(contract.validate().is_ok());

    let single = builtin_effect_contract("File/read_bytes").unwrap().unwrap();
    assert_eq!(single.delivery, EffectDeliveryCardinality::Single);
}

#[test]
fn effect_verifier_rejects_unsafe_stream_contracts() {
    let valid = builtin_effect_contract(boon_effect_schema::FILE_READ_STREAM_OPERATION)
        .unwrap()
        .unwrap();
    let mut plan = empty_plan();
    plan.effects.push(valid.clone());
    let verification = verify_plan(&plan).unwrap();
    assert!(
        verification
            .checks
            .iter()
            .find(|check| check.id == "effect-contracts-canonical-and-safe")
            .unwrap()
            .pass
    );

    let mut invalid_limit = valid.clone();
    invalid_limit.delivery = EffectDeliveryCardinality::Stream {
        initial_credits: 0,
        max_in_flight: 1,
        credit_result_tags: vec!["Chunk".to_owned()],
        terminal_result_tags: vec![
            "Cancelled".to_owned(),
            "Failed".to_owned(),
            "Finished".to_owned(),
        ],
    };
    assert!(invalid_limit.validate().is_err());

    let mut invalid_terminal = valid.clone();
    invalid_terminal.delivery = EffectDeliveryCardinality::Stream {
        initial_credits: 1,
        max_in_flight: 1,
        credit_result_tags: vec!["Chunk".to_owned()],
        terminal_result_tags: vec!["Missing".to_owned()],
    };
    assert!(invalid_terminal.validate().is_err());

    let mut invalid_credit = valid.clone();
    invalid_credit.delivery = EffectDeliveryCardinality::Stream {
        initial_credits: 1,
        max_in_flight: 1,
        credit_result_tags: vec!["Missing".to_owned()],
        terminal_result_tags: vec![
            "Cancelled".to_owned(),
            "Failed".to_owned(),
            "Finished".to_owned(),
        ],
    };
    assert!(invalid_credit.validate().is_err());

    let mut invalid_replay = valid.clone();
    invalid_replay.replay = EffectReplay::NonReplayable;
    assert!(invalid_replay.validate().is_err());

    let mut invalid_constraint = valid;
    let schema = invalid_constraint.schema.as_mut().unwrap();
    schema.intent_constraints = vec![EffectIntentConstraintPlan::UnsignedIntegerRange {
        field_path: vec!["file".to_owned()],
        min_inclusive: 1,
        max_inclusive: 2,
    }];
    assert!(invalid_constraint.validate().is_err());

    plan.effects = vec![invalid_limit];
    let verification = verify_plan(&plan).unwrap();
    assert!(
        !verification
            .checks
            .iter()
            .find(|check| check.id == "effect-contracts-canonical-and-safe")
            .unwrap()
            .pass
    );
}

#[test]
fn passkey_effect_contracts_have_canonical_closed_outbox_schemas() {
    let simulation_type = || {
        DataTypePlan::Variant {
            variants: ["Success", "Cancel", "Failure", "Duplicate"]
                .into_iter()
                .map(|tag| DataVariantPlan {
                    tag: tag.to_owned(),
                    fields: Vec::new(),
                    open: false,
                })
                .collect(),
        }
        .canonicalized()
    };
    let cases = [
        (
            "DevelopmentPasskey/register",
            vec![
                ("account_id", DataTypePlan::Text),
                ("credential_count", DataTypePlan::Number),
                ("simulation", simulation_type()),
                ("workspace_grant_id", DataTypePlan::Text),
                ("workspace_id", DataTypePlan::Text),
            ],
            vec![
                (
                    "DuplicateCredential",
                    vec![
                        ("account_id", DataTypePlan::Text),
                        ("credential_id", DataTypePlan::Text),
                    ],
                ),
                ("RegistrationCancelled", Vec::new()),
                (
                    "RegistrationFailed",
                    vec![
                        ("code", DataTypePlan::Text),
                        ("message", DataTypePlan::Text),
                        ("retryable", DataTypePlan::Bool),
                    ],
                ),
                (
                    "RegistrationSucceeded",
                    vec![
                        ("account_id", DataTypePlan::Text),
                        ("credential_id", DataTypePlan::Text),
                        ("label", DataTypePlan::Text),
                        ("workspace_grant_bound", DataTypePlan::Bool),
                    ],
                ),
            ],
        ),
        (
            "DevelopmentPasskey/authenticate",
            vec![
                ("account_id", DataTypePlan::Text),
                ("credential_count", DataTypePlan::Number),
                ("simulation", simulation_type()),
            ],
            vec![
                ("AuthenticationCancelled", Vec::new()),
                (
                    "AuthenticationFailed",
                    vec![
                        ("code", DataTypePlan::Text),
                        ("message", DataTypePlan::Text),
                        ("retryable", DataTypePlan::Bool),
                    ],
                ),
                (
                    "AuthenticationSucceeded",
                    vec![
                        ("account_id", DataTypePlan::Text),
                        ("credential_id", DataTypePlan::Text),
                    ],
                ),
            ],
        ),
    ];

    for (operation, expected_intent, expected_results) in cases {
        let contract = builtin_effect_contract(operation).unwrap().unwrap();
        assert_eq!(contract.result_policy, EffectResultPolicy::ReturnValue);
        assert_eq!(contract.barrier, EffectBarrier::BeforeAndAfter);
        assert!(matches!(contract.replay, EffectReplay::Idempotent { .. }));

        let schema = builtin_effect_outbox_schema(operation).unwrap().unwrap();
        assert_eq!(schema.effect_id, contract.effect_id);
        let DataTypePlan::Record {
            fields: intent_fields,
            open: false,
        } = schema.intent_type
        else {
            panic!("{operation} intent must be a closed record");
        };
        assert_eq!(
            intent_fields
                .into_iter()
                .map(|field| (field.name, field.data_type))
                .collect::<Vec<_>>(),
            expected_intent
                .into_iter()
                .map(|(name, data_type)| (name.to_owned(), data_type))
                .collect::<Vec<_>>()
        );

        let DataTypePlan::Variant { variants } = schema.result_type else {
            panic!("{operation} result must be a closed variant");
        };
        assert_eq!(
            variants
                .into_iter()
                .map(|variant| {
                    assert!(!variant.open, "{operation}.{} must be closed", variant.tag);
                    (
                        variant.tag,
                        variant
                            .fields
                            .into_iter()
                            .map(|field| (field.name, field.data_type))
                            .collect::<Vec<_>>(),
                    )
                })
                .collect::<Vec<_>>(),
            expected_results
                .into_iter()
                .map(|(tag, fields)| {
                    (
                        tag.to_owned(),
                        fields
                            .into_iter()
                            .map(|(name, data_type)| (name.to_owned(), data_type))
                            .collect::<Vec<_>>(),
                    )
                })
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn persistence_schema_hash_includes_outbox_schema_but_excludes_output_registry() {
    let plan = empty_plan();
    let effect_id = EffectId::from_host_operation("Bank/transfer").unwrap();
    let key_type = DataTypePlan::Text;
    let outbox = EffectOutboxSchema::new(
        effect_id,
        DataTypePlan::Record {
            fields: vec![DataTypeFieldPlan {
                name: "amount".to_owned(),
                data_type: DataTypePlan::Number,
            }],
            open: false,
        },
        key_type.clone(),
        DataTypePlan::Bool,
    )
    .unwrap();
    let with_outbox = PersistencePlan::new_with_migrations_and_effect_outbox(
        &plan.application,
        plan.persistence.schema_version,
        Vec::new(),
        Vec::new(),
        vec![outbox],
        Vec::new(),
        None,
        Vec::new(),
    )
    .unwrap();
    assert_ne!(plan.persistence.schema_hash, with_outbox.schema_hash);

    let mut output_only = plan.clone();
    output_only.outputs.push(
        OutputRootPlan::new(
            "document",
            OutputContractKind::Document,
            OutputDemandPolicy::HostDemanded,
            OutputValueRef::RetainedVisual {
                expression: DocumentExprId(0),
            },
        )
        .unwrap(),
    );
    assert_eq!(
        plan.persistence.schema_hash,
        output_only.persistence.schema_hash
    );
}

#[test]
fn verifier_rejects_unresolved_outputs_and_unsafe_effects() {
    let mut plan = empty_plan();
    plan.outputs.push(
        OutputRootPlan::new(
            "document",
            OutputContractKind::Document,
            OutputDemandPolicy::HostDemanded,
            OutputValueRef::RetainedVisual {
                expression: DocumentExprId(0),
            },
        )
        .unwrap(),
    );
    let mut unsafe_effect = builtin_effect_contract("File/write_bytes")
        .unwrap()
        .unwrap();
    unsafe_effect.barrier = EffectBarrier::BeforeAndAfter;
    plan.effects.push(unsafe_effect);

    let verification = verify_plan(&plan).unwrap();
    assert!(
        verification.checks.iter().any(|check| {
            check.id == "output-roots-typed-canonical-and-resolved" && !check.pass
        })
    );
    assert!(
        verification
            .checks
            .iter()
            .any(|check| check.id == "effect-contracts-canonical-and-safe" && !check.pass)
    );
}

#[test]
fn event_row_requires_the_exact_source_owner_list() {
    let mut plan = empty_plan();
    let list_slot = |id: usize, scope: usize| ListStorageSlot {
        id: PlanStorageId(id),
        list_id: ListId(id),
        scope_id: Some(ScopeId(scope)),
        row_fields: Vec::new(),
        capacity: None,
        hidden_key_type: "u64".to_owned(),
        has_generation: true,
        initializer_kind: ListInitializerKind::Empty,
        range: None,
        initial_rows: Vec::new(),
    };
    plan.storage_layout.list_slots = vec![list_slot(0, 0), list_slot(1, 1)];
    plan.source_routes.push(SourceRoute {
        id: PlanSourceRouteId(0),
        source_id: SourceId(0),
        owner: PlanOwner {
            static_owner: PlanStaticOwnerId(1),
            ancestors: vec![PlanOwnerAncestor {
                static_owner: PlanStaticOwnerId(0),
                scope: ScopeId(0),
                list: ListId(0),
            }],
        },
        path: "rows.controls.select".to_owned(),
        scoped: true,
        scope_id: Some(ScopeId(0)),
        interval_ms: None,
        payload_schema: SourcePayloadSchema {
            fields: Vec::new(),
            typed_fields: Vec::new(),
        },
    });

    let matching_event = plan
        .row_expression_builder()
        .intern(PlanRowExpressionNode::EventRow {
            source: SourceId(0),
            list_id: ListId(0),
        })
        .unwrap();
    let wrong_list_event = plan
        .row_expression_builder()
        .intern(PlanRowExpressionNode::EventRow {
            source: SourceId(0),
            list_id: ListId(1),
        })
        .unwrap();

    assert!(row_expression_list_fields_resolve_inner(
        &plan,
        matching_event,
    ));
    assert!(!row_expression_list_fields_resolve_inner(
        &plan,
        wrong_list_event,
    ));
}

#[test]
fn verifier_detects_schema_hash_corruption() {
    let mut plan = empty_plan();
    plan.persistence.schema_hash[0] ^= 0xff;

    let verification = verify_plan(&plan).unwrap();
    assert!(
        verification
            .checks
            .iter()
            .any(|check| { check.id == "persistence-schema-hash-consistent" && !check.pass })
    );
}

fn migration_fixture() -> (ApplicationPlan, MemoryPlan, MigrationRecipePlan) {
    let application = ApplicationPlan::new(ApplicationIdentity::new(
        "dev.boon.migration",
        "test-user",
        "test",
    ))
    .unwrap();
    let owner = MemoryOwnerPath {
        canonical_module: "$root".to_owned(),
        named_owner_path: "$root".to_owned(),
    };
    let source_id = MemoryId::from_identity(&owner, "old_count", MemoryKind::Scalar).unwrap();
    let destination = MemoryPlan::new(
        PlanStorageId(0),
        MemoryKind::Scalar,
        "click_count",
        DataTypePlan::Number,
        InitialProvenance::ReconstructableDefault,
        owner,
    )
    .unwrap();
    let input = MigrationInputPlan::new(
        vec![MigrationLeafRefPlan::new(source_id, "old_count", DataTypePlan::Number).unwrap()],
        DataTypePlan::Number,
    )
    .unwrap();
    let recipe = MigrationRecipePlan::new(vec![MigrationTransferPlan {
        transfer_kind: MigrationTransferKindPlan::Scalar,
        indexed_list_owner: None,
        list_row_fields: Vec::new(),
        destination: MigrationDestinationPlan::new(
            destination.memory_id,
            "click_count",
            DataTypePlan::Number,
        )
        .unwrap(),
        transform: MigrationTransformPlan::Identity {
            input_id: input.input_id,
        },
        inputs: vec![input],
    }])
    .unwrap();
    (application, destination, recipe)
}

#[test]
fn migration_schema_recipe_and_catalog_hashes_are_independent() {
    let (application, destination, recipe) = migration_fixture();
    let unbound = PersistencePlan::new_with_migrations(
        &application,
        2,
        vec![destination.clone()],
        Vec::new(),
        vec![recipe.clone()],
        Some(recipe.migration_recipe_id),
        Vec::new(),
    )
    .unwrap();
    let edge = MigrationEdgePlan::new(1, 2, [0x44; 32], recipe.migration_recipe_id).unwrap();
    let bound = PersistencePlan::new_with_migrations(
        &application,
        2,
        vec![destination],
        Vec::new(),
        vec![recipe.clone()],
        Some(recipe.migration_recipe_id),
        vec![edge.clone()],
    )
    .unwrap();

    assert_eq!(unbound.schema_hash, bound.schema_hash);
    assert_eq!(unbound.migration_recipe_hash, bound.migration_recipe_hash);
    assert_ne!(unbound.migration_catalog_hash, bound.migration_catalog_hash);
    assert_eq!(
        edge,
        MigrationEdgePlan::new(1, 2, [0x44; 32], recipe.migration_recipe_id).unwrap()
    );
}

#[test]
fn compatible_migration_recipe_is_a_canonical_noop() {
    let first = MigrationRecipePlan::new(Vec::new()).unwrap();
    let second = MigrationRecipePlan::new(Vec::new()).unwrap();

    assert!(first.is_noop());
    assert_eq!(first, second);
    assert!(first.validate().is_ok());
}

#[test]
fn migration_recipe_rejects_unknown_calls_and_corrupt_ids() {
    let (_, _, valid) = migration_fixture();
    let transfer = &valid.transfers[0];
    let error = MigrationRecipePlan::new(vec![MigrationTransferPlan {
        transfer_kind: transfer.transfer_kind,
        indexed_list_owner: transfer.indexed_list_owner.clone(),
        list_row_fields: transfer.list_row_fields.clone(),
        inputs: transfer.inputs.clone(),
        destination: transfer.destination.clone(),
        transform: MigrationTransformPlan::Expression {
            root: MigrationExpressionPlan::Call {
                function: "Unknown/convert".to_owned(),
                input: Some(Box::new(MigrationExpressionPlan::Input {
                    input_id: transfer.inputs[0].input_id,
                })),
                arguments: Vec::new(),
            },
        },
    }])
    .unwrap_err();
    assert!(error.to_string().contains("non-target-neutral"), "{error}");

    let mut corrupt = valid;
    corrupt.migration_recipe_id.0[0] ^= 0xff;
    assert!(corrupt.validate().is_err());
}

#[test]
fn indexed_field_rename_within_one_list_memory_is_not_a_cycle() {
    let list_owner_identity = MemoryOwnerPath {
        canonical_module: "$root".to_owned(),
        named_owner_path: "$root".to_owned(),
    };
    let list_owner = MigrationListOwnerPlan::new(list_owner_identity, "tasks").unwrap();
    let source_memory = list_owner.memory_id;
    let destination_memory = list_owner.memory_id;
    let input = MigrationInputPlan::new(
        vec![MigrationLeafRefPlan::new(source_memory, "task.title", DataTypePlan::Text).unwrap()],
        DataTypePlan::Text,
    )
    .unwrap();
    let recipe = MigrationRecipePlan::new(vec![MigrationTransferPlan {
        transfer_kind: MigrationTransferKindPlan::IndexedRowField,
        indexed_list_owner: Some(list_owner),
        list_row_fields: Vec::new(),
        inputs: vec![input.clone()],
        destination: MigrationDestinationPlan::new(
            destination_memory,
            "task.text",
            DataTypePlan::Text,
        )
        .unwrap(),
        transform: MigrationTransformPlan::Identity {
            input_id: input.input_id,
        },
    }])
    .unwrap();
    assert!(recipe.validate().is_ok());

    let mut missing_owner = recipe.transfers[0].clone();
    missing_owner.indexed_list_owner = None;
    let error = MigrationRecipePlan::new(vec![missing_owner]).unwrap_err();
    assert!(error.to_string().contains("list-owner identity"), "{error}");

    let mut corrupt_owner = recipe.transfers[0].clone();
    corrupt_owner
        .indexed_list_owner
        .as_mut()
        .unwrap()
        .memory_id
        .0[0] ^= 0xff;
    let error = MigrationRecipePlan::new(vec![corrupt_owner]).unwrap_err();
    assert!(error.to_string().contains("list-owner identity"), "{error}");
}

#[test]
fn distributed_call_instance_identity_is_opaque_in_diagnostics() {
    let identity = DistributedCallInstanceId([0xa5; 32]);

    assert_eq!(format!("{identity:?}"), "DistributedCallInstanceId(..)");
    assert_eq!(identity.to_string(), "DistributedCallInstanceId(..)");
    assert!(!format!("{identity:?}").contains("a5"));
}
