"""Focused verification of the bundled C foundations course (not app regression)."""
from __future__ import annotations

import json
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
COURSE = ROOT / "courses" / "c-foundations"
EXPECTED = {
    "c01-first-program": ("", "Hello, C!\n"),
    "c02-types-variables": ("", "Count: 3\nPrice: 12.5\nLevel: A\n"),
    "c03-expressions": ("", "Integer: 3\nFloating: 3.5\n"),
    "c04-input-output": ("12 8\n", "Sum: 20\n"),
    "c05-conditions": ("", "Grade: B\n"),
    "c06-loops": ("", "Sum 1..5: 15\n"),
    "c07-functions": ("", "Square of 4: 16\n"),
    "c08-arrays": ("", "Total: 36\nMax: 12\n"),
    "c09-characters-strings": ("", "Length: 3\nEqual: no\n"),
    "c10-pointers": ("", "Before: 3 7\nAfter: 7 3\n"),
    "c11-arrays-pointers": ("", "Sum: 20\n"),
    "c12-structures": ("", "Pencil x3: 45 cents\n"),
    "c13-dynamic-memory": ("", "Sum: 20\n"),
    "c14-file-io": ("", "Read back: Stock: 4\n"),
    "c15-modules": ("", "Combined: 15\n"),
    "c16-debugging": ("", "Observed total: 15\n"),
    "c17-capstone": ("", "Pencil x3: 45 cents\nNotebook x2: 120 cents\nTotal: 165 cents\n"),
}


def relative_file(root: Path, value: str) -> Path:
    parts = value.split("/")
    assert parts and all(
        part not in {"", ".", ".."} and "\\" not in part and ":" not in part
        for part in parts
    ), f"Unsafe course reference: {value}"
    result = root.joinpath(*parts)
    assert result.is_file() and not result.is_symlink(), f"Missing or linked course file: {value}"
    return result


def check() -> None:
    manifest = json.loads((COURSE / "course.json").read_text(encoding="utf-8"))
    assert manifest["id"] == "c-foundations" and manifest["version"] == "1.0"
    chapters = manifest["chapters"]
    assert len(chapters) == len(EXPECTED)
    assert {chapter["id"] for chapter in chapters} == set(EXPECTED)
    markdown_paths: set[Path] = set()

    with tempfile.TemporaryDirectory(prefix="codelearn-course-check-") as directory:
        work = Path(directory)
        for chapter in chapters:
            cid = chapter["id"]
            assert chapter["title"].strip()
            blocks = chapter["blocks"]
            assert [block["kind"] for block in blocks] == ["content", "program", "content"], cid
            assert len({block["id"] for block in blocks}) == len(blocks), cid
            for block in (blocks[0], blocks[2]):
                markdown = relative_file(COURSE, block["markdown"])
                assert markdown.suffix == ".md" and markdown not in markdown_paths, cid
                markdown_paths.add(markdown)
                assert len(markdown.read_text(encoding="utf-8")) > 250, cid

            block = blocks[1]
            source_dir = COURSE / block["directory"]
            assert source_dir.is_dir() and not source_dir.is_symlink(), cid
            sources = [relative_file(source_dir, name) for name in block["files"]]
            assert len(sources) == len(set(sources)), cid
            assert all(file.suffix in {".c", ".h"} for file in sources), cid
            targets = block["targets"]
            to_compile = [relative_file(source_dir, name) for name in
                          [targets["primary"], *targets["additional"]]]
            assert all(file in sources and file.suffix == ".c" for file in to_compile), cid
            assert len(to_compile) == len(set(to_compile)), cid
            for include in targets["includes"]:
                if include != ".":
                    assert (source_dir / include).is_dir(), cid

            run_dir = work / cid
            run_dir.mkdir()
            binary = run_dir / "program"
            build = subprocess.run(
                ["gcc", "-std=c11", "-Wall", "-Wextra", "-Werror", "-pedantic", "-g",
                 *map(str, to_compile), "-I", str(source_dir), "-o", str(binary)],
                text=True, capture_output=True, timeout=30,
            )
            assert build.returncode == 0, f"{cid} compile failed:\n{build.stderr}"
            stdin, expected = EXPECTED[cid]
            run = subprocess.run([str(binary)], input=stdin, text=True,
                                 capture_output=True, timeout=10, cwd=run_dir)
            assert run.returncode == 0 and run.stdout == expected, (
                f"{cid} output mismatch: code={run.returncode}, out={run.stdout!r}, "
                f"err={run.stderr!r}, expected={expected!r}"
            )
            if cid == "c04-input-output":
                invalid = subprocess.run([str(binary)], input="abc\n", text=True,
                                         capture_output=True, timeout=10, cwd=run_dir)
                assert invalid.returncode == 1 and invalid.stdout == "Invalid input\n", (
                    f"C04 invalid input mismatch: {invalid}"
                )
    assert len(markdown_paths) == 34
    print(f"Verified {len(chapters)} chapters, {len(markdown_paths)} reading blocks, and "
          f"{len(EXPECTED)} compiled and executed C programs.")


if __name__ == "__main__":
    check()
