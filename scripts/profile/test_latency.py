import unittest

from latency import distribution, summarize


def row(kind, phase, at, ident=1, label="sample", owner=1, parent=0, value=0):
    return dict(kind=kind, phase=phase, at_us=at, id=ident, label=label,
                owner=owner, parent=parent, value=value)


def report(*rows, dropped=0):
    return summarize([{"schema": 1}, *rows, {"summary": True, "dropped": dropped}])


class LatencyTests(unittest.TestCase):
    def test_request_stages_are_separate_even_if_writer_order_differs(self):
        result = report(
            row("request", "finished", 18000, label="history"),
            row("request", "created", 1000, label="history"),
            row("request", "submitted", 4000, label="history"),
            row("request", "started", 8000, label="history"),
            row("request", "begin", 20000, label="completion"),
            row("request", "end", 25000, label="completion"),
        )
        for name, expected in dict(admission=3, queue=4, worker=10, delivery=2, apply=5).items():
            self.assertEqual(result["metrics"][f"request.history.{name}"]["p50_ms"], expected)

    def test_missing_pairs_and_abandoned_actions_do_not_improve_percentiles(self):
        result = report(row("interaction", "begin", 0),
                        row("interaction", "abandoned", 5000),
                        row("http", "begin", 1000))
        self.assertEqual(result["metrics"], {})
        self.assertEqual(result["unfinished"], {"http": 1})
        self.assertEqual(result["anomalies"], {"abandoned": 1})

    def test_tool_ids_are_scoped_and_source_clocks_are_kept_separate(self):
        result = report(
            row("provider", "received", 1000, ident=1, label="tool.called", parent=4, value=100),
            row("provider", "received", 2000, ident=2, label="tool.called", owner=2, parent=4, value=200),
            row("provider", "received", 7000, ident=3, label="tool.success", parent=4, value=104),
            row("provider", "received", 9000, ident=4, label="step.started", parent=5),
            row("provider", "received", 12000, ident=5, label="step.first-token", parent=5),
        )
        metrics = result["metrics"]
        self.assertEqual(metrics["tool.observed_call_to_result"]["p50_ms"], 6)
        self.assertEqual(metrics["tool.provider_call_to_result"]["p50_ms"], 4)
        self.assertEqual(metrics["tool.last_result_to_next_step"]["p50_ms"], 2)
        self.assertEqual(metrics["model.observed_step_to_first_token"]["p50_ms"], 3)

    def test_loss_and_unclean_shutdown_are_explicit(self):
        self.assertFalse(report(dropped=2)["complete_capture"])
        self.assertFalse(summarize([{"schema": 1}])["complete_capture"])
        with self.assertRaises(ValueError):
            summarize([{"schema": 7}])

    def test_frame_cpu_work_is_not_a_presentation_interval(self):
        result = report(row("frame", "update", 2000, label="drawn", value=1000),
                        row("frame", "encode", 3000, label="drawn", value=500),
                        row("frame", "update", 5000, ident=2, label="skipped"))
        self.assertEqual(result["metrics"]["frame.update"]["p50_ms"], 1)
        self.assertEqual(result["metrics"]["frame.encode"]["p50_ms"], .5)
        self.assertEqual(result["frame_outcomes"], {"drawn": 1, "skipped": 1})

    def test_percentiles_include_outliers_and_single_samples(self):
        values = distribution(list(range(1, 101)))
        self.assertEqual((values["p50_ms"], values["p95_ms"], values["p99_ms"]), (50.5, 95, 99))
        self.assertEqual(distribution([42])["p99_ms"], 42)
        self.assertIsNone(distribution([]))


if __name__ == "__main__":
    unittest.main()
