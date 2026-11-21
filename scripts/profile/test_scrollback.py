import unittest

from scrollback import complete_workload, summarize_scroll
from test_latency import row


def replay():
    records = []
    for i in range(1920):
        label = "scroll_up" if i < 960 else "scroll_down"
        at = i * 10_000
        y = 50_000 - 24 * (i if i < 960 else 1919 - i)
        records.extend((
            row("interaction", "begin", at, ident=i, label=label),
            row("interaction", "drawn", at + 2200, ident=i, label=label),
            row("frame", "update", at + 1000, ident=i, label="drawn", value=1500),
            row("frame", "encode", at + 1500, ident=i, label="drawn", value=500),
            row("scroll", "step", at + 2100, ident=i, value=i),
            row("scroll", "offset_mpt", at + 2101, ident=i, value=y * 1000),
            row("scroll", "commits", at + 2102, ident=i, value=i // 5),
        ))
    return records


class ScrollbackTests(unittest.TestCase):
    def test_active_phases_exclude_launch_and_settling_frames(self):
        records = replay()
        records.extend((row("frame", "update", 0, ident=9999, label="drawn", value=90000),
                        row("frame", "encode", 0, ident=9999, label="drawn", value=90000)))
        records[-1]["at_us"] = records[-2]["at_us"] = -1
        result = summarize_scroll(list(reversed(records)))
        self.assertTrue(result["complete"])
        for phase in result["phases"].values():
            self.assertEqual(phase["requests"], 960)
            self.assertEqual(phase["observed_steps"], 960)
            self.assertEqual(phase["frame_cpu"]["max_ms"], 2)
            self.assertEqual(phase["draw_callback_intervals"]["p50_ms"], 10)
            self.assertEqual(phase["frame_cpu_over_8_33_ms"], 0)

    def test_missing_draws_stationary_scroll_and_partial_observations_fail(self):
        records = replay()
        without_draw = [r for r in records if not (r["kind"] == "interaction" and r["id"] == 10 and r["phase"] == "drawn")]
        self.assertFalse(summarize_scroll(without_draw)["complete"])
        without_position = [r for r in records if not (r["kind"] == "scroll" and r["id"] == 10 and r["phase"] == "offset_mpt")]
        self.assertFalse(summarize_scroll(without_position)["complete"])
        for record in records:
            if record["phase"] == "offset_mpt":
                record["value"] = 1000
        self.assertFalse(summarize_scroll(records)["complete"])

    def test_coalesced_draws_are_observed_without_inventing_frames(self):
        records = [r for r in replay() if r["kind"] != "scroll" or r["id"] % 2]
        result = summarize_scroll(records)
        self.assertTrue(result["complete"])
        self.assertEqual(result["phases"]["scroll_up"]["observed_steps"], 480)
        self.assertEqual(result["phases"]["scroll_up"]["draw_callback_intervals"]["p50_ms"], 20)

    def test_counts_and_unfinished_interactions_gate_workload_completion(self):
        report = dict(complete_capture=True, anomalies={}, unfinished={},
                      scroll={"complete": True}, workload_events={"scroll": 1920, "stream": 1900})
        self.assertTrue(complete_workload(report, "scroll-streaming"))
        self.assertFalse(complete_workload(report, "scroll"))
        report["unfinished"]["interaction"] = 1
        self.assertFalse(complete_workload(report, "scroll-streaming"))
        report["unfinished"] = {"request": 2}
        self.assertTrue(complete_workload(report, "scroll-streaming"))
        report["workload_events"]["scroll"] = 1919
        self.assertFalse(complete_workload(report, "scroll-streaming"))


if __name__ == "__main__":
    unittest.main()
