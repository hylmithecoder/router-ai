#!/usr/bin/env python3
"""Persistent local ALPR inference server.

The router used to spawn `main.py infer` for every request, which re-imported
OpenCV and re-loaded both ONNX models each time -- hundreds of milliseconds of
pure startup before any inference began. Here the models are loaded once at
startup and reused, so a request pays only for inference.

Run it:
    nix-shell python-alpr-local/shell.nix \
        --run "python-alpr-local/.venv/bin/python python-alpr-local/server.py"

Configuration (env):
    ALPR_HOST, ALPR_PORT       bind address (default 127.0.0.1:8791)
    ALPR_DETECTOR_MODEL        detector checkpoint, see engine.py
    ALPR_OCR_MODEL             OCR checkpoint
    ALPR_DETECTOR_CONF         detection confidence threshold
"""

from __future__ import annotations

import os
import sys
from contextlib import asynccontextmanager
from pathlib import Path
from typing import Any, Dict, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent))

import uvicorn
from fastapi import FastAPI
from pydantic import BaseModel

from engine import DETECTOR_MODEL, OCR_MODEL, LocalALPREngine

HOST = os.getenv("ALPR_HOST", "127.0.0.1")
PORT = int(os.getenv("ALPR_PORT", "8791"))

# One engine shared by every request. The ONNX sessions are thread-safe for
# concurrent Run() calls, and holding a single instance is the entire point of
# running a server instead of a subprocess.
_engine: Optional[LocalALPREngine] = None


class InferRequest(BaseModel):
    image: str
    """Data URI, raw base64, local path, or http(s) URL -- same as the CLI accepts."""


@asynccontextmanager
async def lifespan(app: FastAPI):
    global _engine
    _engine = LocalALPREngine()
    if _engine.load_error:
        # Surfaced rather than raised: /health reports it and the router falls
        # back to the cloud path, instead of the server refusing to start.
        print(f"WARNING: {_engine.load_error}", file=sys.stderr)
    else:
        print(f"Local ALPR ready on {HOST}:{PORT} ({DETECTOR_MODEL} + {OCR_MODEL})")
    yield


app = FastAPI(title="Local ALPR", version="2.0.0", lifespan=lifespan)


@app.get("/health")
def health() -> Dict[str, Any]:
    """Readiness probe. `ready` is false when the ONNX models failed to load."""
    return {
        "ready": _engine is not None and _engine.load_error is None,
        "detector_model": DETECTOR_MODEL,
        "ocr_model": OCR_MODEL,
        "error": _engine.load_error if _engine else "engine not initialized",
    }


@app.post("/infer")
def infer(req: InferRequest) -> Dict[str, Any]:
    """Recognize a plate. Response matches `main.py infer` stdout exactly."""
    if _engine is None:
        return LocalALPREngine._empty_result("engine not initialized")
    return _engine.recognize(req.image)


if __name__ == "__main__":
    uvicorn.run(app, host=HOST, port=PORT, log_level="warning")
