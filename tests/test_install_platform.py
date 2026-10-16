#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Run the whole installer against a real local HTTP fixture server.

Only download origins are redirected in a temporary copy. A uname fixture
selects OS/architecture independently of the CI host. No shell code is extracted.
Run: python3 tests/test_install_platform.py -v
"""
import hashlib
import http.server
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import threading
import unittest


INSTALLER = (Path(__file__).resolve().parents[1] / "install.sh").read_text()
SHELL = shutil.which("sh")


@unittest.skipUnless(os.name == "posix" and SHELL and shutil.which("curl"), "requires POSIX shell and curl")
class InstallPlatformTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        class Handler(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                body = self.server.fixtures.get(self.path)
                self.send_response(200 if body is not None else 404)
                self.end_headers()
                self.wfile.write(body if body is not None else b"not found")

            def log_message(self, *_args):
                pass

        cls.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        cls.server.fixtures = {}
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        cls.server.server_close()
        cls.thread.join()

    def install(self, variables=None, arch="aarch64", assets=None, corrupt_checksum=False):
        env = os.environ.copy()
        for name in ("TERMUX_VERSION", "ANDROID_ROOT", "PREFIX", "TINA4_INSTALL_DIR"):
            env.pop(name, None)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            env["HOME"] = str(root)
            env["TMPDIR"] = str(root)
            env.update({key: value.replace("{root}", directory) for key, value in (variables or {}).items()})
            bindir = root / "tools"
            bindir.mkdir()
            uname = bindir / "uname"
            uname.write_text(f'#!{SHELL}\ncase "$1" in -s) echo Linux;; -m) echo {arch};; esac\n')
            uname.chmod(0o755)
            env["PATH"] = str(bindir) + os.pathsep + env["PATH"]
            names = assets if assets is not None else [
                f"tina4-linux{libc}-{cpu}"
                for libc in ("", "-musl") for cpu in ("amd64", "arm64")
            ]
            binary = f"#!{SHELL}\necho fixture-cli\n".encode()
            digest = "0" * 64 if corrupt_checksum else hashlib.sha256(binary).hexdigest()
            release = "/tina4stack/tina4/releases/download/v-test/"
            self.server.fixtures = {
                "/repos/tina4stack/tina4/releases/latest": json.dumps({
                    "tag_name": "v-test", "assets": [{"name": name} for name in names],
                }, indent=2).encode(),
                release + "SHA256SUMS": "".join(f"{digest}  {name}\n" for name in names).encode(),
                **{release + name: binary for name in names},
            }
            origin = f"http://127.0.0.1:{self.server.server_port}/"
            installer = root / "install.sh"
            installer.write_text(INSTALLER.replace("https://api.github.com/", origin)
                                 .replace("https://github.com/", origin))
            result = subprocess.run(
                [SHELL, str(installer)], env=env, text=True, capture_output=True,
                stdin=subprocess.DEVNULL, start_new_session=True, timeout=30,
            )
            destination = Path(env.get("TINA4_INSTALL_DIR") or
                               (env["PREFIX"] + "/bin" if env.get("PREFIX") else root / ".local/bin"))
            installed = destination / "tina4"
            if result.returncode == 0:
                self.assertEqual(installed.read_bytes(), binary)
                self.assertTrue(os.access(installed, os.X_OK))
                self.assertEqual(subprocess.check_output([str(installed)], text=True), "fixture-cli\n")
            else:
                self.assertFalse(installed.exists())
            return result, str(destination)

    def test_termux_selects_musl_on_both_architectures_and_aliases(self):
        for arch, asset in (("aarch64", "arm64"), ("arm64", "arm64"),
                            ("x86_64", "amd64"), ("amd64", "amd64")):
            with self.subTest(arch=arch):
                result, destination = self.install({"TERMUX_VERSION": "0.118.3", "PREFIX": "{root}/usr"}, arch)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn(f"Downloading tina4-linux-musl-{asset}...", result.stdout)
                self.assertTrue(destination.endswith("/usr/bin"))

    def test_android_without_termux_uses_user_directory(self):
        result, destination = self.install({"ANDROID_ROOT": "/system"})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("Downloading tina4-linux-musl-arm64...", result.stdout)
        self.assertTrue(destination.endswith("/.local/bin"))

    def test_termux_without_prefix_uses_user_directory(self):
        result, destination = self.install({"TERMUX_VERSION": "0.118.3"})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(destination.endswith("/.local/bin"))

    def test_explicit_directory_takes_precedence(self):
        result, destination = self.install({"TERMUX_VERSION": "0.118.3", "PREFIX": "{root}/usr",
                                            "TINA4_INSTALL_DIR": "{root}/custom/bin"})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(destination.endswith("/custom/bin"))

    def test_regular_linux_keeps_glibc(self):
        for variables in ({}, {"PREFIX": "{root}/prefix"}, {"TERMUX_VERSION": "", "ANDROID_ROOT": ""}):
            with self.subTest(variables=variables):
                result, _ = self.install({**variables, "TINA4_INSTALL_DIR": "{root}/bin"})
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn("Downloading tina4-linux-arm64...", result.stdout)

    def test_android_refuses_glibc_fallback_when_musl_missing(self):
        result, _ = self.install({"ANDROID_ROOT": "/system"}, assets=["tina4-linux-arm64"])
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("No matching binary found for linux-musl/", result.stderr)

    def test_checksum_mismatch_does_not_install(self):
        result, _ = self.install({"TERMUX_VERSION": "0.118.3"}, corrupt_checksum=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("checksum mismatch", result.stderr)

    def test_unsupported_architecture_does_not_install(self):
        result, _ = self.install({"TERMUX_VERSION": "0.118.3"}, arch="armv7l")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Unsupported architecture", result.stderr)


if __name__ == "__main__":
    unittest.main()
