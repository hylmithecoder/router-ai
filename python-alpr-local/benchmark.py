#!/usr/bin/env python3
"""Benchmark and evaluation suite for the local ALPR engine.

Reports exact-match rate, character error rate, and latency percentiles.

Two rules this harness exists to enforce:

1. **No substring matching.** The old "partial match" metric asked whether either
   string contained the other, so a 3-character overlap counted as a hit and the
   reported accuracy was meaningless. CER (Levenshtein distance over ground-truth
   length) measures how wrong a read actually is.
2. **Real and synthetic are reported separately, never pooled.** The synthetic
   plates are PIL-rendered in DejaVuSans and share no font, lighting, or camera
   geometry with real photos. Averaging them into one number hides real accuracy
   behind whichever set is larger.
"""

from __future__ import annotations

import json
import os
import sqlite3
import time
from pathlib import Path
from typing import Any, Dict, List, Optional

import plate_format
from dataset_collector import setup_datasets
from engine import LocalALPREngine

BASE_DIR = Path(__file__).resolve().parent
REPO_ROOT = BASE_DIR.parent
DATASETS_DIR = BASE_DIR / "datasets"
INDEX_FILE = DATASETS_DIR / "manifest.json"

# Images harvested from live cloud OCR calls, labelled in the router's SQLite DB.
HARVESTED_DIR = REPO_ROOT / "datasets" / "plates" / "images"
ROUTER_DB = REPO_ROOT / "router.db"


def levenshtein(a: str, b: str) -> int:
    """Edit distance between two strings (iterative, O(len(a)*len(b)) time)."""
    if a == b:
        return 0
    if not a:
        return len(b)
    if not b:
        return len(a)

    prev = list(range(len(b) + 1))
    for i, ca in enumerate(a, start=1):
        cur = [i]
        for j, cb in enumerate(b, start=1):
            cur.append(min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + (ca != cb)))
        prev = cur
    return prev[-1]


def _normalize(text: str) -> str:
    """Compare plates ignoring spacing and case, which are not accuracy signals."""
    return "".join(text.upper().split())


def load_harvested_samples() -> List[Dict[str, Any]]:
    """Build a real-photo eval set from harvested images + their DB ground truth.

    These are the only genuine field images available: photos that went through
    the cloud vision model, whose plate strings were stored in
    `ocr_license_plate_samples`. Samples with no plate string are skipped, since
    an empty ground truth cannot score anything.
    """
    if not ROUTER_DB.exists() or not HARVESTED_DIR.exists():
        return []

    samples: List[Dict[str, Any]] = []
    try:
        conn = sqlite3.connect(f"file:{ROUTER_DB}?mode=ro", uri=True)
        conn.row_factory = sqlite3.Row
        rows = conn.execute(
            "SELECT image_filename, plate_number FROM ocr_license_plate_samples "
            "WHERE plate_number IS NOT NULL AND TRIM(plate_number) != ''"
        ).fetchall()
        conn.close()
    except sqlite3.Error as e:
        print(f"Warning: could not read ground truth from {ROUTER_DB}: {e}")
        return []

    for row in rows:
        plate = row["plate_number"].strip()
        # A failed cloud parse can leak a JSON fragment into the label column.
        # Require a clean plate shape with a real area code, so junk like
        # '"plate_number": ""' cannot be scored against.
        parsed = plate_format.parse(plate)
        if not (parsed.matched_format and parsed.valid_area_code):
            continue
        path = HARVESTED_DIR / row["image_filename"]
        if path.exists():
            samples.append({"file_path": str(path), "ground_truth": plate})
    return samples


def evaluate(engine: LocalALPREngine, samples: List[Dict[str, Any]], label: str,
             verbose: bool = True) -> Optional[Dict[str, float]]:
    """Run the engine over one sample set and return its metrics."""
    latencies: List[float] = []
    exact = 0
    cer_sum = 0.0

    if verbose:
        print(f"\n--- {label} ({len(samples)} samples) ---")

    for idx, sample in enumerate(samples):
        file_path = sample.get("file_path", "")
        ground_truth = str(sample.get("ground_truth", "")).strip().upper()

        if not file_path or not os.path.exists(file_path) or not ground_truth:
            continue

        t0 = time.perf_counter()
        result = engine.recognize(file_path)
        latencies.append((time.perf_counter() - t0) * 1000)

        pred = str(result.get("plate_number", "")).strip().upper()
        gt_norm, pred_norm = _normalize(ground_truth), _normalize(pred)

        is_exact = pred_norm == gt_norm
        # Normalizing by ground-truth length makes CER comparable across plates;
        # it can exceed 1.0 when the prediction is much longer than the truth.
        cer = levenshtein(pred_norm, gt_norm) / max(len(gt_norm), 1)

        exact += is_exact
        cer_sum += cer

        if verbose and (idx < 10 or not is_exact):
            status = "OK  " if is_exact else "MISS"
            print(
                f"[{idx + 1:02d}] {status} | pred={pred:<15s} gt={ground_truth:<15s} "
                f"cer={cer:4.2f} | {latencies[-1]:6.1f}ms"
            )

    total = len(latencies)
    if total == 0:
        if verbose:
            print("  (no evaluable samples)")
        return None

    ordered = sorted(latencies)
    metrics = {
        "samples": total,
        "exact_match": exact / total,
        "cer": cer_sum / total,
        "p50_ms": ordered[int(total * 0.50)],
        "p95_ms": ordered[min(int(total * 0.95), total - 1)],
    }

    if verbose:
        print(
            f"  exact={metrics['exact_match'] * 100:5.1f}%  "
            f"CER={metrics['cer']:.3f}  "
            f"p50={metrics['p50_ms']:.0f}ms  p95={metrics['p95_ms']:.0f}ms"
        )
    return metrics


def run_benchmark() -> Dict[str, Optional[Dict[str, float]]]:
    print("=" * 68)
    print("            LOCAL ALPR ACCURACY & LATENCY BENCHMARK")
    print("=" * 68)

    if not INDEX_FILE.exists():
        print("Dataset manifest not found. Setting up dataset...")
        setup_datasets()

    with open(INDEX_FILE, "r", encoding="utf-8") as f:
        manifest = json.load(f)

    engine = LocalALPREngine()

    # Curated real photos plus everything harvested from production traffic.
    real = manifest.get("real_samples", []) + load_harvested_samples()
    synthetic = manifest.get("synthetic_samples", [])

    results = {
        "real": evaluate(engine, real, "REAL PHOTOS"),
        "synthetic": evaluate(engine, synthetic, "SYNTHETIC (rendered)"),
    }

    print("\n" + "=" * 68)
    print("SUMMARY (real and synthetic are deliberately not averaged together)")
    print("=" * 68)
    for name, m in results.items():
        if m is None:
            print(f"{name:<10s} no evaluable samples")
        else:
            print(
                f"{name:<10s} n={m['samples']:<4d} exact={m['exact_match'] * 100:5.1f}%  "
                f"CER={m['cer']:.3f}  p50={m['p50_ms']:.0f}ms  p95={m['p95_ms']:.0f}ms"
            )
    print("=" * 68)

    return results


if __name__ == "__main__":
    run_benchmark()
