"""Run the real notulen step on a bench transcript and time it.

    uv run python scripts/bench_notulen.py bench.json --server <llama-server> --model <gguf>
"""

from __future__ import annotations

import argparse
import json
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from lariksa.llm import LocalLLM  # noqa: E402
from lariksa.notulen import to_markdown, to_whatsapp, write_notulen  # noqa: E402
from lariksa.transcript import OTHERS, Utterance, as_text  # noqa: E402


def main() -> None:
    p = argparse.ArgumentParser()
    p.add_argument("bench", type=Path)
    p.add_argument("--server", type=Path, required=True)
    p.add_argument("--model", type=Path, required=True)
    a = p.parse_args()
    text = json.loads(a.bench.read_text(encoding="utf-8"))[0]["text"]
    transcript = as_text([Utterance(OTHERS, 0, 600, text)])
    started = time.monotonic()
    with LocalLLM(a.server, a.model) as llm:
        ready = time.monotonic()
        n = write_notulen(transcript, llm.complete_json)
    done = time.monotonic()
    print(f"chars={len(transcript)} startup_s={ready - started:.1f} summarize_s={done - ready:.1f}")
    print(to_markdown(n))
    print("---- WhatsApp ----")
    print(to_whatsapp(n))


if __name__ == "__main__":
    main()
