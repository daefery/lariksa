"""End-to-end smoke test of the real engines, no UI.

    python scripts/smoke.py <portable folder> <audio file> [--assets llama,cuda]

Downloads the named assets through the app's own downloader, imports the
audio, waits for transcript + notulen, and prints timings and the WhatsApp text.
"""

from __future__ import annotations

import argparse
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from lariksa.paths import Paths  # noqa: E402
from lariksa.service import LariksaService  # noqa: E402


def main() -> None:
    sys.stdout.reconfigure(encoding="utf-8")  # Windows consoles default to cp1252
    p = argparse.ArgumentParser()
    p.add_argument("root", type=Path)
    p.add_argument("audio", type=Path)
    p.add_argument("--assets", default="")
    a = p.parse_args()

    svc = LariksaService(Paths(a.root.resolve()).ensure())
    keys = [k for k in a.assets.split(",") if k]
    if keys:
        print("download", svc.download(keys))
        while svc.activity.downloads or svc._download_lock.locked():
            for k, d in list(svc.activity.downloads.items()):
                if d["error"]:
                    sys.exit(f"download {k} failed: {d['error']}")
            time.sleep(1)
    s = svc.status()
    print("compute", s["compute"], "ready", s["ready"])
    print("installed", {k: v["installed"] for k, v in s["assets"].items()})

    started = time.monotonic()
    mid = svc.import_audio(str(a.audio), "Smoke test")["meeting_id"]
    last = ""
    while True:
        m = svc.store.get(mid)
        stage = f"{m.status} {svc.activity.stage} {svc.activity.progress:.0%}"
        if stage != last:
            print(f"[{time.monotonic() - started:6.1f}s] {stage}")
            last = stage
        if m.status in ("done", "failed"):
            break
        time.sleep(0.5)
    print("error:", m.error)
    print("timings:", m.timings, "utterances:", len(m.transcript))
    detail = svc.meeting(mid)
    print(detail.get("whatsapp", ""))


if __name__ == "__main__":
    main()
