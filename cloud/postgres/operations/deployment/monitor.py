#!/usr/bin/env python3
"""Local operator alarms; no external mail and no user data in output."""
import json
from pathlib import Path
import shutil
import time

directory = Path('/var/lib/caidex-backups')
if shutil.disk_usage(directory).free < 1024**3:
    raise SystemExit('CAIdex disk reserve below 1GiB; operator action required')
records = [json.loads(path.read_text()) for path in directory.glob('snapshot-*.json')]
if not records or time.time() - max(record['completed_at'] for record in records) > 36 * 3600:
    raise SystemExit('CAIdex successful backup missing or older than 36 hours')
if sum(path.stat().st_size for path in directory.iterdir()) > 5_000_000_000:
    raise SystemExit('CAIdex backup directory exceeds 5GB')
if len(records) > 7 or any(time.time() - record['captured_at'] > 7 * 86400 for record in records):
    raise SystemExit('CAIdex backup pruning overdue; do not claim expired data removed')
print('CAIdex disk and backup monitor passed')
