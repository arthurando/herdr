//! Bounded opt-in numeric telemetry. Never records terminal text, keys or targets.
use std::io::{BufWriter, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, OnceLock};
use std::time::{Duration, Instant};

static TRACE: OnceLock<Option<Trace>> = OnceLock::new();
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static DROPPED: AtomicU64 = AtomicU64::new(0);
struct Trace {
    sender: mpsc::SyncSender<Event>,
    started: Instant,
}
struct Event {
    stage: &'static str,
    id: u64,
    at_us: u128,
    elapsed_us: u128,
    count: usize,
    remote: bool,
}
#[derive(Clone, Copy, Debug)]
pub(super) struct Token {
    id: u64,
    started: Instant,
}
pub(super) struct Span {
    token: Option<Token>,
    stage: &'static str,
    count: usize,
    remote: bool,
}
fn trace() -> Option<&'static Trace> {
    TRACE.get_or_init(|| {
        let path = std::env::var_os("HERDR_LATENCY_TRACE")?;
        let file = std::fs::OpenOptions::new().create_new(true).write(true).open(path).ok()?;
        let (sender, receiver) = mpsc::sync_channel::<Event>(4096);
        let started = Instant::now();
        std::thread::Builder::new().name("herdr-latency-trace".into()).spawn(move || {
            let mut output = BufWriter::new(file);
            let unix_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
            let _ = writeln!(output, "{{\"stage\":\"start\",\"pid\":{},\"unix_ms\":{},\"duration_s\":180}}", std::process::id(), unix_ms);
            loop {
                match receiver.recv_timeout(Duration::from_millis(250)) {
                    Ok(e) => { let _ = writeln!(output, "{{\"stage\":\"{}\",\"id\":{},\"at_us\":{},\"elapsed_us\":{},\"count\":{},\"remote\":{}}}", e.stage,e.id,e.at_us,e.elapsed_us,e.count,e.remote); }
                    Err(mpsc::RecvTimeoutError::Timeout) => { let _ = output.flush(); }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
                if started.elapsed() > Duration::from_secs(180) { break; }
            }
            let _ = writeln!(output, "{{\"stage\":\"stop\",\"dropped\":{}}}", DROPPED.load(Ordering::Relaxed));
            let _ = output.flush();
        }).ok()?;
        Some(Trace { sender, started })
    }).as_ref().filter(|trace| trace.started.elapsed() <= Duration::from_secs(180))
}
pub(super) fn token() -> Option<Token> {
    trace().map(|_| Token {
        id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
        started: Instant::now(),
    })
}
impl Token {
    pub(super) fn mark(self, stage: &'static str, count: usize, remote: bool) {
        if let Some(trace) = trace() {
            if trace
                .sender
                .try_send(Event {
                    stage,
                    id: self.id,
                    at_us: trace.started.elapsed().as_micros(),
                    elapsed_us: self.started.elapsed().as_micros(),
                    count,
                    remote,
                })
                .is_err()
            {
                DROPPED.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}
pub(super) fn mark(stage: &'static str, count: usize, remote: bool) {
    if let Some(token) = token() {
        token.mark(stage, count, remote);
    }
}
pub(super) fn span(stage: &'static str, count: usize, remote: bool) -> Span {
    Span {
        token: token(),
        stage,
        count,
        remote,
    }
}
impl Drop for Span {
    fn drop(&mut self) {
        if let Some(token) = self.token {
            token.mark(self.stage, self.count, self.remote);
        }
    }
}
