#!/usr/bin/env python3
"""Generate NT-S2-004 transport conformance CI artifacts.

Outputs:
- JSON machine-readable transport outcome summary
- Markdown parity report for quick human review
"""

from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any, Dict, List


def _load_fixture(path: Path) -> Dict[str, Any]:
    data = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(data, dict):
        raise ValueError("fixture root must be an object")
    if not isinstance(data.get("cases"), list):
        raise ValueError("fixture must include array field 'cases'")
    return data


def _summarize_cases(cases: List[Dict[str, Any]]) -> Dict[str, Any]:
    by_mode = Counter()
    by_expected_transport = Counter()
    fallback_expected = 0
    error_cases = 0

    for case in cases:
        mode = str(case.get("transportMode", "unknown"))
        by_mode[mode] += 1

        expect = case.get("expect")
        if isinstance(expect, dict):
            active = expect.get("activeTransport")
            if isinstance(active, str):
                by_expected_transport[active] += 1
            if expect.get("fallbackTriggered") is True:
                fallback_expected += 1
        if isinstance(case.get("expectError"), dict):
            error_cases += 1

    return {
        "totalCases": len(cases),
        "byTransportMode": dict(sorted(by_mode.items())),
        "byExpectedActiveTransport": dict(sorted(by_expected_transport.items())),
        "fallbackExpectedCases": fallback_expected,
        "expectErrorCases": error_cases,
    }


def _write_json(path: Path, payload: Dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")


def _write_markdown(path: Path, payload: Dict[str, Any], fixture_rel: str) -> None:
    summary = payload["summary"]
    lines = [
        "# NT-S2-004 Transport Parity Report",
        "",
        f"- Language lane: `{payload['language']}`",
        f"- Suite status: `{payload['suiteStatus']}`",
        f"- Fixture: `{fixture_rel}`",
        f"- Schema version: `{payload.get('schemaVersion', 'unknown')}`",
        "",
        "## Summary",
        "",
        f"- Total cases: **{summary['totalCases']}**",
        f"- Fallback expected: **{summary['fallbackExpectedCases']}**",
        f"- Error expectation cases: **{summary['expectErrorCases']}**",
        "",
        "## Cases by transportMode",
        "",
    ]
    for key, value in summary["byTransportMode"].items():
        lines.append(f"- `{key}`: {value}")

    lines.extend(["", "## Cases by expected active transport", ""])
    for key, value in summary["byExpectedActiveTransport"].items():
        lines.append(f"- `{key}`: {value}")

    lines.extend(
        [
            "",
            "## Outcome",
            "",
            "- This artifact is generated in CI for transport-specific reporting.",
            "- It complements language-specific test logs and fixture validation.",
            "",
        ]
    )
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines), encoding="utf-8")


def main() -> None:
    parser = argparse.ArgumentParser(description="Generate transport conformance CI report")
    parser.add_argument("--fixture", required=True, help="Path to transport mode fixture JSON")
    parser.add_argument("--language", required=True, help="Driver language lane label")
    parser.add_argument("--suite-status", required=True, help="pass/fail outcome from job lane")
    parser.add_argument("--output-json", required=True, help="Output JSON report path")
    parser.add_argument("--output-md", required=True, help="Output Markdown report path")
    args = parser.parse_args()

    fixture_path = Path(args.fixture)
    data = _load_fixture(fixture_path)
    summary = _summarize_cases(data["cases"])

    payload: Dict[str, Any] = {
        "schemaVersion": data.get("schemaVersion", "unknown"),
        "language": args.language,
        "suiteStatus": args.suite_status,
        "fixturePath": str(fixture_path.as_posix()),
        "summary": summary,
    }

    _write_json(Path(args.output_json), payload)
    _write_markdown(Path(args.output_md), payload, str(fixture_path.as_posix()))


if __name__ == "__main__":
    main()
