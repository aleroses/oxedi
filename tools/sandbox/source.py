"""The change sandbox-pr tests: a pull request fetched from GitHub, or a ref of the host checkout,
in a throwaway clone made with git isolated from the host's configuration.

Every git call goes through `isolated_git`: no global or system configuration (so no filter,
hook path, credential helper or `insteadOf` the host defines), no hook, no terminal prompt, no
LFS smudge and no file transport. Nothing of the clone is executed on the host.
"""

import os
import re
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Literal

from layout import SandboxError, one_line

# Set over the inherited environment; every other GIT_ variable of the shell is dropped.
GIT_ENV = {
    "GIT_CONFIG_GLOBAL": os.devnull,
    "GIT_CONFIG_NOSYSTEM": "1",
    "GIT_TERMINAL_PROMPT": "0",
    "GIT_LFS_SKIP_SMUDGE": "1",
    # git's messages in English whatever the host's locale: a missing remote ref is told by its text.
    "LC_ALL": "C",
    "LANGUAGE": "",
}
# No hook, no attributes file of the host's (git reads $XDG_CONFIG_HOME/git/attributes even with
# no global configuration) and no file transport.
GIT_OPTIONS = (
    "-c", "core.hooksPath=/dev/null", "-c", "core.attributesFile=/dev/null", "-c", "protocol.file.allow=never",
)
# The one clone of the host checkout reads it over git's local transport, which
# `protocol.file.allow=never` refuses; that call alone allows it.
LOCAL_SOURCE = ("-c", "protocol.file.allow=always")
# For tests only: options added after the isolation ones, so a test can serve a pull request
# from a local bare repository.
TEST_OPTIONS: tuple[str, ...] = ()
# Long enough for a fetch of the whole repository over a slow line.
GIT_TIMEOUT_S = 900
# check-ref-format needs no repository.
NO_REPO = Path("/")

PR_NUMBER = re.compile(r"[1-9][0-9]{0,8}")
GITHUB = re.compile(
    r"(?:git@github\.com:|ssh://git@github\.com/|https://github\.com/)"
    r"(?P<owner>[A-Za-z0-9-]+)/(?P<repo>[A-Za-z0-9._-]+?)(?:\.git)?"
)
# The user and password part of a URL (`https://user:token@host/...`), never printed. It runs to
# the last `@` before the path, so a password that holds an `@` is redacted whole.
USERINFO = re.compile(r"(?i)\b([a-z][a-z0-9+.-]*://)[^/?#\s]*@")
LABEL_SAFE = frozenset(b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789._-")
NO_REMOTE_REF = "couldn't find remote ref"


@dataclass(frozen=True)
class Change:
    """The commit a run tests and the two ends of the diff the report shows (full SHAs)."""

    kind: Literal["pr", "ref"]
    # `pr<N>` or `ref-<name>`, every byte outside [A-Za-z0-9._-] as `-`: a directory name.
    label: str
    tested: str
    base: str
    head: str
    # The tested commit is the pull request's merge ref.
    merged: bool


@dataclass(frozen=True)
class Request:
    """What to test, as SANDBOX_PR, SANDBOX_REF and SANDBOX_HEAD name it: `name` is the pull
    request number or the ref; `head` tests `refs/pull/N/head` instead of the merge ref."""

    kind: Literal["pr", "ref"]
    name: str
    head: bool

    @staticmethod
    def parse(env: dict[str, str]) -> "Request":
        """The request the variables name; an empty variable counts as unset. Anything else than
        exactly one of PR and REF, a PR that is not a number, a REF git would not accept as a ref
        name or one that reads as an option, and HEAD other than 1 or without PR are refused."""
        pr, ref, head = (env.get(f"SANDBOX_{name}", "") for name in ("PR", "REF", "HEAD"))
        if pr and ref:
            raise SandboxError(f'set PR or REF, not both: got PR="{pr}" and REF="{ref}"')
        if head and head != "1":
            raise SandboxError(f'HEAD must be 1 or unset, got "{head}"')
        if pr:
            return Request.pull_request(pr, head == "1")
        if not ref:
            raise SandboxError("set PR=<number> or REF=<name>, got neither")
        if head:
            raise SandboxError(f'HEAD=1 goes with PR, not REF: got HEAD="{head}" and REF="{ref}"')
        return Request.ref(ref)

    @staticmethod
    def pull_request(number: str, head: bool) -> "Request":
        if not PR_NUMBER.fullmatch(number):
            raise SandboxError(f'PR must be a pull request number, 1 to 999999999, got "{number}"')
        return Request("pr", number, head)

    @staticmethod
    def ref(name: str) -> "Request":
        # Checked before any git command sees it.
        if name.startswith("-"):
            raise SandboxError(f'REF must not start with "-", got "{name}"')
        if isolated_git(NO_REPO, "check-ref-format", "--allow-onelevel", name).returncode != 0:
            raise SandboxError(f'REF is not a valid git ref name, got "{name}"')
        return Request("ref", name, False)

    @property
    def label(self) -> str:
        if self.kind == "pr":
            return f"pr{self.name}"
        return "ref-" + "".join(chr(b) if b in LABEL_SAFE else "-" for b in self.name.encode())


def git_environment() -> dict[str, str]:
    inherited = {k: v for k, v in os.environ.items() if not k.startswith("GIT_") and k != "SSH_ASKPASS"}
    return inherited | GIT_ENV


def isolated_git(root: Path, *args: str, local_source: bool = False) -> subprocess.CompletedProcess[bytes]:
    """`git -C root <args>` with the host's git configuration, hooks and file transport out of
    reach; `local_source` allows the local transport for the clone of the host checkout. A git
    that runs past the timeout is refused."""
    options = GIT_OPTIONS + (LOCAL_SOURCE if local_source else ()) + TEST_OPTIONS
    command = ["git", "-C", str(root), *options, *args]
    try:
        return subprocess.run(command, env=git_environment(), capture_output=True, check=False,
                              timeout=GIT_TIMEOUT_S)
    except subprocess.TimeoutExpired as error:
        raise SandboxError(f"git {' '.join(args)} ran past {GIT_TIMEOUT_S} s in {root}") from error


def failure(result: subprocess.CompletedProcess[bytes]) -> str:
    return one_line(os.fsdecode(result.stderr).splitlines())


def git_bytes(root: Path, *args: str, local_source: bool = False) -> bytes:
    """The output of an isolated git that must succeed; a failure is refused with git's own
    reason."""
    result = isolated_git(root, *args, local_source=local_source)
    if result.returncode != 0:
        raise SandboxError(f"git {' '.join(args)} failed in {root}: {failure(result)}")
    return result.stdout


def git_text(root: Path, *args: str, local_source: bool = False) -> str:
    return os.fsdecode(git_bytes(root, *args, local_source=local_source)).strip()


def redacted(text: str) -> str:
    """`text` with the user and password of every URL in it replaced, so a token never reaches a
    log line."""
    return USERINFO.sub(r"\1<redacted>@", text)


def github_https(origin_url: str) -> str:
    """The anonymous HTTPS URL of the GitHub repository an `origin` URL names, in its SSH or HTTPS
    form; any other host or form is refused."""
    match = GITHUB.fullmatch(origin_url)
    if match is None or match["repo"] in (".", ".."):
        raise SandboxError(
            f'origin is "{redacted(origin_url)}"; sandbox-pr fetches pull requests only from a GitHub origin '
            "(git@github.com:<owner>/<repo>, ssh://git@github.com/<owner>/<repo> or https://github.com/<owner>/<repo>)"
        )
    return f"https://github.com/{match['owner']}/{match['repo']}.git"


def commit_of(root: Path, name: str) -> str:
    """The full SHA of the commit `name` names in `root`; a name that names none is refused."""
    result = isolated_git(root, "rev-parse", "--verify", "--quiet", "--end-of-options", f"{name}^{{commit}}")
    if result.returncode != 0:
        raise SandboxError(f'"{name}" names no commit in {root}')
    return os.fsdecode(result.stdout).strip()


def default_base(host: Path, repo: Path, head: str) -> str:
    """Where a change without a merge ref branched off: its merge base, found in `repo` (which
    holds the host's objects), with the host's origin/master, else with its master."""
    for branch in ("refs/remotes/origin/master", "refs/heads/master"):
        found = isolated_git(host, "rev-parse", "--verify", "--quiet", "--end-of-options", f"{branch}^{{commit}}")
        if found.returncode == 0:
            return git_text(repo, "merge-base", "--end-of-options", os.fsdecode(found.stdout).strip(), head)
    raise SandboxError(f"{host} has neither origin/master nor master to compare the change with")


def fetch(into: Path, url: str, ref: str) -> bool:
    """Fetches `ref` of `url` into `refs/sandbox/<last part>`; False when the remote has no such
    ref, a refusal for any other failure."""
    local = f"refs/sandbox/{ref.rsplit('/', 1)[1]}"
    result = isolated_git(into, "fetch", "--no-tags", "--no-recurse-submodules", "--end-of-options", url,
                          f"+{ref}:{local}")
    if result.returncode == 0:
        return True
    if NO_REMOTE_REF in os.fsdecode(result.stderr):
        return False
    raise SandboxError(f"cannot fetch {ref} from {url}: {failure(result)}")


def pull_request(host: Path, request: Request, into: Path) -> Change:
    url = github_https(git_text(host, "remote", "get-url", "origin"))
    number = request.name
    if not fetch(into, url, f"refs/pull/{number}/head"):
        raise SandboxError(f"pull request {number} not found at {url}")
    head = commit_of(into, "refs/sandbox/head")
    if request.head:
        return Change("pr", request.label, head, default_base(host, into, head), head, False)
    if not fetch(into, url, f"refs/pull/{number}/merge"):
        raise SandboxError(
            f"pull request {number} has no merge ref refs/pull/{number}/merge: it has conflicts or is closed; "
            f"set HEAD=1 to test refs/pull/{number}/head instead"
        )
    merge = commit_of(into, "refs/sandbox/merge")
    # GitHub recomputes the merge ref after a push, so it can still merge an older head; the report
    # judges the head the tested merge holds, and a stale merge ref is refused.
    merged_head = commit_of(into, f"{merge}^2")
    if merged_head != head:
        raise SandboxError(
            f"refs/pull/{number}/merge merges {merged_head}, but refs/pull/{number}/head is {head}: "
            "GitHub has not yet recomputed the merge after a push; run it again in a minute"
        )
    return Change("pr", request.label, merge, commit_of(into, f"{merge}^1"), merged_head, True)


def local_ref(host: Path, request: Request) -> Change:
    head = commit_of(host, request.name)
    return Change("ref", request.label, head, default_base(host, host, head), head, False)


def clone(host_checkout: Path, request: Request, into: Path) -> Change:
    """Clones the host checkout into `into` without checking anything out, fetches the pull
    request's refs anonymously from GitHub when the request names one, and checks out the
    tested commit detached."""
    git_text(host_checkout, "clone", "--quiet", "--no-checkout", "--no-hardlinks", "--no-recurse-submodules",
             "--end-of-options", str(host_checkout), str(into), local_source=True)
    change = pull_request(host_checkout, request, into) if request.kind == "pr" else local_ref(host_checkout, request)
    git_text(into, "checkout", "--quiet", "--detach", change.tested)
    return change
