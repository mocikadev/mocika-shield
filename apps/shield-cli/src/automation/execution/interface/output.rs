//! 有界公共事件流；写线程阻塞不能阻塞任务监督，终态发送有独立有限宽限。
use super::*;
use crate::automation::event_output::Output;

pub(super) struct Events {
    output: Output,
    cancel: control::Cancel,
    operation: &'static str,
    task: Option<String>,
    sequence: u64,
    seen: [bool; 13],
    started: Instant,
    delivery_ok: bool,
}
impl Events {
    pub fn new(
        operation: &'static str,
        task: Option<String>,
        cancel: control::Cancel,
    ) -> Result<Self, ()> {
        Self::with_writer(operation, task, cancel, std::io::stdout())
    }
    fn with_writer(
        operation: &'static str,
        task: Option<String>,
        cancel: control::Cancel,
        writer: impl Write + Send + 'static,
    ) -> Result<Self, ()> {
        let output = Output::new(writer, cancel.clone())?;
        Ok(Self {
            output,
            cancel,
            operation,
            task,
            sequence: 0,
            seen: [false; 13],
            started: Instant::now(),
            delivery_ok: true,
        })
    }
    fn send(&mut self, mut body: Value, kind: &str) {
        body["schema_version"] = serde_json::json!(1);
        body["experimental"] = serde_json::json!(true);
        body["tool_version"] = serde_json::json!(env!("CARGO_PKG_VERSION"));
        body["operation"] = serde_json::json!(self.operation);
        body["task_id"] = serde_json::json!(self.task);
        body["sequence"] = serde_json::json!(self.sequence);
        body["type"] = serde_json::json!(kind);
        let Ok(mut bytes) = serde_json::to_vec(&body) else {
            self.reject_delivery();
            return;
        };
        if bytes.len() > 4096 {
            self.reject_delivery();
            return;
        }
        bytes.push(b'\n');
        if !self.output.send(bytes) {
            self.reject_delivery();
        } else {
            self.sequence += 1;
        }
    }
    fn reject_delivery(&mut self) {
        self.delivery_ok = false;
        self.cancel.request(Cause::Output);
    }
    pub fn started(&mut self) {
        self.send(serde_json::json!({}), "started");
    }
    pub fn progress(&mut self, notice: Notice) {
        use crate::automation::task_lifecycle::Stage;
        let (index, name) = match notice {
            Notice::Publishing => (12, "publish"),
            Notice::Stage(stage) => (
                stage as usize,
                match stage {
                    Stage::Prepare => "prepare",
                    Stage::CheckTools => "check_tools",
                    Stage::Preflight => "preflight",
                    Stage::ValidateResources => "validate_resources",
                    Stage::Unpack => "unpack",
                    Stage::ModifyManifest => "modify_manifest",
                    Stage::ProcessDex => "process_dex",
                    Stage::InjectRuntime => "inject_runtime",
                    Stage::Repack => "repack",
                    Stage::Align => "align",
                    Stage::Sign => "sign",
                    Stage::Verify => "verify",
                },
            ),
        };
        if self.seen[index] {
            return;
        }
        self.seen[index] = true;
        self.send(
            serde_json::json!({"stage":name,"elapsed_ms":self.started.elapsed().as_millis()}),
            "progress",
        );
    }
    pub fn finish(mut self, terminal: Terminal, grace: Duration) -> bool {
        match serde_json::to_value(terminal) {
            Ok(value) => self.send(value, "result"),
            Err(_) => self.reject_delivery(),
        }
        self.output.finish(grace) && self.delivery_ok
    }
}

pub(super) fn initial_failure(operation: &'static str, code: &'static str) -> i32 {
    match Events::new(operation, None, control::Cancel::new()) {
        Ok(events) => {
            if events.finish(Terminal::before_start(code), Duration::from_secs(2)) {
                1
            } else {
                3
            }
        }
        _ => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Condvar, Mutex};
    #[test]
    fn 事件发送失败即使写线程正常结束也不能确认回执() {
        let mut events =
            Events::with_writer("agent_plan", None, control::Cancel::new(), std::io::sink())
                .ok()
                .unwrap();
        events.send(
            serde_json::json!({"oversized":"x".repeat(5000)}),
            "progress",
        );
        assert!(!events.finish(Terminal::before_start("FIXTURE"), Duration::from_secs(1)));
    }
    struct Blocked(Arc<(Mutex<bool>, Condvar)>);
    impl Write for Blocked {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            let (lock, condition) = &*self.0;
            let mut released = lock.lock().unwrap();
            while !*released {
                released = condition.wait(released).unwrap();
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    #[test]
    fn 阻塞输出不能卡住事件提交和有限收尾() {
        let shared = Arc::new((Mutex::new(false), Condvar::new()));
        let mut events = Events::with_writer(
            "agent_plan",
            None,
            control::Cancel::new(),
            Blocked(shared.clone()),
        )
        .ok()
        .unwrap();
        let started = Instant::now();
        events.started();
        for _ in 0..100 {
            events.progress(Notice::Publishing);
        }
        let delivered = events.finish(Terminal::before_start("FIXTURE"), Duration::from_millis(20));
        let (lock, condition) = &*shared;
        *lock.lock().unwrap() = true;
        condition.notify_all();
        assert!(!delivered);
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
