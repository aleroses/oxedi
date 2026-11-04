"""What a review of a change found, and its two reports: the text one, printed part by part as the
flow goes and written whole as report.txt, and report.json. Both go next to the clone.
"""

import json
import sys
from dataclasses import asdict, dataclass
from enum import Enum
from pathlib import Path

from deps import PackageDiff
from risk import Risk
from source import Change, Request


class Confirmed(Enum):
    """How the run was allowed to go on, in the order that wins when two confirmations differ."""

    REFUSED = "refused"
    TTY = "tty"
    ENV = "env"
    NOT_NEEDED = "not needed"


def combined(answers: list[Confirmed]) -> Confirmed:
    return next((answer for answer in Confirmed if answer in answers), Confirmed.NOT_NEEDED)


@dataclass(frozen=True)
class Tool:
    """An audit tool's exit status and everything it printed."""

    status: int
    output: str


@dataclass(frozen=True)
class VenvAudit:
    """pip-audit over one venv: the venv's place in the container, the exit status and everything
    it printed."""

    venv: str
    status: int
    output: str


@dataclass(frozen=True)
class Ran:
    """The target the run phase ran, its exit status and the audit's lines about the clone."""

    target: str
    status: int
    audit: tuple[str, ...]


@dataclass(frozen=True)
class Progress:
    """How far the containers went. `cargo_deny` is None when the flow stopped before it;
    `pip_audit` holds one entry per venv of the image, none when the flow stopped before the fetch
    or the fetch failed; `ran` is false when the run phase never started."""

    status: int
    cargo_deny: Tool | None
    pip_audit: tuple[VenvAudit, ...]
    answers: tuple[Confirmed, ...]
    ran: bool


@dataclass(frozen=True)
class Report:
    request: Request
    change: Change
    risk: Risk
    outcome: Progress
    run: Ran | None
    image: str
    runtime: str
    # What to rerun with when this image cannot run the tests of what the change touches, or "".
    image_hint: str


def describe_change(request: Request, change: Change) -> str:
    if change.kind == "ref":
        return f"ref {request.name}"
    if change.merged:
        return f"pull request {request.name} at its merge ref refs/pull/{request.name}/merge"
    return f"pull request {request.name} at its head refs/pull/{request.name}/head (HEAD=1: not merged with its base)"


def package_lines(title: str, diff: PackageDiff) -> list[str]:
    rows = [f"  added {n} {new} ({source})" for n, _, new, source in diff.added]
    rows += [f"  changed {n} {old} -> {new} ({source})" for n, old, new, source in diff.changed]
    rows += [f"  removed {n} {old} ({source})" for n, old, _, source in diff.removed]
    rows += [f"  cannot read: {error}" for error in diff.errors]
    return [f"{title}:", *(rows or ["  none"])]


def tool_lines(name: str, status: int, output: str) -> list[str]:
    return [f"{name}: exit {status}", *(f"  {line}" for line in output.splitlines())]


def deny_lines(deny: Tool | None) -> list[str]:
    return ["cargo-deny: not run"] if deny is None else tool_lines("cargo-deny", deny.status, deny.output)


def pip_audit_lines(audits: tuple[VenvAudit, ...]) -> list[str]:
    if not audits:
        return ["pip-audit: not run"]
    return [line for audit in audits for line in tool_lines(f"pip-audit {audit.venv}", audit.status, audit.output)]


def risk_lines(request: Request, change: Change, risk: Risk) -> list[str]:
    flagged_paths = [t for t in risk.touched if t.kinds]
    lines = [
        f"sandbox-pr report: {describe_change(request, change)}",
        f"tested {change.tested}, base {change.base}, head {change.head}",
        f"touched paths: {len(risk.touched)}, flagged: {len(flagged_paths)}",
    ]
    for item in risk.touched:
        lines.append(f"  {item.status} {item.path}")
        lines += [f"      {kind.name}: {kind.why}" for kind in item.kinds]
    lines += package_lines("Cargo.lock packages", risk.cargo)
    lines += package_lines("declared Python packages", risk.python)
    sources = risk.new_sources
    lines += ["new cargo sources:", *(f"  {url}" for url in sources.urls or ("none",))]
    lines += [f"  cannot read: {error}" for error in sources.unreadable]
    return lines + [f"flagged: {'yes' if risk.flagged else 'no'}"]


def hint_lines(hint: str) -> list[str]:
    """The image hint as report lines: none when the image fits the change."""
    return [hint] if hint else []


def render(report: Report) -> list[str]:
    outcome = report.outcome
    run = report.run
    run_lines = ["run: not run"] if run is None else [
        f"run: {run.target}: exit {run.status}", *(f"  audit: {line}" for line in run.audit or ("clean",))]
    return [
        *risk_lines(report.request, report.change, report.risk),
        *deny_lines(outcome.cargo_deny),
        *pip_audit_lines(outcome.pip_audit),
        f"confirmed: {combined(list(outcome.answers)).value}",
        *run_lines,
        f"image {report.image}, runtime {report.runtime}",
        *hint_lines(report.image_hint),
    ]


def packages_json(diff: PackageDiff) -> dict:
    def rows(packages):
        return [{"name": n, "old": old, "new": new, "source": source} for n, old, new, source in packages]
    return {"added": rows(diff.added), "removed": rows(diff.removed), "changed": rows(diff.changed),
            "errors": list(diff.errors)}


def report_json(report: Report) -> dict:
    outcome, run = report.outcome, report.run
    return {
        "change": asdict(report.change),
        "touched": [{"path": t.path, "status": t.status, "kinds": [k.name for k in t.kinds]}
                    for t in report.risk.touched],
        "cargo": packages_json(report.risk.cargo),
        "python": packages_json(report.risk.python),
        "new_sources": list(report.risk.new_sources.urls),
        "unreadable_cargo": list(report.risk.new_sources.unreadable),
        "cargo_deny": None if outcome.cargo_deny is None else asdict(outcome.cargo_deny),
        "pip_audit": [asdict(audit) for audit in outcome.pip_audit],
        "flagged": report.risk.flagged,
        "confirmed": combined(list(outcome.answers)).value,
        "run": None if run is None else {"target": run.target, "status": run.status, "audit": list(run.audit)},
        "image": report.image,
        "runtime": report.runtime,
        "image_hint": report.image_hint,
    }


def write_reports(review: Path, report: Report) -> None:
    (review / "report.txt").write_text("\n".join(render(report)) + "\n")
    (review / "report.json").write_text(json.dumps(report_json(report), indent=1) + "\n")
    print(f"sandbox-pr: reports in {review / 'report.txt'} and {review / 'report.json'}", file=sys.stderr)
