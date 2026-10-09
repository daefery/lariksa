"""Where Lariksa keeps its files.

Portable means everything lives next to the executable: recordings, the
database, downloaded models, and the llama.cpp runtime. Copy the folder to
another laptop and your meetings come with it. In development the root is
``.lariksa/`` in the repository, or ``LARIKSA_HOME`` when set.
"""

from __future__ import annotations

import os
import sys
from dataclasses import dataclass
from pathlib import Path


def _root() -> Path:
    override = os.environ.get("LARIKSA_HOME")
    if override:
        return Path(override).resolve()
    if getattr(sys, "frozen", False):
        return Path(sys.executable).resolve().parent
    return Path(__file__).resolve().parents[2] / ".lariksa"


@dataclass(frozen=True)
class Paths:
    root: Path

    @property
    def data(self) -> Path:
        return self.root / "data"

    @property
    def database(self) -> Path:
        return self.data / "lariksa.db"

    @property
    def recordings(self) -> Path:
        return self.data / "recordings"

    @property
    def models(self) -> Path:
        return self.root / "models"

    @property
    def runtime(self) -> Path:
        """Downloaded native runtimes: llama.cpp and the optional CUDA libraries."""
        return self.root / "runtime"

    def meeting_dir(self, meeting_id: str) -> Path:
        return self.recordings / meeting_id

    def ensure(self) -> Paths:
        for path in (self.data, self.recordings, self.models, self.runtime):
            path.mkdir(parents=True, exist_ok=True)
        return self


def app_paths() -> Paths:
    return Paths(_root()).ensure()


def resource(relative: str) -> Path:
    """A file bundled with the app (UI assets), frozen or not."""
    base = Path(getattr(sys, "_MEIPASS", Path(__file__).resolve().parent.parent))
    return base / relative
