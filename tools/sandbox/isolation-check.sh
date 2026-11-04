#!/usr/bin/env bash
# Proves from inside the sandbox what isolates it: one line per protection, "ok" when it holds,
# "FAIL" when it does not, "skip" with the reason when it does not apply; it exits 1 when a line
# failed.
# Uses bash, coreutils and python3 only. Run it through `make sandbox-check`, which passes the
# host's home path as the argument, so the check can prove that path is not mounted.
set -uo pipefail

host_home=${1:?usage: isolation-check.sh HOST_HOME (make sandbox-check passes it)}
failures=0

# check WHAT COMMAND...: prints "ok   WHAT" when COMMAND succeeds, "FAIL WHAT" otherwise.
check() {
  local what=$1
  shift
  if "$@" > /dev/null 2>&1; then
    echo "ok   $what"
  else
    echo "FAIL $what"
    failures=$((failures + 1))
  fi
}

# fails COMMAND...: succeeds when COMMAND fails.
fails() { ! "$@"; }

# status_is FIELD VALUE: the field of /proc/self/status has exactly that value.
status_is() { grep -qE "^$1:[[:space:]]+$2\$" /proc/self/status; }

# read_only PATH: the path exists and cannot be written.
read_only() { [ -e "$1" ] && ! touch "$1"; }

# absent_named PATH: PATH is not empty (a .git file named a directory) and does not exist.
absent_named() { [ "$1" != /work/ ] && [ ! -e "$1" ]; }

# absent_or_empty DIR: the directory is missing (the host has none) or masked (empty).
absent_or_empty() { [ ! -e "$1" ] || [ -z "$(ls -A "$1")" ]; }

# no_secret_variables: no variable name holds TOKEN, SECRET, PASSWORD or PYPI.
# (grep reads all of compgen's output: with -q it could stop early and SIGPIPE would pass the check.)
no_secret_variables() { ! compgen -e | grep -iE 'TOKEN|SECRET|PASSWORD|PYPI' > /dev/null; }

root_ro='import sys; sys.exit(not any(f[4] == "/" and "ro" in f[5].split(",") for f in map(str.split, open("/proc/self/mountinfo"))))'
tcp='import socket; socket.create_connection(("1.1.1.1", 443), timeout=5)'
dns='import socket; socket.getaddrinfo("github.com", 443)'

check "no network: TCP to 1.1.1.1:443 fails" fails python3 -c "$tcp"
check "no network: a DNS lookup of github.com fails" fails python3 -c "$dns"
check "read-only root: / is mounted ro" python3 -c "$root_ro"
check "read-only root: touch /usr/probe fails" fails touch /usr/probe
check "writable /tmp: touch /tmp/probe works" touch /tmp/probe
rm -f /tmp/probe
check "not root: uid $(id -u)" test "$(id -u)" -ne 0
check "no capabilities: CapBnd is 0000000000000000" status_is CapBnd 0000000000000000
check "no new privileges: NoNewPrivs is 1" status_is NoNewPrivs 1
check "seccomp filter: Seccomp is 2" status_is Seccomp 2
check "read-only host-executed set: touch /work/Makefile fails" read_only /work/Makefile
check "read-only host-executed set: touch /work/.cargo/config.toml fails" read_only /work/.cargo/config.toml
check "read-only git: touch /work/.git fails" read_only /work/.git
if [ -d /work/.git ]; then
  check "read-only git: a file under /work/.git cannot be made" fails touch /work/.git/probe
else
  # In a worktree .git is a file naming the git directory, outside the checkout: it must not be mounted.
  gitdir=$(sed -n 's/^gitdir: //p' /work/.git)
  case $gitdir in /*) ;; *) gitdir=/work/$gitdir ;; esac
  check "read-only git: the git directory /work/.git names ($gitdir) is not mounted" absent_named "$gitdir"
fi
check "masked notes: /work/.doc is absent or empty" absent_or_empty /work/.doc
check "masked notes: /work/.workflow-kit is absent or empty" absent_or_empty /work/.workflow-kit
check "no host home: $host_home does not exist" test ! -e "$host_home"
check "no docker socket: /var/run/docker.sock does not exist" test ! -e /var/run/docker.sock
check "no variable named like TOKEN, SECRET, PASSWORD or PYPI" no_secret_variables

if [ "$failures" -ne 0 ]; then
  echo "isolation-check: failed checks: $failures"
  exit 1
fi
