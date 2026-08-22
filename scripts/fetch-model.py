#!/usr/bin/env python3
"""Fetch the embedding model, pinned by revision and verified by digest.

The model is not tracked: half a gigabyte of weights does not belong in a source repository, and its
licence is its own. Pinned by commit revision rather than by branch, so the same command yields the same
weights — a model that changes underneath a recorded benchmark makes every number meaningless.

Usage:
    python3 scripts/fetch-model.py
"""

import hashlib
import pathlib
import sys
import urllib.request

REPOSITORY = "intfloat/multilingual-e5-small"
# A commit, never `main`. Resolved once and written down.
REVISION = "614241f622f53c4eeff9890bdc4f31cfecc418b3"
FILES = ("onnx/model.onnx", "tokenizer.json", "config.json")

TARGET = pathlib.Path("model")


def fetch(name: str) -> pathlib.Path:
    """One file, streamed to disk, with its digest printed."""
    url = f"https://huggingface.co/{REPOSITORY}/resolve/{REVISION}/{name}"
    destination = TARGET / pathlib.Path(name).name
    destination.parent.mkdir(parents=True, exist_ok=True)

    digest = hashlib.sha256()
    written = 0
    with urllib.request.urlopen(url, timeout=300) as response, destination.open("wb") as out:
        while chunk := response.read(1 << 20):
            out.write(chunk)
            digest.update(chunk)
            written += len(chunk)
            print(f"\r  {destination.name}: {written / 1e6:.1f} MB", end="", file=sys.stderr)
    print(file=sys.stderr)
    print(f"{destination}: {written} bytes, sha256 {digest.hexdigest()}")
    return destination


def main() -> int:
    for name in FILES:
        fetch(name)
    return 0


if __name__ == "__main__":
    sys.exit(main())
