use super::*;

fn recorder(capacity: usize) -> (Recorder, mpsc::Receiver<Message>) {
    let (sender, receiver) = mpsc::sync_channel(capacity);
    (
        Recorder {
            start: Instant::now(),
            hashes: RandomState::new(),
            next: AtomicU64::new(1),
            sender,
            counters: Arc::new(Counters::default()),
        },
        receiver,
    )
}

fn record() -> Record {
    Record {
        at_us: 42,
        mark: Mark {
            kind: "http",
            label: "session.messages",
            id: 1,
            owner: 2,
            parent: 3,
        },
        phase: "ok",
        value: 100,
    }
}

#[test]
fn full_and_disconnected_writers_drop_records_without_blocking() {
    let (trace, receiver) = recorder(1);
    trace.record(record());
    trace.record(record());
    assert_eq!(trace.counters.dropped.load(Ordering::Relaxed), 1);
    drop(receiver);
    trace.record(record());
    assert_eq!(trace.counters.dropped.load(Ordering::Relaxed), 2);
}

#[test]
fn capture_size_is_bounded_even_when_the_writer_keeps_up() {
    let (trace, receiver) = recorder(1);
    trace
        .counters
        .attempted
        .store(RECORD_LIMIT, Ordering::Relaxed);
    trace.record(record());
    assert!(receiver.try_recv().is_err());
    assert_eq!(trace.counters.dropped.load(Ordering::Relaxed), 1);
}

#[test]
fn trace_schema_contains_only_labels_counts_and_anonymous_ids() {
    let mut bytes = Vec::new();
    write_record(&mut bytes, &record()).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        value,
        serde_json::json!({"at_us":42,"kind":"http","label":"session.messages","id":1,"owner":2,"parent":3,"phase":"ok","value":100})
    );
}
