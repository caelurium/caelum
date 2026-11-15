import json
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import unittest
from unittest.mock import patch

from capture import capture_all, capture_plan


class CaptureTests(unittest.TestCase):
    def test_parallel_work_is_bounded_and_manifest_keeps_plan_order(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            active = peak = 0
            lock = threading.Lock()
            barrier = threading.Barrier(2)

            def child(command, **kwargs):
                nonlocal active, peak
                name = command[0]
                with lock:
                    active += 1
                    peak = max(peak, active)
                barrier.wait(timeout=5)
                (output / f"{name}.png").write_bytes(b"\x89PNG\r\n\x1a\nfixture")
                (output / f"{name}.json").write_text('{"fixture": true}')
                with lock:
                    active -= 1
                return subprocess.CompletedProcess(command, 0, name, "")

            plan = [(f"capture-{index}", [f"capture-{index}"], None) for index in range(8)]
            with patch("capture.subprocess.run", side_effect=child):
                capture_all(plan, output, workers=2)
            manifest = json.loads((output / "manifest.json").read_text())
            self.assertEqual(peak, 2)
            self.assertEqual(manifest["completed"], 8)
            self.assertEqual([item["name"] for item in manifest["captures"]], [item[0] for item in plan])
            self.assertTrue(all((output / f"{item[0]}.log").read_text() == item[0] for item in plan))

    def test_failures_timeouts_and_missing_outputs_cannot_reuse_stale_success(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            plan = [
                ("failed", [sys.executable, "-c", "print('child failure'); raise SystemExit(3)"], None),
                ("timeout", [sys.executable, "-c", "import time; print('waiting', flush=True); time.sleep(30)"], None),
                ("missing", [sys.executable, "-c", "pass"], None),
            ]
            for name, _, _ in plan:
                (output / f"{name}.png").write_bytes(b"\x89PNG\r\n\x1a\nold")
                (output / f"{name}.json").write_text('{}')
            with self.assertRaisesRegex(RuntimeError, "3 captures failed"):
                capture_all(plan, output, workers=2, timeout=0.5)
            manifest = json.loads((output / "manifest.json").read_text())
            self.assertEqual(manifest["completed"], 0)
            self.assertEqual(len(manifest["captures"]), 3)
            self.assertIn("child failure", (output / "failed.log").read_text())
            self.assertIn("exceeded", (output / "timeout.log").read_text())
            self.assertFalse(list(output.glob("*.png")))

    def test_plan_covers_both_themes_sizes_and_interaction_inputs(self):
        output = Path("preview-results")
        plan = capture_plan(Path("caelum-desktop"), output)
        names = [item[0] for item in plan]
        self.assertEqual(len(names), len(set(names)))
        for theme in ("dark", "light"):
            for suffix in ("", "-small"):
                name = f"rune-{theme}-patch-expanded{suffix}"
                _, command, events = next(item for item in plan if item[0] == name)
                self.assertEqual(command[command.index("--at") + 1], "1.8")
                self.assertEqual(command[command.index("--script") + 1], str(output / f"{name}-input.json"))
                self.assertTrue(events)

    def test_invalid_worker_counts_and_duplicate_names_fail_before_launch(self):
        with tempfile.TemporaryDirectory() as directory, patch("capture.subprocess.run") as run:
            for workers in (0, 5):
                with self.assertRaises(ValueError):
                    capture_all([], Path(directory), workers)
            with self.assertRaisesRegex(ValueError, "unique"):
                capture_all([("same", [], None)] * 2, Path(directory), 1)
            run.assert_not_called()


if __name__ == "__main__":
    unittest.main()
