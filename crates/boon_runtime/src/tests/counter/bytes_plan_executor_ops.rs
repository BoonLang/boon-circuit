// Bytes PlanExecutor tests are grouped by operation family while staying in one
// test module for private runtime diagnostics and report helpers.
include!("bytes_plan_executor_ops/source_payload_and_indexed.rs");
include!("bytes_plan_executor_ops/scalar_core.rs");
include!("bytes_plan_executor_ops/numeric_mutation_and_dependencies.rs");
