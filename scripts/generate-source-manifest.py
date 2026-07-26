#!/usr/bin/env python3
"""Generate or verify SanjuOS's canonical source-integrity manifest."""

from __future__ import annotations

import argparse
import hashlib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "SOURCE_MANIFEST.sha256"
BINARY_SUFFIXES = {
    ".elf",
    ".png",
    ".jpg",
    ".jpeg",
    ".gif",
    ".bmp",
    ".ico",
    ".woff",
    ".woff2",
    ".ttf",
    ".otf",
}
EXCLUDED_SUFFIXES = {".patch", ".zip", ".pyc"}


def source_files() -> list[Path]:
    return sorted(
        (
            path
            for path in ROOT.rglob("*")
            if path.is_file()
            and ".git" not in path.parts
            and "target" not in path.parts
            and "build" not in path.parts
            and "__pycache__" not in path.parts
            and path != MANIFEST
            and path.suffix not in EXCLUDED_SUFFIXES
        ),
        key=lambda path: path.relative_to(ROOT).as_posix(),
    )


def source_digest(path: Path) -> str:
    data = path.read_bytes()
    if path.suffix.lower() not in BINARY_SUFFIXES:
        data = data.replace(b"\r\n", b"\n").replace(b"\r", b"\n")
    return hashlib.sha256(data).hexdigest()


def rendered_manifest() -> str:
    return "".join(
        f"{source_digest(path)}  {path.relative_to(ROOT).as_posix()}\n"
        for path in source_files()
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--check",
        action="store_true",
        help="verify the retained manifest without changing it",
    )
    args = parser.parse_args()
    rendered = rendered_manifest()
    if args.check:
        if not MANIFEST.exists() or MANIFEST.read_text(encoding="utf-8") != rendered:
            raise SystemExit("SOURCE_MANIFEST.sha256 is out of date")
        print("SanjuOS source manifest is current.")
        return 0

    MANIFEST.write_text(rendered, encoding="utf-8", newline="\n")
    print(f"Updated {MANIFEST.relative_to(ROOT)} with {len(source_files())} files.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
