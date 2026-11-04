"""Tests of rust_version.py: every way rust-toolchain.toml can fail to name a version ends in one
`sandbox-image:` line and exit status 2, never a traceback."""

import contextlib
import io
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))
import rust_version  # noqa: E402


class ChannelTest(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp())
        self.addCleanup(lambda: shutil.rmtree(self.root))

    def refusal(self) -> str:
        with self.assertRaises(rust_version.Refused) as caught:
            rust_version.channel(self.root)
        return str(caught.exception)

    def write(self, text: str) -> None:
        (self.root / "rust-toolchain.toml").write_text(text, encoding="utf-8")

    def test_exact_channel_is_returned(self):
        self.write('[toolchain]\nchannel = "1.99.0"\n')
        self.assertEqual(rust_version.channel(self.root), "1.99.0")

    def test_missing_file_refuses(self):
        line = self.refusal()
        self.assertTrue(line.startswith(f"sandbox-image: cannot read rust-toolchain.toml under {self.root}: "), line)
        self.assertIn("No such file", line)

    def test_malformed_toml_refuses(self):
        self.write("[toolchain\n")
        line = self.refusal()
        self.assertTrue(line.startswith(f"sandbox-image: cannot read rust-toolchain.toml under {self.root}: "), line)

    def test_toolchain_that_is_not_a_table_refuses(self):
        self.write('toolchain = "1.99.0"\n')
        self.assertEqual(
            self.refusal(),
            'sandbox-image: rust-toolchain.toml has no [toolchain] table; write [toolchain] with channel = "X.Y.Z"')

    def test_channel_that_is_not_a_string_refuses(self):
        self.write("[toolchain]\nchannel = 199\n")
        self.assertEqual(
            self.refusal(),
            "sandbox-image: rust-toolchain.toml names no exact channel X.Y.Z (found '199'); "
            "set channel to a version such as 1.99.0")

    def test_main_prints_the_line_to_stderr_and_exits_2(self):
        self.write('[toolchain]\nchannel = "stable"\n')
        with self.assertRaises(SystemExit) as caught, contextlib.redirect_stderr(io.StringIO()) as err:
            rust_version.main(["--root", str(self.root)])
        self.assertEqual(caught.exception.code, 2)
        self.assertTrue(err.getvalue().startswith("sandbox-image: rust-toolchain.toml names no exact channel"))


if __name__ == "__main__":
    unittest.main()
