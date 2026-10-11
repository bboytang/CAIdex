#!/usr/bin/env python3
"""Run I-6 real PostgreSQL/HTTP, Memory state and member RLS failure paths."""
from pathlib import Path
import runpy
import sys

if __name__ == '__main__':
    sys.argv.insert(1, '--memory-sync')
    runpy.run_path(str(Path(__file__).with_name('i2-auth.py')), run_name='__main__')
