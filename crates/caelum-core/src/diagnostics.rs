//! Opt-in, content-free latency traces. Producers never wait for the writer.
mod application;
mod http;
#[cfg(test)]
mod tests;

pub(crate) use application::command;
pub use application::event;
pub(crate) use application::opencode_event;
pub use application::session_owner;
pub(crate) use http::http;

use serde::Serialize;
use std::{
    cell::Cell,
    collections::hash_map::RandomState,
    fs::OpenOptions,
    hash::{BuildHasher, Hash},
    io::{self, BufWriter, Write},
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, SyncSender},
        Arc, OnceLock,
    },
    thread::JoinHandle,
    time::Instant,
};

const QUEUE_LIMIT: usize = 4096;
const RECORD_LIMIT: u64 = 250_000;
static TRACE: OnceLock<Recorder> = OnceLock::new();
thread_local! { static REQUEST: Cell<u64> = const { Cell::new(0) }; }

#[derive(Clone, Copy, Serialize)]
pub struct Mark {
    kind: &'static str,
    label: &'static str,
    id: u64,
    owner: u64,
    parent: u64,
}

#[derive(Serialize)]
struct Record {
    at_us: u64,
    #[serde(flatten)]
    mark: Mark,
    phase: &'static str,
    value: u64,
}

#[derive(Default)]
struct Counters {
    attempted: AtomicU64,
    dropped: AtomicU64,
    active: AtomicBool,
}

enum Message {
    Record(Record),
    Finish,
}

struct Recorder {
    start: Instant,
    hashes: RandomState,
    next: AtomicU64,
    sender: SyncSender<Message>,
    counters: Arc<Counters>,
}

/// Keep alive until the application exits; joins the writer and reports lost records.
pub struct Capture(Option<JoinHandle<io::Result<()>>>);

impl Capture {
    pub fn start(path: &Path) -> io::Result<Self> {
        if TRACE.get().is_some() {
            return Err(io::Error::other("latency capture already started"));
        }
        let file = OpenOptions::new().write(true).create_new(true).open(path)?;
        let (sender, receiver) = mpsc::sync_channel(QUEUE_LIMIT);
        let counters = Arc::new(Counters::default());
        counters.active.store(true, Ordering::Relaxed);
        let state = counters.clone();
        let writer = std::thread::spawn(move || {
            let mut output = BufWriter::new(file);
            let result = (|| {
                writeln!(output, "{{\"schema\":1,\"clock\":\"process_monotonic_us\",\"record_limit\":{RECORD_LIMIT}}}")?;
                'records: while let Ok(message) = receiver.recv() {
                    for message in std::iter::once(message).chain(receiver.try_iter()) {
                        match message {
                            Message::Record(record) => write_record(&mut output, &record)?,
                            Message::Finish => break 'records,
                        }
                    }
                    // Flush each available batch; idle captures have no timer or polling.
                    output.flush()?;
                }
                writeln!(
                    output,
                    "{{\"summary\":true,\"attempted\":{},\"dropped\":{}}}",
                    state.attempted.load(Ordering::Relaxed),
                    state.dropped.load(Ordering::Relaxed)
                )?;
                output.flush()
            })();
            state.active.store(false, Ordering::Relaxed);
            result
        });
        TRACE
            .set(Recorder {
                start: Instant::now(),
                hashes: RandomState::new(),
                next: AtomicU64::new(1),
                sender,
                counters,
            })
            .map_err(|_| io::Error::other("latency capture already started"))?;
        Ok(Self(Some(writer)))
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        if let Some(trace) = TRACE.get() {
            trace.counters.active.store(false, Ordering::Relaxed);
            let _ = trace.sender.send(Message::Finish);
        }
        if let Some(writer) = self.0.take() {
            match writer.join() {
                Ok(Ok(())) => {}
                _ => eprintln!("Latency trace writer failed; capture is incomplete"),
            }
        }
    }
}

fn write_record(output: &mut impl Write, record: &Record) -> io::Result<()> {
    serde_json::to_writer(&mut *output, record)?;
    output.write_all(b"\n")
}

pub fn enabled() -> bool {
    TRACE
        .get()
        .is_some_and(|trace| trace.counters.active.load(Ordering::Relaxed))
}

pub fn anonymous(value: impl Hash) -> u64 {
    TRACE.get().map_or(0, |trace| trace.hashes.hash_one(value))
}

pub fn mark(kind: &'static str, label: &'static str, id: u64, owner: u64) -> Option<Mark> {
    enabled().then(|| Mark {
        kind,
        label,
        id,
        owner,
        parent: REQUEST.get(),
    })
}

pub fn next(kind: &'static str, label: &'static str, owner: u64) -> Option<Mark> {
    let trace = TRACE.get().filter(|_| enabled())?;
    mark(
        kind,
        label,
        trace.next.fetch_add(1, Ordering::Relaxed),
        owner,
    )
}

impl Mark {
    pub fn owner(&self) -> u64 {
        self.owner
    }

    pub fn record(self, phase: &'static str, value: u64) {
        let Some(trace) = TRACE.get().filter(|_| enabled()) else {
            return;
        };
        let record = Record {
            at_us: trace.start.elapsed().as_micros() as u64,
            mark: self,
            phase,
            value,
        };
        trace.record(record);
    }

    pub fn span(self) -> Span {
        self.record("begin", 0);
        Span(Some(self))
    }
}

impl Recorder {
    fn record(&self, record: Record) {
        if self.counters.attempted.fetch_add(1, Ordering::Relaxed) >= RECORD_LIMIT
            || self.sender.try_send(Message::Record(record)).is_err()
        {
            self.counters.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

pub struct Span(Option<Mark>);

impl Span {
    pub fn finish(mut self, phase: &'static str, value: u64) {
        if let Some(mark) = self.0.take() {
            mark.record(phase, value);
        }
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        if let Some(mark) = self.0.take() {
            mark.record("end", 0);
        }
    }
}

pub(crate) struct RequestContext(Option<u64>);

pub(crate) fn context(id: u64) -> RequestContext {
    RequestContext(enabled().then(|| REQUEST.replace(id)))
}

impl Drop for RequestContext {
    fn drop(&mut self) {
        if let Some(previous) = self.0 {
            REQUEST.set(previous);
        }
    }
}
