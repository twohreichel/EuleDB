#!/usr/bin/env python3
"""Check the properties of .github/dependabot.yml that matter, and that nothing else checks.

GitHub validates the file's syntax and reports a broken one in the repository's dependency graph, which
nobody looks at until an update stops arriving. What it does not check is the policy: that both
ecosystems are covered, that no `ignore` entry is quietly suppressing security updates, that a cooldown
exists, and that major versions are not swept into a group with patches.

Deliberately no schema fetch. Validating against the published JSON schema is a better syntax check, but
it makes a gate depend on a third-party URL at runtime, and a gate that fails when someone else's CDN is
down is a gate people learn to ignore.
"""

from __future__ import annotations

import pathlib
import sys

import yaml

CONFIG = pathlib.Path(__file__).resolve().parent.parent / ".github" / "dependabot.yml"
REQUIRED_ECOSYSTEMS = {"cargo", "github-actions"}


def problems(config: dict) -> list[str]:
    """Return one message per policy violation, empty when the configuration is sound."""
    found: list[str] = []
    updates = config.get("updates") or []

    covered = {entry.get("package-ecosystem") for entry in updates}
    for missing in sorted(REQUIRED_ECOSYSTEMS - covered):
        found.append(f"no update entry for the {missing} ecosystem")

    for entry in updates:
        eco = entry.get("package-ecosystem", "<unnamed>")

        if "ignore" in entry:
            found.append(
                f"{eco}: has an `ignore` entry. It suppresses security updates as well as version "
                f"updates, so hold a version back in the manifest instead, where it is visible"
            )

        if entry.get("schedule", {}).get("interval") != "weekly":
            found.append(f"{eco}: schedule is not weekly")

        if "cooldown" not in entry:
            found.append(f"{eco}: no cooldown, so a release is proposed before it has settled")

        for name, group in (entry.get("groups") or {}).items():
            if "major" in (group.get("update-types") or []):
                found.append(
                    f"{eco}: group `{name}` includes major updates. Each breaking change deserves "
                    f"its own pull request, its own CI run and its own decision"
                )

    return found


def main() -> int:
    """Report every violation at once, so one run tells you everything to fix."""
    if not CONFIG.is_file():
        print(f"error: {CONFIG} does not exist", file=sys.stderr)
        return 1

    found = problems(yaml.safe_load(CONFIG.read_text(encoding="utf-8")) or {})
    for message in found:
        print(f"error: {message}", file=sys.stderr)
    if found:
        return 1

    print(f"ok: {CONFIG.name} covers {', '.join(sorted(REQUIRED_ECOSYSTEMS))}, weekly, with a cooldown")
    print("ok: no ignore entry, and no group sweeps up major versions")
    return 0


if __name__ == "__main__":
    sys.exit(main())
