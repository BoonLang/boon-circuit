use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use boon_runtime::{LiveRuntime, RuntimeSourceUnit, RuntimeTurn};
use futures::channel::mpsc;

use crate::protocol::{PreviewIntent, SourceUnit, TestStep};

#[derive(Clone)]
pub struct CompileRequest {
    pub intent: PreviewIntent,
    pub request_id: Option<u64>,
    pub revision: u64,
    pub units: Vec<SourceUnit>,
    pub test_steps: Vec<TestStep>,
}

pub struct CompiledPreview {
    pub intent: PreviewIntent,
    pub request_id: Option<u64>,
    pub revision: u64,
    pub elapsed: Duration,
    pub source_key: String,
    pub runtime: LiveRuntime,
    pub mount: RuntimeTurn,
    pub test_steps: Vec<TestStep>,
}

pub struct CompileOutcome {
    pub revision: u64,
    pub result: Result<CompiledPreview, String>,
}

#[derive(Default)]
struct State {
    pending: Option<CompileRequest>,
    closing: bool,
    replaced: u64,
}

pub struct CompileWorker {
    state: Arc<(Mutex<State>, Condvar)>,
    thread: Option<JoinHandle<()>>,
}

impl CompileWorker {
    pub fn start() -> (Self, mpsc::UnboundedReceiver<CompileOutcome>) {
        let state = Arc::new((Mutex::new(State::default()), Condvar::new()));
        let worker_state = Arc::clone(&state);
        let (output, receiver) = mpsc::unbounded();
        let thread = thread::Builder::new()
            .name("boon-preview-compile".to_owned())
            .spawn(move || compile_loop(worker_state, output))
            .expect("spawn preview compile worker");
        (
            Self {
                state,
                thread: Some(thread),
            },
            receiver,
        )
    }

    pub fn replace(&self, request: CompileRequest) {
        let (lock, wake) = &*self.state;
        let mut state = lock.lock().expect("compile worker lock");
        if state.pending.replace(request).is_some() {
            state.replaced = state.replaced.saturating_add(1);
        }
        wake.notify_one();
    }

    pub fn replaced_count(&self) -> u64 {
        self.state.0.lock().expect("compile worker lock").replaced
    }
}

impl Drop for CompileWorker {
    fn drop(&mut self) {
        let (lock, wake) = &*self.state;
        lock.lock().expect("compile worker lock").closing = true;
        wake.notify_one();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn compile_loop(
    state: Arc<(Mutex<State>, Condvar)>,
    output: mpsc::UnboundedSender<CompileOutcome>,
) {
    loop {
        let request = {
            let (lock, wake) = &*state;
            let mut state = lock.lock().expect("compile worker lock");
            while state.pending.is_none() && !state.closing {
                state = wake.wait(state).expect("compile worker wait");
            }
            if state.closing {
                return;
            }
            state.pending.take().expect("pending compile request")
        };
        let revision = request.revision;
        let result = compile(request);
        if output
            .unbounded_send(CompileOutcome { revision, result })
            .is_err()
        {
            return;
        }
    }
}

fn compile(request: CompileRequest) -> Result<CompiledPreview, String> {
    if request.units.is_empty() {
        return Err("preview source bundle is empty".to_owned());
    }
    let started = Instant::now();
    let source_key = source_key(&request.units);
    let label = request
        .units
        .last()
        .map(|unit| unit.path.clone())
        .unwrap_or_else(|| "preview.bn".to_owned());
    let units = request
        .units
        .into_iter()
        .map(|unit| RuntimeSourceUnit {
            path: unit.path,
            source: unit.source,
        })
        .collect::<Vec<_>>();
    let runtime = LiveRuntime::from_project(&label, &units).map_err(|error| error.to_string())?;
    let mount = runtime.mount();
    Ok(CompiledPreview {
        intent: request.intent,
        request_id: request.request_id,
        revision: request.revision,
        elapsed: started.elapsed(),
        source_key,
        runtime,
        mount,
        test_steps: request.test_steps,
    })
}

pub fn source_key(units: &[SourceUnit]) -> String {
    let parts = units
        .iter()
        .map(|unit| (unit.path.as_str(), unit.source.as_str()))
        .collect::<Vec<_>>();
    boon_runtime::source_unit_parts_hash(&parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mailbox_is_depth_one_and_latest_wins() {
        let (worker, _results) = CompileWorker::start();
        for revision in 1..=4 {
            worker.replace(CompileRequest {
                intent: PreviewIntent::Replace,
                request_id: None,
                revision,
                units: Vec::new(),
                test_steps: Vec::new(),
            });
        }
        assert!(worker.replaced_count() >= 1);
    }
}
