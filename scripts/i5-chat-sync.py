#!/usr/bin/env python3
"""Run I-5 real PostgreSQL/HTTP and two scoped Rust Chat clients, with deterministic failures."""
from pathlib import Path
import runpy
import sys

if __name__ == '__main__':
    sys.argv.insert(1, '--chat-sync')
    runpy.run_path(str(Path(__file__).with_name('i2-auth.py')), run_name='__main__')
