#!/usr/bin/env python3
"""Repeat a packaged macOS latency workload with native CPU/RSS sampling."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import signal
import subprocess
import sys
import time

from latency import read_trace, summarize
from macos import processes
from native_windows import WindowObserver, summarize_windows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--workload", choices=("small", "streaming"), required=True)
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--no-trace", action="store_true", help="Resource-only observer-overhead comparison")
    args = parser.parse_args()
    if platform.system() != "Darwin" or not 1 <= args.runs <= 20:
        parser.error("Use macOS and 1–20 runs")
    app = args.app.resolve()
    executable = str(app / "Contents/MacOS/caelum-desktop")
    binary_hash = hashlib.sha256(Path(executable).read_bytes()).hexdigest()
    args.output.mkdir(parents=True, exist_ok=True)
    observer = WindowObserver()
    reports = []
    for run in range(1, args.runs + 1):
        trace = (args.output / f"{args.workload}-{run}.jsonl").resolve()
        resource = args.output / f"{args.workload}-{run}-resources.json"
        windows = args.output / f"{args.workload}-{run}-windows.json"
        if trace.exists() or resource.exists() or windows.exists():
            parser.error("Output already exists; choose a new capture directory")
        previous = set(processes())
        launch = subprocess.Popen(["open", "-W", "-n", str(app), "--env", f"CAELUM_LATENCY_TRACE={'' if args.no_trace else trace}",
                                   "--args", "--latency-workload", args.workload])
        deadline = time.monotonic() + 10
        pid = None
        while time.monotonic() < deadline:
            pid = next((p for p, info in processes().items()
                        if p not in previous and info["executable"] == executable), None)
            if pid:
                break
            time.sleep(.1)
        if not pid:
            launch.terminate()
            launch.wait()
            raise RuntimeError("Native workload did not launch")
        sampler = None
        observations = []
        try:
            sampler = subprocess.Popen([sys.executable, str(Path(__file__).with_name("macos.py")),
                                        "--pid", str(pid), "--label", args.workload,
                                        "--duration", "15", "--output", str(resource)],
                                       stdout=subprocess.DEVNULL)
            started = time.monotonic()
            while launch.poll() is None and time.monotonic() - started < 45:
                observations.append({"seconds": time.monotonic() - started, **observer.sample(pid)})
                time.sleep(.5)
            if launch.poll() is None:
                raise RuntimeError("Native workload did not finish")
            if launch.returncode or sampler.wait(timeout=10):
                raise RuntimeError("Native launch or resource sampling failed")
        finally:
            windows.write_text(json.dumps(observations, indent=2) + "\n")
            if sampler is not None and sampler.poll() is None:
                sampler.terminate()
                sampler.wait()
            info = processes().get(pid)
            if info and info["executable"] == executable:
                os.kill(pid, signal.SIGTERM)
            if launch.poll() is None:
                launch.terminate()
                launch.wait()
        report = {} if args.no_trace else summarize(read_trace(trace))
        if not args.no_trace:
            names = ("interaction.navigate.drawn", "interaction.navigate.ready", "interaction.edit.drawn")
            report["complete_workload"] = (report["complete_capture"] and not report["anomalies"]
                and all(report["metrics"].get(name, {}).get("count") == 80 for name in names)
                and report["workload_events"].get("stream", 0) == (2280 if args.workload == "streaming" else 0))
        report["resources"] = json.loads(resource.read_text())["summary"]
        report["window"] = summarize_windows(observations)
        report["run"] = run
        reports.append(report)
        print(json.dumps({"run": run, "complete_workload": report.get("complete_workload"),
                           "metrics": report.get("metrics"), "resources": report["resources"],
                           "window": report["window"]}), flush=True)
    result = {
        "workload": args.workload,
        "tracing": not args.no_trace,
        "binary_sha256": binary_hash,
        "revision": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(),
        "dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], text=True)),
        "runs": reports,
    }
    (args.output / f"{args.workload}-summary.json").write_text(json.dumps(result, indent=2) + "\n")
    if not args.no_trace and not all(report["complete_workload"] for report in reports):
        raise RuntimeError("Incomplete native workload; inspect the saved report before comparing timings")
    if not all(report["window"]["stable_front_window"] for report in reports):
        raise RuntimeError("Native window moved, lost the front position or was not observed throughout; inspect the saved captures")


if __name__ == "__main__":
    main()
