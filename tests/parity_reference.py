#!/usr/bin/env python3
"""Parity reference driver for the native Rust metrics engine (ADR-0002).

Invokes the REAL Python master reference — tina4-python's
`tina4_python/dev_admin/metrics.py` — on a single source file and prints its
loc / cyclomatic-complexity / function-count / maintainability-index as JSON.

The Rust parity test (`metrics::tests::parity_matches_python_master`) runs the
Rust engine on the SAME file and asserts the numbers match. No mocks: this shells
out to the actual metrics.py against the actual file.

Usage:  TINA4_PYTHON_DIR=/path/to/tina4-python  python3 parity_reference.py <file.py>
"""
import json
import os
import shutil
import sys
import tempfile

tina4_dir = os.environ.get("TINA4_PYTHON_DIR")
if not tina4_dir or not os.path.isdir(tina4_dir):
    print(json.dumps({"error": "TINA4_PYTHON_DIR not set or not a directory"}))
    sys.exit(3)

sys.path.insert(0, tina4_dir)
try:
    from tina4_python.dev_admin import metrics as m
except Exception as exc:  # metrics.py or tina4_python not importable
    print(json.dumps({"error": f"cannot import metrics.py: {exc}"}))
    sys.exit(3)

src_file = sys.argv[1]
# full_analysis() scans a directory; isolate the one file so file_metrics[0] is it.
tmp = tempfile.mkdtemp()
try:
    dst = os.path.join(tmp, os.path.basename(src_file))
    shutil.copy(src_file, dst)
    result = m.full_analysis(tmp)
    fm = result["file_metrics"][0]
    print(json.dumps({
        "loc": fm["loc"],
        "complexity": fm["complexity"],
        "functions": fm["functions"],
        "maintainability": fm["maintainability"],
        "avg_complexity": fm["avg_complexity"],
    }))
finally:
    shutil.rmtree(tmp, ignore_errors=True)
