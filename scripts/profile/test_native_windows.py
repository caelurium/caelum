import unittest

from native_windows import select_window, summarize_windows


def window(pid, bounds=(0, 0, 800, 900), **extra):
    return {"kCGWindowOwnerPID": pid, "kCGWindowLayer": 0, "kCGWindowIsOnscreen": True,
            "kCGWindowAlpha": 1, "kCGWindowBounds": dict(zip(("X", "Y", "Width", "Height"), bounds)),
            "kCGWindowName": "Private window title", **extra}


class WindowObservationTests(unittest.TestCase):
    def test_only_owned_geometry_and_front_position_survive_selection(self):
        rows = [window(20, kCGWindowLayer=25), window(10), window(20)]
        self.assertEqual(select_window(rows, 10), {"front_window_owned": True, "bounds": [[0, 0, 800, 900]]})
        self.assertFalse(select_window(list(reversed(rows)), 10)["front_window_owned"])
        self.assertEqual(select_window([window(10, kCGWindowIsOnscreen=False)], 10),
                         {"front_window_owned": False, "bounds": []})
        self.assertEqual(select_window([window(10, kCGWindowAlpha=0)], 10)["bounds"], [])

    def test_launch_is_ignored_but_later_occlusion_resize_and_missing_observations_fail(self):
        samples = [{"seconds": 0, **select_window([], 10)}]
        samples += [{"seconds": second, **select_window([window(10)], 10)} for second in range(1, 20)]
        self.assertTrue(summarize_windows(samples)["stable_front_window"])
        self.assertFalse(summarize_windows(samples[:10])["stable_front_window"])
        self.assertFalse(summarize_windows(samples[-1:])["stable_front_window"])
        self.assertFalse(summarize_windows(samples[:10] + samples[11:])["stable_front_window"])
        self.assertFalse(summarize_windows([])["stable_front_window"])
        for rows in ([window(20), window(10)], [window(10, bounds=(0, 0, 900, 900))], [],
                     [window(10), window(10, bounds=(0, 0, 300, 200))]):
            changed = samples[:]
            changed[10] = {"seconds": 10, **select_window(rows, 10)}
            self.assertFalse(summarize_windows(changed)["stable_front_window"])


if __name__ == "__main__":
    unittest.main()
