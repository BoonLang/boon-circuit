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
        application,
        persistence,
        effects: Vec::new(),
        outputs: Vec::new(),
        host_ports: Vec::new(),
        query_collections: Vec::new(),
        query_indexes: Vec::new(),
        demand: DemandPlan {
            root_derived_outputs: RootOutputDemand::Selected(Vec::new()),
        },
        document: None,
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
            update_branch_count: 0,
            unresolved_update_branch_count: 0,
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

fn distributed_declaration(semantic_path: &str) -> DistributedDeclarationId {
    DistributedDeclarationId::from_semantic_path("DistributedFixture", semantic_path).unwrap()
}

fn distributed_graph_fixture() -> (ApplicationIdentity, DistributedGraphPlan) {
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
        ValueRef::Constant(PlanConstantId(0)),
        DataTypePlan::Number,
    )
    .unwrap();
    let function_declaration = distributed_declaration("server.function.double");
    let function_export_id = ExportId::from_identity(
        graph.graph_id,
        server_endpoint_id,
        DistributedExportKind::PureFunction,
        function_declaration,
    )
    .unwrap();
    let function_argument_id =
        DistributedArgumentId::from_parameter_name(function_export_id, "value").unwrap();
    let function_argument = || PlanRowExpression::Field {
        input: ValueRef::DistributedFunctionArgument {
            export_id: function_export_id,
            argument_id: function_argument_id,
        },
    };
    let server_function = DistributedPureFunctionExportPlan::new(
        graph.graph_id,
        server_endpoint_id,
        function_declaration,
        1,
        ProgramRole::Server,
        vec![("value".to_owned(), DataTypePlan::Number)],
        DataTypePlan::Number,
        PlanRowExpression::NumberInfix {
            op: "+".to_owned(),
            left: Box::new(function_argument()),
            right: Box::new(function_argument()),
        },
    )
    .unwrap();
    let server = DistributedEndpointContractPlan::new(
        &graph,
        server_declaration,
        1,
        ProgramRole::Server,
        vec![server_value.clone()],
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
    let session_value = DistributedValueExportPlan::new(
        graph.graph_id,
        session_endpoint_id,
        distributed_declaration("session.value.server_count"),
        1,
        ProgramRole::Session,
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
    let client_call = RemoteCallSitePlan::new(
        graph.graph_id,
        client_endpoint_id,
        distributed_declaration("client.call.double"),
        1,
        ProgramRole::Client,
        &server_function,
        vec![(
            "value".to_owned(),
            ValueRef::DistributedImport(client_import.import_id),
        )],
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
        vec![client_call],
    )
    .unwrap();

    let graph =
        DistributedGraphPlan::new(&application, graph, vec![server, client, session]).unwrap();
    (application, graph)
}

#[test]
fn distributed_graph_links_three_roles_and_each_machine_passes_the_distributed_check() {
    let (application, graph) = distributed_graph_fixture();

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

        let verification = verify_plan(&plan).unwrap();
        let distributed_check = verification
            .checks
            .iter()
            .find(|check| check.id == "distributed-endpoint-canonical-and-resolved")
            .unwrap();
        assert!(distributed_check.pass, "{}", distributed_check.detail);
    }
}

#[test]
fn distributed_graph_rejects_direction_type_and_source_revision_mismatches() {
    let (application, graph) = distributed_graph_fixture();

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
    let (_, graph) = distributed_graph_fixture();
    let mut plan = empty_plan();
    plan.program_role = ProgramRole::Server;
    plan.distributed_endpoint = graph.endpoint_plan(ProgramRole::Client);

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
fn distributed_endpoint_verifier_rejects_an_unresolved_call_argument_constant() {
    let (_, graph) = distributed_graph_fixture();
    let mut endpoint = graph.endpoint_plan(ProgramRole::Client).unwrap();
    endpoint.endpoint.remote_call_sites[0].arguments[0].value = PlanRowExpression::Constant {
        constant_id: PlanConstantId(999),
    };
    let mut plan = empty_plan();
    plan.program_role = ProgramRole::Client;
    plan.distributed_endpoint = Some(endpoint);

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
            .contains("bounded pure expressions")
    );
}

#[test]
fn distributed_graph_rejects_a_remote_call_result_cycle() {
    let (application, mut graph) = distributed_graph_fixture();
    let client = &mut graph.endpoints[0];
    let call_result = client.remote_call_sites[0].result_import_id;
    client.remote_call_sites[0].arguments[0].value =
        ValueRef::DistributedImport(call_result).into();

    let error = graph.validate(&application).unwrap_err();
    assert!(error.to_string().contains("call-result cycles"));
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
    assert_eq!(
        write.replay,
        EffectReplay::Idempotent {
            key_type: DataTypePlan::Bytes {
                fixed_len: Some(32),
            },
        }
    );
    assert_eq!(write.barrier, EffectBarrier::BeforeAndAfter);
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
        terminal_result_tags: vec!["Missing".to_owned()],
    };
    assert!(invalid_terminal.validate().is_err());

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
    plan.effects.push(
        builtin_effect_contract("File/write_bytes")
            .unwrap()
            .unwrap(),
    );

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
