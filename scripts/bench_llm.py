"""Isolate LLM speed: schema-constrained vs free generation, same server.

    python scripts/bench_llm.py <portable folder>
"""

from __future__ import annotations

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

import httpx  # noqa: E402

from lariksa import catalog  # noqa: E402
from lariksa.llm import LocalLLM, server_binary  # noqa: E402
from lariksa.notulen import SCHEMA  # noqa: E402
from lariksa.paths import Paths  # noqa: E402


def main() -> None:
    paths = Paths(Path(sys.argv[1]).resolve())
    server = server_binary(catalog.LLAMA_RUNTIME.path(paths))
    model = catalog.SUMMARY_MODEL.path(paths) / catalog.SUMMARY_MODEL.marker
    prompt = "Tulis notulen rapat fiktif tentang rilis aplikasi, lengkap dengan 5 action item. " * 3
    with LocalLLM(server, model) as llm:
        print("device:", llm.device)
        for label, fmt in (("free", None), ("schema", SCHEMA)):
            body = {"messages": [{"role": "user", "content": prompt}], "max_tokens": 300, "temperature": 0.2}
            if fmt:
                body["response_format"] = {"type": "json_schema", "json_schema": {"name": "n", "schema": fmt}}
            started = time.monotonic()
            r = httpx.post(f"http://127.0.0.1:{llm.port}/v1/chat/completions", json=body, timeout=600,
                           headers={"Authorization": f"Bearer {llm._key}"}).json()
            t = r.get("timings", {})
            print(f"{label:7} wall={time.monotonic() - started:5.1f}s "
                  f"pp={t.get('prompt_per_second', 0):7.1f} t/s  tg={t.get('predicted_per_second', 0):6.1f} t/s "
                  f"tokens={t.get('predicted_n')}")


if __name__ == "__main__":
    main()
