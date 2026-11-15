#!/usr/bin/env python3
"""Summarize content-free native latency traces; never treat missing pairs as zero."""

import argparse
from collections import Counter, defaultdict
import json
import math
from pathlib import Path
import statistics


def distribution(values):
    values = sorted(values)
    if not values:
        return None
    return {
        "count": len(values), "p50_ms": statistics.median(values),
        "p95_ms": values[math.ceil(len(values) * .95) - 1],
        "p99_ms": values[math.ceil(len(values) * .99) - 1], "max_ms": values[-1],
    }


def summarize(records):
    groups = defaultdict(dict)
    labels = {}
    metrics = defaultdict(list)
    http_counts = Counter()
    http_bytes = Counter()
    outcomes = Counter()
    tools = {}
    last_tool = {}
    steps = {}
    anomalies = Counter()
    footer = next((r for r in reversed(records) if r.get("summary")), None)
    if not records or records[0].get("schema") != 1:
        raise ValueError("Expected latency trace schema 1")

    def elapsed(name, start, end):
        if start is None or end is None:
            return
        if end < start:
            anomalies["reversed_intervals"] += 1
            return
        metrics[name].append((end - start) / 1000)

    # Thread scheduling can put records on the writer queue out of timestamp order.
    for row in sorted((r for r in records if "at_us" in r), key=lambda r: r["at_us"]):
        kind, phase, label = row["kind"], row["phase"], row["label"]
        at = row["at_us"]
        key = (kind, row["id"])
        if kind == "startup":
            metrics["startup.first_drawn"].append(at / 1000)
            continue
        if kind == "frame":
            if phase == "update":
                outcomes[label] += 1
            if label == "drawn":
                metrics[f"frame.{phase}"].append(row["value"] / 1000)
            continue
        if phase in groups[key]:
            anomalies["duplicate_stages"] += 1
            continue
        groups[key][phase] = at
        if phase == "created" or key not in labels:
            labels[key] = label
        if kind == "http":
            if phase == "begin":
                http_counts[label] += 1
            elif phase in ("ok", "error"):
                http_bytes[label] += row["value"]
        if kind != "provider" or phase != "received":
            continue
        owner, parent = row["owner"], row["parent"]
        tool = (owner, parent)
        if label == "tool.called" and parent:
            tools[tool] = (at, row["value"])
        elif label in ("tool.success", "tool.failed") and parent:
            start = tools.pop(tool, None)
            if start:
                elapsed("tool.observed_call_to_result", start[0], at)
                # Provider timestamps are milliseconds; never mix them with local time.
                if start[1] and row["value"]:
                    elapsed("tool.provider_call_to_result", start[1] * 1000, row["value"] * 1000)
                last_tool[owner] = at
        elif label == "step.started":
            if parent:
                steps[tool] = at
            if not any(key[0] == owner for key in tools):
                elapsed("tool.last_result_to_next_step", last_tool.pop(owner, None), at)
        elif label == "step.first-token" and parent:
            elapsed("model.observed_step_to_first_token", steps.pop(tool, None), at)
        elif label.startswith("execution."):
            last_tool.pop(owner, None)
            tools = {key: value for key, value in tools.items() if key[0] != owner}
            steps = {key: value for key, value in steps.items() if key[0] != owner}

    unfinished = Counter()
    for key, times in groups.items():
        kind, _ = key
        label = labels[key]
        if kind == "request":
            for name, start, end in [
                ("admission", "created", "submitted"),
                ("queue", "submitted", "started"),
                ("worker", "started", "finished"),
                ("delivery", "finished", "begin"),
                ("apply", "begin", "end"),
            ]:
                elapsed(f"request.{label}.{name}", times.get(start), times.get(end))
            if "created" in times and "end" not in times and "rejected" not in times:
                unfinished[kind] += 1
        elif kind in ("http", "connection"):
            end = next((times[p] for p in ("ok", "error", "end") if p in times), None)
            elapsed(f"{kind}.{label}", times.get("begin"), end)
            if end is None:
                unfinished[kind] += 1
        elif kind == "interaction":
            for phase in ("drawn", "ready"):
                elapsed(f"interaction.{label}.{phase}", times.get("begin"), times.get(phase))
            if not any(p in times for p in ("drawn", "abandoned", "overflow")):
                unfinished[kind] += 1
            for phase in ("abandoned", "overflow"):
                if phase in times:
                    anomalies[phase] += 1
        elif kind == "provider":
            for name, start, end in [
                ("delivery", "received", "begin"),
                ("apply", "begin", "end"),
                ("drawn", "received", "drawn"),
            ]:
                elapsed(f"provider.{label}.{name}", times.get(start), times.get(end))

    return {
        "complete_capture": footer is not None and footer["dropped"] == 0,
        "dropped_records": footer["dropped"] if footer else None,
        "unfinished": dict(unfinished), "anomalies": dict(anomalies),
        "frame_outcomes": dict(outcomes), "http_requests": dict(http_counts),
        "http_response_bytes": dict(http_bytes),
        "metrics": {key: distribution(values) for key, values in sorted(metrics.items())},
    }


def read_trace(path):
    records = []
    for line in path.read_text().splitlines():
        try:
            records.append(json.loads(line))
        except json.JSONDecodeError as error:
            raise ValueError("Truncated or invalid trace; capture did not finish cleanly") from error
    return records


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("trace", type=Path)
    args = parser.parse_args()
    print(json.dumps(summarize(read_trace(args.trace)), indent=2))
