"""Local LLM through a short-lived llama-server process.

The server starts only for the notulen step and stops right after, so the
4B model and the Whisper model are never resident together. llama.cpp's
``--fit`` (on by default) sizes context and GPU offload to the memory that is
actually free, and the JSON-schema grammar guarantees parseable output.
"""

from __future__ import annotations

import atexit
import json
import logging
import re
import secrets
import socket
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path

import httpx

log = logging.getLogger(__name__)

#: Enough for ~36k characters of transcript plus instructions and the answer.
CONTEXT_TOKENS = 16384


class LLMError(RuntimeError):
    """The local model could not produce notulen."""


#: Live server processes, so closing the app never orphans a 3 GB llama-server.
_LIVE: set[subprocess.Popen] = set()


def _terminate(proc: subprocess.Popen) -> None:
    if proc.poll() is None:
        proc.terminate()
        try:
            proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            proc.kill()
    _LIVE.discard(proc)


@atexit.register
def stop_all_servers() -> None:
    for proc in list(_LIVE):
        _terminate(proc)


# ---------------------------------------------------------------------------
# Device choice. llama.cpp's Vulkan build lists every GPU, and on a laptop with
# an Intel iGPU plus an NVIDIA dGPU it defaults to the iGPU: measured on an
# RTX 3050 Ti laptop, 7 tokens/s on the iGPU vs 62 on the dGPU (pp 2048:
# 159 vs 1994 t/s). So the device is chosen explicitly.
# ---------------------------------------------------------------------------

_DEVICE_LINE = re.compile(r"^\s*(\w+\d+):\s*(.+?)\s*\((\d+) MiB,\s*(\d+) MiB free\)\s*$")
_INTEGRATED = re.compile(r"intel.*(uhd|iris|hd graphics)|radeon\(tm\) graphics|radeon graphics|vega \d+ graphics", re.I)
_SOFTWARE = re.compile(r"llvmpipe|swiftshader|software|microsoft basic", re.I)
#: Model weights (~2.4 GB) + 16k context KV cache + compute buffers.
MODEL_NEED_MIB = 3000


@dataclass(frozen=True)
class Device:
    name: str
    description: str
    total_mib: int
    free_mib: int

    @property
    def integrated(self) -> bool:
        return bool(_INTEGRATED.search(self.description))


def parse_devices(listing: str) -> list[Device]:
    """Parse ``llama-server --list-devices`` output."""
    devices = []
    for line in listing.splitlines():
        m = _DEVICE_LINE.match(line)
        if m and not _SOFTWARE.search(m.group(2)):
            devices.append(Device(m.group(1), m.group(2), int(m.group(3)), int(m.group(4))))
    return devices


def choose_device(devices: list[Device], need_mib: int = MODEL_NEED_MIB) -> str:
    """Best device name, or "" for CPU.

    A discrete GPU wins even when partly full: ``--fit`` keeps what does not
    fit in system RAM, and a half-offloaded dGPU still beats a full iGPU.
    An iGPU is used only when it is the only GPU and has room for the model;
    it shares system RAM, so its "free" figure is real headroom.
    """
    discrete = sorted((d for d in devices if not d.integrated), key=lambda d: -d.free_mib)
    if discrete and discrete[0].free_mib >= need_mib // 2:
        return discrete[0].name
    integrated = [d for d in devices if d.integrated and d.free_mib >= need_mib]
    return integrated[0].name if integrated else ""


def list_devices(server: Path) -> list[Device]:
    try:
        out = subprocess.run([str(server), "--list-devices"], capture_output=True, text=True,
                             timeout=60, cwd=str(server.parent),
                             creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
    except (OSError, subprocess.TimeoutExpired) as exc:
        log.warning("Could not list llama.cpp devices: %s", exc)
        return []
    return parse_devices(out.stdout + out.stderr)


def _free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


class LocalLLM:
    def __init__(self, server: Path, model: Path, *, startup_timeout: float = 180.0):
        # Absolute: the process runs with cwd set to the server's folder.
        self.server = server.resolve()
        self.model = model.resolve()
        self.startup_timeout = startup_timeout
        self._proc: subprocess.Popen | None = None
        self._log: Path | None = None
        self.port = 0
        # The server listens on localhost while a meeting transcript is loaded;
        # a per-run key keeps other local programs and web pages (it allows any
        # CORS origin) from querying it.
        self._key = secrets.token_urlsafe(24)

    def __enter__(self) -> LocalLLM:
        if not self.server.exists():
            raise LLMError("Mesin AI lokal belum terpasang.")
        if not self.model.exists():
            raise LLMError("Model notulen belum diunduh.")
        self.port = _free_port()
        self._log = self.model.parent / "llama-server.log"
        devices = list_devices(self.server)
        self.device = choose_device(devices)
        log.info("llama.cpp devices: %s -> %s", devices, self.device or "CPU")
        args = [
            str(self.server),
            "--model", str(self.model),
            "--host", "127.0.0.1",
            "--port", str(self.port),
            "--api-key", self._key,
            "--ctx-size", str(CONTEXT_TOKENS),
            "--parallel", "1",
            "--no-webui",
            # Instruct model: no hidden reasoning tokens to wait for.
            "--reasoning-budget", "0",
            "--device", self.device or "none",
            # q8 KV cache halves its memory with no visible quality loss, and a
            # smaller --fit margin lets a 4 GB laptop GPU hold 29/37 layers
            # instead of 16/37 (measured on an RTX 3050 Ti).
            "--cache-type-k", "q8_0",
            "--cache-type-v", "q8_0",
            "--flash-attn", "on",
            "--fit-target", "384",
        ]
        log.info("Starting llama-server on port %d, device %s", self.port, self.device or "CPU")
        self._proc = subprocess.Popen(
            args,
            stdout=self._log.open("w", encoding="utf-8"),
            stderr=subprocess.STDOUT,
            cwd=str(self.server.parent),
            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
        )
        _LIVE.add(self._proc)
        try:
            self._wait_ready()
        except BaseException:
            _terminate(self._proc)
            raise
        return self

    def _wait_ready(self) -> None:
        deadline = time.monotonic() + self.startup_timeout
        url = f"http://127.0.0.1:{self.port}/health"
        while time.monotonic() < deadline:
            if self._proc and self._proc.poll() is not None:
                raise LLMError(f"Mesin AI berhenti saat mulai. Lihat {self._log}:\n{self._tail()}")
            try:
                if httpx.get(url, timeout=2).status_code == 200:
                    return
            except httpx.HTTPError:
                pass
            time.sleep(0.5)
        raise LLMError("Mesin AI tidak siap dalam waktu yang wajar.")

    def _tail(self, lines: int = 15) -> str:
        try:
            return "\n".join(self._log.read_text(encoding="utf-8", errors="replace").splitlines()[-lines:])
        except (OSError, AttributeError):
            return ""

    def __exit__(self, *exc) -> None:
        if self._proc:
            _terminate(self._proc)
        self._proc = None

    def complete_json(self, system: str, user: str, schema: dict) -> dict:
        payload = {
            "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}],
            "temperature": 0.2,
            "top_p": 0.9,
            "max_tokens": 2048,
            "response_format": {"type": "json_schema", "json_schema": {"name": "notulen", "schema": schema}},
        }
        try:
            response = httpx.post(
                f"http://127.0.0.1:{self.port}/v1/chat/completions",
                json=payload,
                headers={"Authorization": f"Bearer {self._key}"},
                timeout=httpx.Timeout(10.0, read=900.0),
            )
        except httpx.HTTPError as exc:
            raise LLMError(f"Mesin AI tidak merespons: {exc}") from exc
        if response.status_code != 200:
            raise LLMError(f"Mesin AI error {response.status_code}: {response.text[:300]}")
        content = response.json()["choices"][0]["message"]["content"]
        try:
            return json.loads(content)
        except json.JSONDecodeError as exc:
            raise LLMError("Model mengembalikan JSON yang tidak valid.") from exc


def server_binary(runtime_dir: Path) -> Path:
    name = "llama-server.exe" if sys.platform == "win32" else "llama-server"
    return runtime_dir / name
