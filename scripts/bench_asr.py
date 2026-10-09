"""Compare Whisper settings on one audio file: speed and output side by side.

    uv run python scripts/bench_asr.py sample.wav --model-dir <turbo> --model-dir <small> --auto-small
"""

from __future__ import annotations

import argparse
import json
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from lariksa.hardware import detect  # noqa: E402
from lariksa.transcribe import Transcriber, TranscribeOptions  # noqa: E402


def run(audio: Path, model_dir: Path, language: str, glossary: tuple[str, ...], cuda: bool) -> dict:
    compute = detect(cuda)
    opts = TranscribeOptions(model_dir=model_dir, compute=compute, language=language, glossary=glossary)
    started = time.monotonic()
    with Transcriber(opts) as t:
        loaded = time.monotonic()
        utterances = t.transcribe(audio, "Peserta")
    done = time.monotonic()
    return {
        "model": model_dir.name,
        "language": language or "auto",
        "device": f"{compute.device}/{compute.compute_type}",
        "load_s": round(loaded - started, 1),
        "transcribe_s": round(done - loaded, 1),
        "words": sum(len(u.text.split()) for u in utterances),
        "text": " ".join(u.text for u in utterances),
    }


def main() -> None:
    p = argparse.ArgumentParser()
    p.add_argument("audio", type=Path)
    p.add_argument("--model-dir", type=Path, action="append", required=True)
    p.add_argument("--language", default="id")
    p.add_argument("--glossary", default="")
    p.add_argument("--auto-small", action="store_true", help="also run the last model with auto language")
    p.add_argument("--cpu", action="store_true")
    p.add_argument("--out", type=Path)
    a = p.parse_args()
    glossary = tuple(t.strip() for t in a.glossary.split(",") if t.strip())
    results = [run(a.audio, m, a.language, glossary, not a.cpu) for m in a.model_dir]
    if a.auto_small:
        results.append(run(a.audio, a.model_dir[-1], "", (), not a.cpu))
    for r in results:
        print(json.dumps({k: v for k, v in r.items() if k != "text"}), "|", r["text"][:300])
    if a.out:
        a.out.write_text(json.dumps(results, ensure_ascii=False, indent=1), encoding="utf-8")


if __name__ == "__main__":
    main()
