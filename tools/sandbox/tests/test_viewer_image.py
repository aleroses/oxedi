"""Tests of viewer_image.py: the pins it reads and the tag it derives, on temporary trees only."""

import hashlib
import os
import stat
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))
import viewer_image  # noqa: E402
from viewer_image import WasmPins  # noqa: E402

REPO = HERE.parents[2]
DOCS = "playwright==1.63.0 \\\n    --hash=sha256:aaaa\n"


def write(root: Path, relative: str, text: str) -> None:
    path = root / relative
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def tree(lock: str = "1.63.0", docs: str = "1.63.0") -> Path:
    root = Path(tempfile.mkdtemp())
    write(root, viewer_image.PLAYWRIGHT_LOCK, f"playwright=={lock} \\\n    --hash=sha256:bbbb\n")
    write(root, viewer_image.DOCS_REQUIREMENTS, f"playwright=={docs}\n    # via pytest-playwright\n")
    write(root, viewer_image.INSTALL_TOOLS, (REPO / viewer_image.INSTALL_TOOLS).read_text())
    write(root, viewer_image.DOCKERFILE, "FROM base\n")
    write(root, viewer_image.VIEWER_IMAGE, "# the module\n")
    return root


class Pins(unittest.TestCase):
    def test_wasm_pins_from_the_script(self):
        pins = viewer_image.wasm_pins((REPO / viewer_image.INSTALL_TOOLS).read_text())
        self.assertEqual(
            pins,
            WasmPins(
                "0.2.129",
                "82d12bb940e2d4e72e0d5605387fc1b8ca179044e012b620f0ce4e7440e8320e",
                "133",
                "2dc9c7813f5375db93d96ead4b78222fcc3e2677bbb832297af4797782a37489",
            ),
        )

    def test_a_script_without_a_pin_names_the_missing_variable(self):
        with self.assertRaises(SystemExit) as raised:
            viewer_image.wasm_pins('WASM_BINDGEN_VERSION="1"\n')
        self.assertEqual(
            str(raised.exception),
            'sandbox-image-viewer: crates/oxedi_wasm/scripts/install_tools.sh has no '
            'WASM_BINDGEN_SHA256="<value>" line',
        )

    def test_playwright_pin_ignores_hashes(self):
        self.assertEqual(viewer_image.playwright_pin(DOCS), "1.63.0")
        self.assertEqual(viewer_image.playwright_pin("x==1\nplaywright==1.63.0\n    # via y\n"), "1.63.0")

    def test_a_file_with_no_pin_is_refused(self):
        with self.assertRaises(SystemExit) as raised:
            viewer_image.playwright_pin("pytest-playwright==0.7.2\n")
        self.assertEqual(
            str(raised.exception),
            "sandbox-image-viewer: docs/requirements.txt pins no playwright==<version>",
        )

    def test_pins_disagree(self):
        with self.assertRaises(SystemExit) as raised:
            viewer_image.build_args(tree(lock="1.63.0", docs="1.64.0"))
        self.assertEqual(
            str(raised.exception),
            "sandbox-image-viewer: tools/sandbox/playwright.txt pins playwright 1.63.0 but "
            "docs/requirements.txt pins 1.64.0; regenerate tools/sandbox/playwright.txt",
        )

    def test_build_args_carry_the_five_pins(self):
        args = viewer_image.build_args(tree())
        self.assertEqual(args[0::2], ["--build-arg"] * 5)
        self.assertEqual(
            args[1::2][0::2],
            ["WASM_BINDGEN_VERSION=0.2.129", "BINARYEN_VERSION=133", "PLAYWRIGHT_VERSION=1.63.0"],
        )
        self.assertEqual(len(args[1::2]), 5)

    def test_a_missing_file_is_one_line(self):
        with self.assertRaises(SystemExit) as raised:
            viewer_image.build_args(Path(tempfile.mkdtemp()))
        self.assertTrue(str(raised.exception).startswith("sandbox-image-viewer: cannot read "))


class Tag(unittest.TestCase):
    def test_tag_changes_with_each_input(self):
        root = tree()
        original = viewer_image.tag(root, "oxedi-sandbox:abc")
        self.assertRegex(original, r"^oxedi-sandbox-viewer:[0-9a-f]{12}$")
        tags = {viewer_image.tag(root, "oxedi-sandbox:abd")}
        for relative in viewer_image.TAG_INPUTS:
            path = root / relative
            text = path.read_text()
            path.write_text(text + "\n#")
            tags.add(viewer_image.tag(root, "oxedi-sandbox:abc"))
            path.write_text(text)
        self.assertEqual(len(tags), 5)
        self.assertNotIn(original, tags)
        self.assertEqual(viewer_image.tag(root, "oxedi-sandbox:abc"), original)

    def test_the_command_line_prints_the_tag(self):
        root = tree()
        result = subprocess.run(
            [sys.executable, "-I", str(HERE.parent / "viewer_image.py"), "tag", "--root", str(root),
             "--base", "oxedi-sandbox:abc"],
            capture_output=True, text=True, check=False, timeout=30,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), viewer_image.tag(root, "oxedi-sandbox:abc"))


class TreeDigest(unittest.TestCase):
    def make(self, files: dict[str, str]) -> Path:
        root = Path(tempfile.mkdtemp())
        for name, text in files.items():
            write(root, name, text)
        return root

    def test_markers_ignored(self):
        first = self.make({"a/x": "1", "b": "2"})
        second = self.make({"b": "2", "a/x": "1", "a/INSTALLATION_COMPLETE": "", "DEPENDENCIES_VALIDATED": ""})
        (second / "a" / "INSTALLATION_COMPLETE").chmod(0o755)
        self.assertEqual(viewer_image.tree_digest(first), viewer_image.tree_digest(second))

    def test_content_and_path_both_count(self):
        base = viewer_image.tree_digest(self.make({"a": "1"}))
        self.assertNotEqual(base, viewer_image.tree_digest(self.make({"a": "2"})))
        self.assertNotEqual(base, viewer_image.tree_digest(self.make({"b": "1"})))

    def test_mode_changes_digest(self):
        plain, executable = self.make({"a": "1"}), self.make({"a": "1"})
        (plain / "a").chmod(0o644)
        (executable / "a").chmod(0o755)
        self.assertNotEqual(viewer_image.tree_digest(plain), viewer_image.tree_digest(executable))

    def test_symlink_target_changes_digest(self):
        trees = []
        for target in ("x", "y"):
            root = self.make({"x": "1", "y": "1"})
            (root / "link").symlink_to(target)
            trees.append(root)
        without = self.make({"x": "1", "y": "1"})
        digests = {viewer_image.tree_digest(root) for root in (*trees, without)}
        self.assertEqual(len(digests), 3)

    def test_a_linked_directory_is_not_followed(self):
        root = self.make({"d/x": "1"})
        (root / "e").symlink_to("d")
        (root / "d").chmod(0o755)
        (root / "d" / "x").chmod(0o644)
        lines = (
            f"{hashlib.sha256(b'1').hexdigest()}  0644  d/x\n",
            # A link's own mode is the platform's (0777 on Linux).
            f"link  {stat.S_IMODE(os.lstat(root / 'e').st_mode):04o}  e -> d\n",
        )
        self.assertEqual(viewer_image.tree_digest(root), hashlib.sha256("".join(lines).encode()).hexdigest())

    def test_a_known_tree_has_a_known_digest(self):
        root = self.make({"a": "1"})
        (root / "a").chmod(0o644)
        line = hashlib.sha256(b"1").hexdigest() + "  0644  a\n"
        self.assertEqual(viewer_image.tree_digest(root), hashlib.sha256(line.encode()).hexdigest())

if __name__ == "__main__":
    unittest.main()
