"""Automated Benchmark & Evaluation Harness for Local ALPR vs Ground Truth."""

import os
import sqlite3
import sys
import time
from pathlib import Path

# Add project root to sys.path
BASE_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(BASE_DIR))

from engine import LocalALPREngine

ROOT_DIR = BASE_DIR.parent
DB_PATH = ROOT_DIR / "router.db"
IMAGES_DIR = ROOT_DIR / "datasets" / "plates" / "images"


def levenshtein_distance(s1: str, s2: str) -> int:
    """Calculate character edit distance between two strings."""
    if len(s1) < len(s2):
        return levenshtein_distance(s2, s1)
    if len(s2) == 0:
        return len(s1)

    previous_row = range(len(s2) + 1)
    for i, c1 in enumerate(s1):
        current_row = [i + 1]
        for j, c2 in enumerate(s2):
            insertions = previous_row[j + 1] + 1
            deletions = current_row[j] + 1
            substitutions = previous_row[j] + (c1 != c2)
            current_row.append(min(insertions, deletions, substitutions))
        previous_row = current_row
    return previous_row[-1]


def run_benchmark():
    print("=" * 70)
    print("  Local ALPR Benchmark & Evaluation Suite")
    print("=" * 70)

    if not DB_PATH.exists():
        print(f"Database {DB_PATH} not found. Running synthetic benchmark test...")
        samples = []
    else:
        conn = sqlite3.connect(str(DB_PATH))
        cursor = conn.cursor()
        cursor.execute(
            """
            CREATE TABLE IF NOT EXISTS ocr_license_plate_samples (
                id TEXT PRIMARY KEY,
                image_filename TEXT NOT NULL,
                plate_number TEXT NOT NULL,
                vehicle_type TEXT,
                confidence TEXT,
                raw_text TEXT,
                description TEXT,
                model TEXT NOT NULL,
                provider TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            """
        )
        cursor.execute(
            """
            SELECT id, image_filename, plate_number, vehicle_type, confidence, model, provider
            FROM ocr_license_plate_samples
            WHERE plate_number IS NOT NULL AND plate_number != ''
            ORDER BY created_at DESC
            LIMIT 100
            """
        )
        samples = cursor.fetchall()
        conn.close()

    engine = LocalALPREngine()
    print(
        f"Engine mode: {'YOLO ONNX' if engine.detector_session else 'OpenCV Heuristic / System OCR'}"
    )
    print(f"Total test samples available: {len(samples)}\n")

    if not samples:
        print(
            "Tip: Test images will automatically accumulate in `datasets/plates/images/`"
        )
        print("     when requests are sent to `POST /api/v1/ocr/licenseplate`.")
        return

    exact_matches = 0
    total_cer = 0.0
    total_latency_ms = 0.0
    results_table = []

    for idx, sample in enumerate(samples, start=1):
        sample_id, filename, ground_truth, vtype, conf, model_src, prov = sample
        img_path = IMAGES_DIR / filename

        if not img_path.exists():
            continue

        start_time = time.perf_counter()
        output = engine.recognize(str(img_path))
        elapsed_ms = (time.perf_counter() - start_time) * 1000.0

        pred_plate = output.get("plate_number", "").strip().upper()
        gt_plate = ground_truth.strip().upper()

        # Clean spaces for comparison
        clean_pred = "".join(pred_plate.split())
        clean_gt = "".join(gt_plate.split())

        is_exact = clean_pred == clean_gt
        if is_exact:
            exact_matches += 1

        dist = levenshtein_distance(clean_pred, clean_gt)
        cer = dist / max(len(clean_gt), 1)
        total_cer += cer
        total_latency_ms += elapsed_ms

        status = "MATCH" if is_exact else "DIFF"
        results_table.append(
            {
                "id": sample_id[:8],
                "ground_truth": gt_plate,
                "predicted": pred_plate,
                "latency_ms": f"{elapsed_ms:.1f}ms",
                "status": status,
            }
        )

    tested_count = len(results_table)
    if tested_count == 0:
        print("No image files found in datasets directory.")
        return

    # Print summary results
    print(
        f"{'#':<3} | {'Sample ID':<8} | {'Ground Truth (Cloud AI)':<22} | {'Predicted (Local ALPR)':<22} | {'Latency':<8} | {'Status'}"
    )
    print("-" * 85)
    for i, res in enumerate(results_table[:20], start=1):
        print(
            f"{i:<3} | {res['id']:<8} | {res['ground_truth']:<22} | {res['predicted']:<22} | {res['latency_ms']:<8} | {res['status']}"
        )

    if tested_count > 20:
        print(f"... and {tested_count - 20} more samples evaluated.")

    ema = (exact_matches / tested_count) * 100.0
    avg_cer = (total_cer / tested_count) * 100.0
    avg_latency = total_latency_ms / tested_count

    print("\n" + "=" * 70)
    print("  Benchmark Summary Metrics:")
    print("=" * 70)
    print(f"  • Total Evaluated      : {tested_count} images")
    print(f"  • Exact Match Accuracy : {ema:.2f}%")
    print(f"  • Average CER          : {avg_cer:.2f}%")
    print(f"  • Average Latency      : {avg_latency:.2f} ms")
    print("=" * 70)


if __name__ == "__main__":
    run_benchmark()
