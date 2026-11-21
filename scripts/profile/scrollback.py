"""Content-free scrolling observations from the fixed-deadline native replay."""

from collections import defaultdict

from latency import distribution


def summarize_scroll(records):
    positions = defaultdict(dict)
    frames = defaultdict(dict)
    interactions = defaultdict(dict)
    for row in records:
        kind, phase = row.get("kind"), row.get("phase")
        if kind == "scroll":
            sample = positions[row["id"]]
            sample[phase] = row["value"]
            sample["at_us"] = max(sample.get("at_us", row["at_us"]), row["at_us"])
        elif kind == "frame" and row["label"] == "drawn":
            frame = frames[row["id"]]
            frame[phase] = row["value"] / 1000
            frame["at_us"] = max(frame.get("at_us", row["at_us"]), row["at_us"])
        elif kind == "interaction" and row["label"] in ("scroll_up", "scroll_down"):
            action = interactions[row["id"]]
            action[phase] = row["at_us"]
            action["label"] = row["label"]
    complete_positions = all(
        {"step", "offset_mpt", "commits"} <= sample.keys() for sample in positions.values())
    samples = sorted((s for s in positions.values()
                      if {"step", "offset_mpt", "commits"} <= s.keys()), key=lambda s: s["at_us"])
    phases = {}
    for label, low, high in (("scroll_up", 0, 960), ("scroll_down", 960, 1920)):
        actions = [a for a in interactions.values() if a["label"] == label]
        complete = [a for a in actions if "begin" in a and "drawn" in a]
        if not complete:
            phases[label] = {"complete": False}
            continue
        start, end = min(a["begin"] for a in complete), max(a["drawn"] for a in complete)
        shown = [s for s in samples if start <= s["at_us"] <= end and low <= s["step"] < high]
        cpu = [f["update"] + f["encode"] for f in frames.values()
               if start <= f["at_us"] <= end and {"update", "encode"} <= f.keys()]
        intervals = [(b["at_us"] - a["at_us"]) / 1000 for a, b in zip(shown, shown[1:])]
        offsets = [s["offset_mpt"] / 1000 for s in shown]
        phases[label] = {
            "complete": (len(complete) == len(actions) == 960 and complete_positions
                         and bool(cpu) and len(shown) > 1),
            "requests": len(actions),
            "observed_steps": len({s["step"] for s in shown}),
            "draw_callbacks": len(shown),
            "commits": shown[-1]["commits"] - shown[0]["commits"] if shown else None,
            "offset_change_points": offsets[-1] - offsets[0] if offsets else None,
            "offset_span_points": max(offsets) - min(offsets) if offsets else None,
            "frame_cpu": distribution(cpu),
            "frame_cpu_over_8_33_ms": sum(t > 1000 / 120 for t in cpu),
            "frame_cpu_over_16_67_ms": sum(t > 1000 / 60 for t in cpu),
            "draw_callback_intervals": distribution(intervals),
            "draw_callback_intervals_over_12_5_ms": sum(t > 12.5 for t in intervals),
        }
    complete = (all(p["complete"] for p in phases.values())
                and phases["scroll_up"]["offset_change_points"] < -10_000
                and phases["scroll_down"]["offset_change_points"] > 10_000)
    return {"complete": complete, "phases": phases}


def complete_workload(report, workload):
    if not report["complete_capture"] or report["anomalies"]:
        return False
    if any(kind.startswith("interaction") for kind in report["unfinished"]):
        return False
    if workload.startswith("scroll"):
        return (report["scroll"]["complete"]
                and report["workload_events"].get("scroll") == 1920
                and report["workload_events"].get("stream", 0)
                    == (1900 if workload == "scroll-streaming" else 0))
    names = ("interaction.navigate.drawn", "interaction.navigate.ready", "interaction.edit.drawn")
    return (all(report["metrics"].get(name, {}).get("count") == 80 for name in names)
            and report["workload_events"].get("stream", 0) == (2280 if workload == "streaming" else 0))
