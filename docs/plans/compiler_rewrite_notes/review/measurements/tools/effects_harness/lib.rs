//! Observation harness for today's effect re-run behaviour.
//!
//! It compiles a Boon source with the same request shape as
//! `crates/boon_host_runtime/tests/host_services.rs` (Server role, trusted
//! server profile), starts a `LiveRuntime` from the machine template so the
//! initial turn is visible, dispatches SOURCE events and prints every
//! `transient_effects` / `cancelled_transient_effects` entry of every turn.
//! Random/bytes completions are produced by the real `HostServiceEffectAdapter`
//! and delivered only when the test says so, which makes "still in flight"
//! observable.

use boon_host_runtime::{HostServiceEffectAdapter, NamedSecret};
use boon_host_services::{HostServiceConfig, HostServices};
use boon_plan::{ApplicationIdentity, ProgramRole};
use boon_program_runtime::{
    ProgramArtifact, ProgramCompileRequest, ProgramSession, compile_program_artifact,
};
use boon_runtime::{
    LiveRuntime, ProgramCapabilityProfile, RuntimeSourceUnit, RuntimeTurn, SessionOptions,
    SourcePayload, TransientEffectInvocation,
};

pub fn compile(name: &str, source: &str) -> ProgramArtifact {
    compile_program_artifact(&ProgramCompileRequest {
        revision: 1,
        entry_path: format!("{name}.bn"),
        units: vec![RuntimeSourceUnit {
            path: format!("{name}.bn"),
            source: source.to_owned(),
        }],
        application: ApplicationIdentity::new("dev.boon.effects-probe", "test", "local"),
        role: ProgramRole::Server,
        capability_profile: ProgramCapabilityProfile::TrustedServer,
    })
    .unwrap_or_else(|error| panic!("{name}: compile failed: {error:?}"))
}

pub struct Probe {
    pub runtime: LiveRuntime,
    pub adapter: HostServiceEffectAdapter,
    pub next_sequence: u64,
    pub invocations_seen: Vec<TransientEffectInvocation>,
    /// Call ids are opaque (Debug redacts them); number them in issue order.
    pub call_ordinals: std::collections::BTreeMap<boon_runtime::TransientEffectCallId, usize>,
}

impl Probe {
    fn ordinal(&self, call: &boon_runtime::TransientEffectCallId) -> String {
        match self.call_ordinals.get(call) {
            Some(n) => format!("#{n}"),
            None => "#?".to_owned(),
        }
    }
}

impl Probe {
    pub fn start(name: &str, source: &str) -> Self {
        let artifact = compile(name, source);
        // Record whether the embedded program host would accept this program:
        // `ProgramSession::start` refuses any startup turn with host work.
        match ProgramSession::start(compile(name, source)) {
            Ok(_) => println!("[{name}] ProgramSession::start: Ok (no startup host work)"),
            Err(error) => println!("[{name}] ProgramSession::start: Err({})", error.message),
        }
        let activation = LiveRuntime::from_machine_template(
            artifact.machine_template(),
            SessionOptions {
                program_revision: artifact.revision(),
                ..SessionOptions::default()
            },
        )
        .unwrap();
        let (runtime, initial_turn, _base) = activation.into_parts();
        let adapter = HostServiceEffectAdapter::new(
            HostServices::new(HostServiceConfig::default()),
            std::iter::empty::<NamedSecret>(),
            4,
        )
        .unwrap();
        let mut probe = Self {
            runtime,
            adapter,
            next_sequence: 1,
            invocations_seen: Vec::new(),
            call_ordinals: std::collections::BTreeMap::new(),
        };
        probe.describe("initial turn (startup)", &initial_turn);
        probe
    }

    pub fn describe(&mut self, label: &str, turn: &RuntimeTurn) {
        println!(
            "  turn {} [{label}] source_sequence={:?}: {} transient invocation(s), {} cancellation(s)",
            turn.sequence,
            turn.source_sequence,
            turn.transient_effects.len(),
            turn.cancelled_transient_effects.len()
        );
        for invocation in &turn.transient_effects {
            let n = self.call_ordinals.len() + 1;
            self.call_ordinals.insert(invocation.call_id, n);
            println!(
                "    INVOKE call=#{n} effect={} trigger_sequence={} intent={:?}",
                invocation.effect_id,
                invocation.trigger_sequence,
                invocation.intent
            );
            self.invocations_seen.push(invocation.clone());
        }
        for call in &turn.cancelled_transient_effects {
            println!("    CANCEL call={}", self.ordinal(call));
        }
        println!(
            "    pending transient effects after turn: {}",
            self.runtime.pending_transient_effect_count()
        );
    }

    pub fn dispatch(&mut self, label: &str, source_path: &str) -> RuntimeTurn {
        let sequence = self.next_sequence;
        self.next_sequence += 1;
        let event = self
            .runtime
            .source_event_for_path(sequence, source_path, &[], SourcePayload::default())
            .unwrap();
        let turn = self.runtime.dispatch(event).unwrap();
        self.describe(&format!("{label}: dispatch {source_path}"), &turn);
        turn
    }

    /// Runs the invocation through the real host-service adapter and delivers
    /// its completion to the runtime.
    pub fn complete(&mut self, label: &str, invocation: &TransientEffectInvocation) {
        let submission = self.adapter.submit(invocation.clone()).unwrap();
        let completion = submission
            .immediate_completion
            .expect("Random/bytes completes immediately");
        match self
            .runtime
            .complete_transient_effect(completion.call_id, completion.outcome.clone())
        {
            Ok(turn) => {
                let label = format!("{label}: complete call {}", self.ordinal(&invocation.call_id));
                self.describe(&label, &turn)
            }
            Err(error) => println!(
                "  complete call {} [{label}]: Err({error})",
                self.ordinal(&invocation.call_id)
            ),
        }
    }

    pub fn output(&mut self, name: &str) -> String {
        match self.runtime.output_value_current(name) {
            Ok(value) => format!("{value:?}"),
            Err(error) => format!("Err({error})"),
        }
    }

    pub fn report_outputs(&mut self, names: &[&str]) {
        for name in names {
            let value = self.output(name);
            println!("    output {name} = {value}");
        }
    }
}

pub fn read_program(file: &str) -> String {
    let dir = std::env::var("EFFECTS_DIR").expect("EFFECTS_DIR must point at the .bn files");
    std::fs::read_to_string(format!("{dir}/{file}")).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUTPUTS: &[&str] = &["size", "result_ready"];

    /// Q1a: THEN-triggered transient effect whose intent reads HOLD `size`.
    /// The first call is left IN FLIGHT when `size` changes.
    #[test]
    fn q1a_then_effect_hold_changes_while_in_flight() {
        println!("=== q1a: THEN effect, intent reads HOLD size; bump while call in flight ===");
        let mut p = Probe::start("q1_then_effect_reads_hold", &read_program("q1_then_effect_reads_hold.bn"));
        p.report_outputs(OUTPUTS);
        p.dispatch("step1", "store.go");
        p.report_outputs(OUTPUTS);
        p.dispatch("step2 (HOLD size 8->9 while call in flight)", "store.bump");
        p.report_outputs(OUTPUTS);
        p.dispatch("step3 (HOLD size 9->10 while call in flight)", "store.bump");
        p.report_outputs(OUTPUTS);
        let seen = p.invocations_seen.clone();
        for invocation in &seen {
            p.complete("step4 deliver every call ever issued", invocation);
        }
        p.report_outputs(OUTPUTS);
        p.dispatch("step5 (bump after all completions)", "store.bump");
        p.report_outputs(OUTPUTS);
        println!("  total invocations issued: {}", p.invocations_seen.len());
    }

    /// Q1b: same program; the first call is COMPLETED before `size` changes.
    #[test]
    fn q1b_then_effect_hold_changes_after_completion() {
        println!("=== q1b: THEN effect, intent reads HOLD size; bump after completion ===");
        let mut p = Probe::start("q1_then_effect_reads_hold", &read_program("q1_then_effect_reads_hold.bn"));
        p.dispatch("step1", "store.go");
        let first = p.invocations_seen[0].clone();
        p.complete("step2", &first);
        p.report_outputs(OUTPUTS);
        p.dispatch("step3 (HOLD size 8->9 after completion)", "store.bump");
        p.report_outputs(OUTPUTS);
        p.dispatch("step4 (HOLD size 9->10 after completion)", "store.bump");
        p.report_outputs(OUTPUTS);
        let later = p.invocations_seen[1..].to_vec();
        for invocation in &later {
            p.complete("step5 deliver later calls", invocation);
        }
        p.report_outputs(OUTPUTS);
        p.dispatch("step6 (go again)", "store.go");
        p.report_outputs(OUTPUTS);
        println!("  total invocations issued: {}", p.invocations_seen.len());
    }

    /// Q2: effect inside a WHILE arm whose selector HOLD starts in the open state.
    #[test]
    fn q2_while_arm_initially_open() {
        println!("=== q2: WHILE arm effect, selector HOLD mode starts Active ===");
        let mut p = Probe::start("q2_while_initially_open", &read_program("q2_while_initially_open.bn"));
        p.report_outputs(OUTPUTS);
        p.dispatch("step1 (HOLD size 8->9, gate open)", "store.bump");
        p.report_outputs(OUTPUTS);
        let seen = p.invocations_seen.clone();
        for invocation in &seen {
            p.complete("step2 deliver calls", invocation);
        }
        p.report_outputs(OUTPUTS);
        p.dispatch("step3 (HOLD size 9->10, gate open)", "store.bump");
        p.report_outputs(OUTPUTS);
        p.dispatch("step4 (stop: mode Active->Inactive)", "store.stop");
        p.report_outputs(OUTPUTS);
        println!("  total invocations issued: {}", p.invocations_seen.len());
    }

    /// Q2 variant: WHEN arm instead of WHILE.
    #[test]
    fn q2_when_arm_initially_open() {
        println!("=== q2: WHEN arm effect, selector HOLD mode starts Active ===");
        let mut p = Probe::start("q2_when_initially_open", &read_program("q2_when_initially_open.bn"));
        p.report_outputs(OUTPUTS);
        p.dispatch("step1 (HOLD size 8->9, gate open)", "store.bump");
        p.report_outputs(OUTPUTS);
        let seen = p.invocations_seen.clone();
        for invocation in &seen {
            p.complete("step2 deliver calls", invocation);
        }
        p.report_outputs(OUTPUTS);
        p.dispatch("step3 (HOLD size 9->10, gate open)", "store.bump");
        p.report_outputs(OUTPUTS);
        p.dispatch("step4 (stop: mode Active->Inactive)", "store.stop");
        p.report_outputs(OUTPUTS);
        println!("  total invocations issued: {}", p.invocations_seen.len());
    }

    /// Q3 baseline: WHILE arm whose selector starts closed; `go` opens it.
    #[test]
    fn q3_while_arm_initially_closed() {
        println!("=== q3: WHILE arm effect, selector HOLD mode starts Inactive; go opens ===");
        let mut p = Probe::start("q3_while_initially_closed", &read_program("q3_while_initially_closed.bn"));
        p.report_outputs(OUTPUTS);
        p.dispatch("step1 (bump while gate closed)", "store.bump");
        p.dispatch("step2 (go: mode Inactive->Active)", "store.go");
        p.report_outputs(OUTPUTS);
        p.dispatch("step3 (bump while call in flight, gate open)", "store.bump");
        p.report_outputs(OUTPUTS);
        let seen = p.invocations_seen.clone();
        for invocation in &seen {
            p.complete("step4 deliver every call", invocation);
        }
        p.report_outputs(OUTPUTS);
        p.dispatch("step5 (bump after completion, gate open)", "store.bump");
        p.report_outputs(OUTPUTS);
        p.dispatch("step6 (stop: gate closes)", "store.stop");
        p.report_outputs(OUTPUTS);
        p.dispatch("step7 (go again)", "store.go");
        p.report_outputs(OUTPUTS);
        println!("  total invocations issued: {}", p.invocations_seen.len());
    }

    /// Q3 variant: WHEN arm whose selector starts closed; `go` opens it.
    #[test]
    fn q3_when_arm_initially_closed() {
        println!("=== q3w: WHEN arm effect, selector HOLD mode starts Inactive; go opens ===");
        let mut p = Probe::start("q3_when_initially_closed", &read_program("q3_when_initially_closed.bn"));
        p.report_outputs(OUTPUTS);
        p.dispatch("step1 (bump while gate closed)", "store.bump");
        p.dispatch("step2 (go: mode Inactive->Active)", "store.go");
        p.report_outputs(OUTPUTS);
        p.dispatch("step3 (bump while call in flight, gate open)", "store.bump");
        p.report_outputs(OUTPUTS);
        let seen = p.invocations_seen.clone();
        for invocation in &seen {
            p.complete("step4 deliver every call", invocation);
        }
        p.report_outputs(OUTPUTS);
        p.dispatch("step5 (bump after completion, gate open)", "store.bump");
        p.report_outputs(OUTPUTS);
        p.dispatch("step6 (stop: gate closes)", "store.stop");
        p.report_outputs(OUTPUTS);
        p.dispatch("step7 (go again)", "store.go");
        p.report_outputs(OUTPUTS);
        println!("  total invocations issued: {}", p.invocations_seen.len());
    }
}
