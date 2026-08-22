#!/usr/bin/env python3
"""Benchmark and Evaluation Suite for Local ALPR Engine.

Measures latency (milliseconds), plate detection accuracy, and character recognition
against real and synthetic Indonesian license plate datasets.
"""

import json
import os
import time
from pathlib import Path
from typing import Dict, List

from dataset_collector import setup_datasets
from engine import LocalALPREngine

BASE_DIR = Path(__file__).resolve().parent
DATASETS_DIR = BASE_DIR / "datasets"
INDEX_FILE = DATASETS_DIR / "manifest.json"


def run_benchmark():
    print("=" * 60)
    print("      LOCAL ALPR PERFORMANCE & ACCURACY BENCHMARK      ")
    print("=" * 60)

    if not INDEX_FILE.exists():
        print("Dataset manifest not found. Setting up dataset...")
        setup_datasets()

    with open(INDEX_FILE, "r", encoding="utf-8") as f:
        manifest = json.load(f)

    samples = manifest.get("real_samples", []) + manifest.get("synthetic_samples", [])
    if not samples:
        print("No dataset samples found to evaluate.")
        return

    print(f"Loaded {len(samples)} test samples from dataset.")
    print("-" * 60)

    engine = LocalALPREngine()
    latencies = []
    correct_exact = 0
    correct_partial = 0

    for idx, sample in enumerate(samples):
        file_path = sample.get("file_path")
        ground_truth = sample.get("ground_truth", "").strip().upper()

        if not os.path.exists(file_path):
            continue

        t0 = time.perf_counter()
        result = engine.recognize(file_path)
        t1 = time.perf_counter()

        elapsed_ms = (t1 - t0) * 1000
        latencies.append(elapsed_ms)

        pred_plate = result.get("plate_number", "").strip().upper()
        # Clean spacing for partial match
        pred_clean = pred_plate.replace(" ", "")
        gt_clean = ground_truth.replace(" ", "")

        is_exact = (pred_plate == ground_truth)
        is_partial = (gt_clean in pred_clean or pred_clean in gt_clean) and len(pred_clean) >= 3

        if is_exact:
            correct_exact += 1
            correct_partial += 1
            status = "✓ EXACT"
        elif is_partial:
            correct_partial += 1
            status = "≈ PARTIAL"
        else:
            status = "✗ MISMATCH"

        if idx < 10 or not is_exact:
            print(f"[{idx+1:02d}] {status:10s} | Pred: {pred_plate:15s} | GT: {ground_truth:15s} | Latency: {elapsed_ms:5.1f}ms")

    total = len(latencies)
    avg_latency = sum(latencies) / total if total > 0 else 0
    p95_latency = sorted(latencies)[int(total * 0.95)] if total > 0 else 0

    print("=" * 60)
    print("                     EVALUATION RESULTS                 ")
    print("=" * 60)
    print(f"Total Evaluated Samples : {total}")
    print(f"Average Inference Speed : {avg_latency:.2f} ms")
    print(f"P95 Latency             : {p95_latency:.2f} ms")
    print(f"Exact Plate Match Rate  : {correct_exact}/{total} ({(correct_exact/total)*100:.1f}%)")
    print(f"Partial Match / Hit Rate: {correct_partial}/{total} ({(correct_partial/total)*100:.1f}%)")
    print("=" * 60)


if __name__ == "__main__":
    run_benchmark()
