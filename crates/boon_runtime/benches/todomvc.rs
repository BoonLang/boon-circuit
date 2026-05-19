use std::path::Path;
use std::time::Instant;

fn main() {
    let source = Path::new("../../examples/todomvc.bn");
    let scenario = Path::new("../../examples/todomvc.scn");
    let started = Instant::now();
    for _ in 0..100 {
        boon_runtime::run_scenario(
            source,
            scenario,
            boon_runtime::VerificationLayer::Speed,
            None,
        )
        .expect("TodoMVC bench scenario should pass");
    }
    let elapsed = started.elapsed();
    println!(
        "todomvc static-runtime bench: {} iterations in {:.3}ms",
        100,
        elapsed.as_secs_f64() * 1000.0
    );
}
