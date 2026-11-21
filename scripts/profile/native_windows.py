"""Content-free WindowServer observations for the packaged macOS workload."""

import ctypes
import plistlib


def select_window(rows, pid):
    visible = [row for row in rows if row.get("kCGWindowLayer") == 0
               and row.get("kCGWindowIsOnscreen") and row.get("kCGWindowAlpha", 0) > 0]
    owned = [row for row in visible if row.get("kCGWindowOwnerPID") == pid]
    return {
        "front_window_owned": bool(visible) and visible[0].get("kCGWindowOwnerPID") == pid,
        "bounds": [[row["kCGWindowBounds"][key] for key in ("X", "Y", "Width", "Height")]
                   for row in owned],
    }


def summarize_windows(samples):
    observed = [sample for sample in samples if sample["seconds"] >= 1]
    bounds = next((sample["bounds"][0] for sample in observed if len(sample["bounds"]) == 1), None)
    missing = sum(len(sample["bounds"]) != 1 for sample in observed)
    non_front = sum(not sample["front_window_owned"] for sample in observed)
    changed = sum(sample["bounds"] != [bounds] for sample in observed)
    gap = max((b["seconds"] - a["seconds"] for a, b in zip(observed, observed[1:])), default=0)
    return {
        "stable_front_window": bool(observed) and observed[-1]["seconds"] >= 18
            and len(observed) >= 18 and gap <= 1 and not (missing or non_front or changed),
        "bounds_points": bounds,
        "samples_after_warmup": len(observed),
        "last_sample_seconds": observed[-1]["seconds"] if observed else None,
        "max_sample_gap_seconds": gap,
        "missing_or_multiple_window_samples": missing,
        "non_front_samples": non_front,
        "different_bounds_samples": changed,
    }


class WindowObserver:
    def __init__(self):
        self.cf = ctypes.CDLL("/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation")
        self.cg = ctypes.CDLL("/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics")
        self.cg.CGWindowListCopyWindowInfo.argtypes = [ctypes.c_uint, ctypes.c_uint]
        self.cg.CGWindowListCopyWindowInfo.restype = ctypes.c_void_p
        self.cf.CFPropertyListCreateData.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_long,
                                                  ctypes.c_ulong, ctypes.c_void_p]
        self.cf.CFPropertyListCreateData.restype = ctypes.c_void_p
        self.cf.CFDataGetLength.argtypes = [ctypes.c_void_p]
        self.cf.CFDataGetLength.restype = ctypes.c_long
        self.cf.CFDataGetBytePtr.argtypes = [ctypes.c_void_p]
        self.cf.CFDataGetBytePtr.restype = ctypes.c_void_p
        self.cf.CFRelease.argtypes = [ctypes.c_void_p]
        self.cf.CFRelease.restype = None

    def sample(self, pid):
        info = self.cg.CGWindowListCopyWindowInfo(1 | 16, 0)
        if not info:
            raise RuntimeError("WindowServer information is unavailable")
        data = None
        try:
            data = self.cf.CFPropertyListCreateData(None, info, 100, 0, None)
            if not data:
                raise RuntimeError("Cannot decode WindowServer information")
            rows = plistlib.loads(ctypes.string_at(self.cf.CFDataGetBytePtr(data), self.cf.CFDataGetLength(data)))
            return select_window(rows, pid)
        finally:
            if data:
                self.cf.CFRelease(data)
            self.cf.CFRelease(info)
