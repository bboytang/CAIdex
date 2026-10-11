"""Synthetic environment boundary peer; never reads credential files."""
import os
import sys

mode = sys.argv[1]
allowed = {"PATH", "HOME", "SYSTEMROOT", "USERPROFILE", "TEMP", "TMP"}
# Python can coerce the Unix C locale before this script executes.
allowed.add("LC_CTYPE")
if mode == "runtime":
    allowed.add("CODEX_HOME")
    assert os.environ.get("CODEX_HOME") == "synthetic-owned-runtime-home"
else:
    assert sys.argv[2:] == ["--version"]
unexpected = sorted(name for name in os.environ if name.upper() not in allowed)
assert not unexpected, "unexpected environment names: " + ", ".join(unexpected)
assert "PATH" in os.environ
if os.name == "nt":
    assert "SYSTEMROOT" in {name.upper() for name in os.environ}
if mode != "runtime":
    print("codex-cli 0.160.2" if mode == "wrong-version" else "codex-cli 0.160.1")
    if mode == "nonzero-exit":
        sys.exit(17)
