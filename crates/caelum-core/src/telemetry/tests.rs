use super::*;
use crate::application::{Conversation, Entry, EntryKind, Provider, Scope};
use crate::remote::{Line, Run, Speaker, Thread, Turn as BoatTurn, VmProvider};
use serde_json::json;

#[test]
fn meter_computes_disjoint_buckets_and_pricing() {
    let meter = Meter::inclusive(&json!({
        "inputTokens": 1000,
        "outputTokens": 500,
        "cachedInputTokens": 300,
        "cachedWriteTokens": 100,
        "reasoningOutputTokens": 200,
        "cost": {"currency": "USD", "amount": 0.42}
    }))
    .unwrap();
    assert_eq!(meter.input, 600);
    assert_eq!(meter.output, 300);
    assert_eq!(meter.read, 300);
    assert_eq!(meter.write, 100);
    assert_eq!(meter.reasoning, 200);
    assert_eq!(meter.cost, Some(0.42));

    let priced = meter.price("openai/gpt-5.3-codex").unwrap();
    assert!(priced > 0.0);
    assert!(meter.price("unknown-model").is_none());
}

#[test]
fn tracking_records_codex_antigravity_and_boat_native_turns() {
    let mut tracking = Tracking::default();
    let scope = Scope {
        provider: Provider::Codex,
        project: "/repo/app".into(),
    };
    let mut conv = Conversation::default();
    conv.entries.push(Entry::new(
        "u1".into(),
        EntryKind::User,
        "hello".into(),
        true,
    ));
    conv.entries.push(Entry::new(
        "a1".into(),
        EntryKind::Assistant,
        "world".into(),
        true,
    ));

    assert!(tracking
        .event(
            &scope,
            "thr-1",
            Some("gpt-5.3-codex"),
            "turn/started",
            &json!({"_created": 1_700_000_000_000_u64, "turn": {"id": "turn-1"}}),
            &conv,
        )
        .is_none());
    tracking.event(
        &scope,
        "thr-1",
        Some("gpt-5.3-codex"),
        "thread/tokenUsage/updated",
        &json!({
            "turnId": "turn-1",
            "tokenUsage": {
                "total": {"totalTokens": 1500},
                "last": {"inputTokens": 1000, "outputTokens": 500, "cachedInputTokens": 200}
            }
        }),
        &conv,
    );
    // Duplicate totalTokens snapshot should be ignored.
    tracking.event(
        &scope,
        "thr-1",
        Some("gpt-5.3-codex"),
        "thread/tokenUsage/updated",
        &json!({
            "turnId": "turn-1",
            "tokenUsage": {
                "total": {"totalTokens": 1500},
                "last": {"inputTokens": 1000, "outputTokens": 500}
            }
        }),
        &conv,
    );
    let record = tracking
        .event(
            &scope,
            "thr-1",
            Some("gpt-5.3-codex"),
            "turn/completed",
            &json!({"_created": 1_700_000_005_000_u64, "turn": {"id": "turn-1", "durationMs": 4200}}),
            &conv,
        )
        .unwrap();
    assert_eq!(record.provider, StatsProvider::Codex);
    assert_eq!(record.duration, 4200);
    assert_eq!(record.meter.input, 800);
    assert_eq!(record.meter.read, 200);
    assert_eq!(record.meter.output, 500);
    assert!(!record.estimated);

    // Native Boat checkout uses StatsProvider::Boat and namespaced session ID.
    let boat_scope = Scope {
        provider: Provider::Antigravity,
        project: crate::remote::native::project_key("vm-1"),
    };
    tracking.event(
        &boat_scope,
        "ses-1",
        Some("gemini-pro"),
        "antigravity/turnStarted",
        &json!({"_created": 1_700_000_010_000_u64, "turnId": "ag-1"}),
        &conv,
    );
    let boat_record = tracking
        .event(
            &boat_scope,
            "ses-1",
            Some("gemini-pro"),
            "antigravity/turnCompleted",
            &json!({"_created": 1_700_000_012_500_u64, "turnId": "ag-1"}),
            &conv,
        )
        .unwrap();
    assert_eq!(boat_record.provider, StatsProvider::Boat);
    assert_eq!(boat_record.session, "native:antigravity:ses-1");
    assert_eq!(boat_record.duration, 2500);
    assert!(boat_record.estimated);
}

#[test]
fn store_persists_deduplicates_filters_and_merges_opencode_history() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::default();
    store.open(&dir.path().join("workspace-v1.json"));

    let day1 = 1_760_000_000_000_u64;
    let day2 = day1 + 86_400_000;
    let codex_row = Record {
        project: "/repo/caelum".into(),
        provider: StatsProvider::Codex,
        session: "s1".into(),
        turn: "t1".into(),
        model: "gpt-5.3-codex".into(),
        timestamp: day1,
        duration: 3000,
        meter: Meter {
            input: 1000,
            output: 500,
            read: 200,
            write: 0,
            reasoning: 100,
            cost: None,
        },
        estimated: false,
    };
    store.record(codex_row.clone());
    // Duplicate primary key must be ignored.
    store.record(codex_row);
    store.record(Record {
        project: "/repo/caelum".into(),
        provider: StatsProvider::OpenCode,
        session: "oc1".into(),
        turn: "t1".into(),
        model: "claude-sonnet-4-6".into(),
        timestamp: day2,
        duration: 2000,
        meter: Meter {
            input: 400,
            output: 200,
            read: 0,
            write: 0,
            reasoning: 0,
            cost: Some(0.15),
        },
        estimated: false,
    });
    // Invalid record increments dropped count.
    store.record(Record {
        project: String::new(),
        provider: StatsProvider::Codex,
        session: "s2".into(),
        turn: "t2".into(),
        model: "gpt-5".into(),
        timestamp: day2,
        duration: 100,
        meter: Meter::default(),
        estimated: true,
    });

    let (all, projects) = store.stats(None, &OpenCodeStatsFilter::default()).unwrap();
    assert_eq!(all.sessions, 2);
    assert_eq!(all.prompts, 2);
    assert_eq!(all.duration_ms, 5000);
    assert_eq!(all.active_days, 2);
    assert_eq!(all.streak, 2);
    assert_eq!(all.dropped, 1);
    assert!(all.recorded_only);
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].canonical, "/repo/caelum");

    let (codex_only, _) = store
        .stats(
            None,
            &OpenCodeStatsFilter {
                provider: Some(StatsProvider::Codex),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(codex_only.sessions, 1);
    assert_eq!(codex_only.prompts, 1);
    assert!(!codex_only.recorded_only);
    assert!(codex_only.cost > 0.0);

    // Invalid range is rejected.
    assert!(store
        .stats(
            None,
            &OpenCodeStatsFilter {
                from: Some(day2),
                to: Some(day1),
                ..Default::default()
            },
        )
        .is_err());

    // Merging authoritative OpenCode server stats excludes local OpenCode rows to avoid double-counting.
    let (mut local_non_oc, _) = store.read(&OpenCodeStatsFilter::default(), true).unwrap();
    assert_eq!(local_non_oc.prompts, 1);
    assert_eq!(local_non_oc.duration_ms, 5000);
    let server = OpenCodeStats {
        prompts: 10,
        steps: 10,
        sessions: 3,
        cost: 2.5,
        ..Default::default()
    };
    query::merge(&mut local_non_oc, server);
    assert_eq!(local_non_oc.prompts, 11);
    assert_eq!(local_non_oc.sessions, 4);
    assert_eq!(local_non_oc.duration_ms, 5000);
}

#[test]
fn boat_managed_turn_creates_estimated_record() {
    let mut thread = Thread {
        last_activity: Some(1000),
        model: Some("claude-sonnet-4-6".into()),
        turn: BoatTurn::Following(Run {
            id: "run-1".into(),
            provider: VmProvider::Codex,
            conversation: "conv-1".into(),
            prompt: "hi".into(),
        }),
        ..Default::default()
    };
    thread.lines.push(Line::new(Speaker::You, "hello world!"));
    thread.lines.push(Line::new(Speaker::Codex, "all done"));
    let row = boat("chk-1", &thread, None, 3500).unwrap();
    assert_eq!(row.provider, StatsProvider::Boat);
    assert_eq!(row.project, crate::remote::native::project_key("chk-1"));
    assert_eq!(row.session, "managed:codex:conv-1");
    assert_eq!(row.duration, 2500);
    assert!(row.estimated);
    assert!(row.meter.input > 0 && row.meter.output > 0);
}
