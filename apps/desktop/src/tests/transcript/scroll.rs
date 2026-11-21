use super::*;
use crate::latency::scroll::{DISTANCE, STEPS, background, workspace};
use serde_json::json;
use std::time::Instant;

fn runtime() -> Runtime<Workspace> {
    let app = spec();
    let mut rt = Runtime::new(workspace(), app.view, (800.0, 948.0), Env::new(true, 2.0));
    rt.poll = app.poll;
    rt.on_key = app.on_key;
    rt.set_font_families(app.families.0, app.families.1);
    advance(&mut rt, 0.6);
    rt
}

#[test]
#[ignore = "opt-in release scroll CPU profile; run alone"]
fn transcript_scrollback_profile() {
    for streaming in [false, true] {
        let mut rt = runtime();

        let [left, top, _, height] = element(&rt, "live-transcript-0").rect;
        let (x, y) = (left + 2.0, top + height / 2.0);
        let commits = rt.stats.commits;
        let start_y = rt.state.live.as_ref().unwrap().reading_position(0).offset[1];
        let mut frames = Vec::new();
        let mut committed = Vec::new();
        let mut midpoint_y = start_y;
        for step in 0..STEPS {
            let start = Instant::now();
            if streaming && step % 6 == 0 {
                rt.update_if(|s, _| background(s.live.as_mut().unwrap()));
            }
            let before = rt.stats.commits;
            rt.wheel(
                x,
                y,
                if step < STEPS / 2 {
                    DISTANCE
                } else {
                    -DISTANCE
                },
            );
            rt.frame(1.0 / 120.0);
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            if step == STEPS / 2 - 1 {
                midpoint_y = rt.state.live.as_ref().unwrap().reading_position(0).offset[1];
            }
            frames.push(elapsed);
            if rt.stats.commits != before {
                committed.push(elapsed);
            }
        }
        let end_y = rt.state.live.as_ref().unwrap().reading_position(0).offset[1];
        assert!(
            start_y - midpoint_y > 10_000.0,
            "outward pass must scroll the transcript"
        );
        assert!(
            end_y - midpoint_y > 10_000.0,
            "return pass must scroll the transcript"
        );
        let stats = |mut values: Vec<f64>| {
            if values.is_empty() {
                return serde_json::Value::Null;
            }
            values.sort_by(f64::total_cmp);
            json!({"count":values.len(),"p50_ms":values[values.len()/2],"p95_ms":values[values.len()*95/100],"p99_ms":values[values.len()*99/100],"max_ms":values.last().unwrap(),"over_8_33_ms":values.iter().filter(|t| **t > 1000.0/120.0).count()})
        };
        eprintln!(
            "{}",
            json!({"workload":if streaming {"scroll-streaming"} else {"scroll"},"frames":stats(frames),"committed_frames":stats(committed),"commits":rt.stats.commits-commits,"start_y":start_y,"midpoint_y":midpoint_y,"end_y":end_y})
        );
    }
}
