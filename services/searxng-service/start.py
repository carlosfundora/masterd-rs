"""MASTERd local SearXNG launcher.

Runs the installed SearXNG Flask app on 127.0.0.1:9265 with a local settings
file. The installer prepares the venv and settings file; this wrapper keeps the
desktop sidecar command stable across source and packaged installs.
"""
from __future__ import annotations

import os
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parent
SRC = ROOT / "searxng-src"
SETTINGS = ROOT / "settings.yml"

if SRC.exists():
    sys.path.insert(0, str(SRC))

os.environ.setdefault("SEARXNG_SETTINGS_PATH", str(SETTINGS))
os.environ.setdefault("SEARXNG_URL", "http://127.0.0.1:9265/")
os.environ.setdefault("SEARXNG_DEBUG", "false")

from searx.webapp import app  # noqa: E402


if __name__ == "__main__":
    app.run(host="127.0.0.1", port=9265, debug=False, threaded=True)
