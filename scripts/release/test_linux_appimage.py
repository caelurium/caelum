import struct
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from linux_appimage import audit_elf, clear_runtime_metadata, dependencies, host_library


class AppImageTests(unittest.TestCase):
    def test_unresolved_dependencies_fail_and_host_graphics_are_never_bundled(self):
        with self.assertRaisesRegex(SystemExit, "Unresolved"):
            dependencies("libxkbcommon.so.0 => not found")
        self.assertEqual(dependencies("\tlibexample.so.1 => /usr/lib/libexample.so.1 (0xabcd)"),
                         {"libexample.so.1": Path("/usr/lib/libexample.so.1")})
        for name in ("libc.so.6", "libm.so.6", "ld-linux-aarch64.so.1", "libvulkan.so.1", "libGLX_mesa.so.0", "libdrm.so.2"):
            self.assertTrue(host_library(name), name)
        self.assertFalse(host_library("libxkbcommon.so.0"))

    def test_architecture_glibc_floor_and_machine_specific_linkage_are_enforced(self):
        with tempfile.TemporaryDirectory() as temporary:
            binary = Path(temporary) / "caelum-desktop"
            header = bytearray(20)
            header[:6] = b"\x7fELF\x02\x01"
            struct.pack_into("<H", header, 18, 62)
            binary.write_bytes(header)
            with patch("linux_appimage.command", side_effect=["GLIBC_2.2.5 GLIBC_2.35", "NEEDED libc.so.6"]):
                audit_elf(binary, "x86_64")
            with self.assertRaisesRegex(SystemExit, "native aarch64"):
                audit_elf(binary, "aarch64")
            with patch("linux_appimage.command", return_value="GLIBC_2.36"):
                with self.assertRaisesRegex(SystemExit, "GLIBC_2.36"):
                    audit_elf(binary, "x86_64")
            with patch("linux_appimage.command", side_effect=["GLIBC_2.35", "interpreter /nix/store/example/ld.so"]):
                with self.assertRaisesRegex(SystemExit, "Nix dependency"):
                    audit_elf(binary, "x86_64")

    def test_reused_runtime_does_not_retain_the_packaging_tools_update_url_or_signature(self):
        runtime = bytearray(640)
        # Minimal ELF64 section table with string, update and signature sections.
        struct.pack_into("<HHIQQQIHHHHHH", runtime, 16, 2, 62, 1, 0, 0, 64, 0, 64, 0, 0, 64, 4, 1)
        names = b"\0.shstrtab\0.upd_info\0.sha256_sig\0"
        runtime[320:320 + len(names)] = names
        struct.pack_into("<IIQQQQIIQQ", runtime, 128, 1, 3, 0, 0, 320, len(names), 0, 0, 0, 0)
        struct.pack_into("<IIQQQQIIQQ", runtime, 192, 11, 1, 0, 0, 400, 32, 0, 0, 0, 0)
        struct.pack_into("<IIQQQQIIQQ", runtime, 256, 21, 1, 0, 0, 500, 32, 0, 0, 0, 0)
        runtime[400:432] = b"u" * 32
        runtime[500:532] = b"s" * 32
        result = clear_runtime_metadata(runtime)
        self.assertEqual(result[400:432], b"\0" * 32)
        self.assertEqual(result[500:532], b"\0" * 32)
        self.assertEqual(result[320:320 + len(names)], names)
