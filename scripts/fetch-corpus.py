#!/usr/bin/env python3
"""Fetch the reference corpus this project's benchmarks are measured against.

The corpus is a fixed window of a dated Wikipedia snapshot, so the same command yields the same
documents on any machine at any time. Nothing here is generated: every document is real text, and the
provenance is recorded in corpus/README.md beside the licence it carries.

Usage:
    python3 scripts/fetch-corpus.py            # write corpus/reference.tsv
    python3 scripts/fetch-corpus.py --smoke    # write corpus/smoke.tsv, the vendored subset
"""

import argparse
import hashlib
import json
import pathlib
import sys
import time
import urllib.parse
import urllib.request

# A dated snapshot, not "latest": a corpus that drifts silently invalidates every recorded number.
SNAPSHOT = "20231101"
DATASET = "wikimedia/wikipedia"
# Three languages that differ morphologically, plus English. The embedding model is multilingual, so a
# single-language corpus would measure the easy half of what it claims.
LANGUAGES = ("de", "fr", "pl", "en")
# A fixed window. The offset skips the alphabetically first articles, which are unusually short stubs.
OFFSET = 1_000
FULL_PER_LANGUAGE = 500
SMOKE_PER_LANGUAGE = 10
# The server caps a page; the loop below pages through.
PAGE = 100

ENDPOINT = "https://datasets-server.huggingface.co/rows"


def escape(text: str) -> str:
    """Escape the characters the line format uses, and the escape itself."""
    return text.replace("\\", "\\\\").replace("\t", "\\t").replace("\n", "\\n").replace("\r", "\\r")


def fetch_page(language: str, offset: int, length: int) -> list[dict]:
    """One page of rows, or an exception after the retries are spent."""
    query = urllib.parse.urlencode(
        {
            "dataset": DATASET,
            "config": f"{SNAPSHOT}.{language}",
            "split": "train",
            "offset": offset,
            "length": length,
        }
    )
    last_error: Exception | None = None
    for attempt in range(5):
        try:
            with urllib.request.urlopen(f"{ENDPOINT}?{query}", timeout=60) as response:
                return json.load(response).get("rows", [])
        except Exception as error:  # noqa: BLE001 - any transport failure is retried the same way
            last_error = error
            time.sleep(2 * (attempt + 1))
    raise RuntimeError(f"could not fetch {language} at offset {offset}: {last_error}")


def collect(per_language: int) -> list[str]:
    """One line per document: id, language, title, text."""
    lines: list[str] = []
    for language in LANGUAGES:
        taken = 0
        while taken < per_language:
            want = min(PAGE, per_language - taken)
            for row in fetch_page(language, OFFSET + taken, want):
                record = row["row"]
                text = record.get("text", "")
                # A stub carries no signal for retrieval and would flatter every recall number.
                if len(text) < 500:
                    continue
                lines.append(
                    "\t".join(
                        (
                            f"{language}-{record['id']}",
                            language,
                            escape(record.get("title", "")),
                            escape(text),
                        )
                    )
                )
            taken += want
            print(f"  {language}: {taken}/{per_language}", file=sys.stderr)
    return lines


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--smoke", action="store_true", help="write the small vendored subset instead")
    arguments = parser.parse_args()

    per_language = SMOKE_PER_LANGUAGE if arguments.smoke else FULL_PER_LANGUAGE
    target = pathlib.Path("corpus") / ("smoke.tsv" if arguments.smoke else "reference.tsv")

    lines = collect(per_language)
    body = "\n".join(lines) + "\n"
    target.parent.mkdir(exist_ok=True)
    target.write_text(body, encoding="utf-8")

    digest = hashlib.sha256(body.encode("utf-8")).hexdigest()
    print(f"{target}: {len(lines)} documents, sha256 {digest}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
