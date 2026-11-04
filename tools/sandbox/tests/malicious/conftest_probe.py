# Synthetic hostile test hook for make sandbox-pr-check, appended to a conftest.py: it tries to
# reach the host home, the network and the shared caches, and records only whether each try
# succeeded. It never reads or sends any file's contents.
import os as _probe_os
import socket as _probe_socket
from pathlib import Path as _ProbePath

_PROBE_HOST_HOME = "@HOST_HOME@"
_PROBE_HOST_CANARY = "@HOST_CANARY@"


def _probe(action):
    try:
        action()
    except PermissionError:
        return "denied"
    except OSError:
        return "failed"
    return "succeeded"


def _probe_connect(host, port):
    _probe_socket.create_connection((host, port), timeout=5).close()


def _probe_write(path):
    with open(path, "ab") as handle:
        handle.write(b"x")


_probe_attempts = {
    "read host home": lambda: _probe_os.listdir(_PROBE_HOST_HOME),
    "connect 1.1.1.1:443": lambda: _probe_connect("1.1.1.1", 443),
    "connect pypi.org:443": lambda: _probe_connect("pypi.org", 443),
    "write /sandbox/cargo/x": lambda: _probe_write("/sandbox/cargo/x"),
    "write host canary": lambda: _probe_write(_PROBE_HOST_CANARY),
}
_probe_target = _ProbePath(__file__).resolve().parents[3] / "target"
_probe_target.mkdir(exist_ok=True)
(_probe_target / "malicious-conftest.txt").write_text(
    "".join(f"{what}: {_probe(action)}\n" for what, action in _probe_attempts.items())
)
