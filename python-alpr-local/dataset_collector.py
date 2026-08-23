#!/usr/bin/env python3
"""Dataset Collector & Manager for Indonesian License Plates.

Collects, downloads, and indexes Indonesian license plate images with labels
for continuous evaluation, accuracy benchmarking, and training.
"""

import json
import os
import urllib.request
from pathlib import Path
from typing import Any, Dict, List, Optional

BASE_DIR = Path(__file__).resolve().parent
DATASETS_DIR = BASE_DIR / "datasets"
REAL_DIR = DATASETS_DIR / "real"
INDEX_FILE = DATASETS_DIR / "manifest.json"

# Curated high-quality open public Indonesian license plate samples
CURATED_REAL_SAMPLES = [
    {
        "id": "real_b_1234_abc",
        "url": "https://upload.wikimedia.org/wikipedia/commons/thumb/d/d0/Indonesian_license_plate_B_1234_ABC.jpg/800px-Indonesian_license_plate_B_1234_ABC.jpg",
        "ground_truth": "B 1234 ABC",
        "vehicle_type": "car",
        "plate_type": "black_old",
    },
    {
        "id": "real_dk_car",
        "url": "https://images.unsplash.com/photo-1549399542-7e3f8b79c341?w=800",
        "ground_truth": "DK 1945 ZZ",
        "vehicle_type": "car",
        "plate_type": "white_new",
    },
]


def download_sample(url: str, output_path: Path) -> bool:
    """Safely download image sample with custom browser headers."""
    try:
        req = urllib.request.Request(
            url,
            headers={
                "User-Agent": "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
                "Accept": "image/webp,image/apng,image/*,*/*;q=0.8",
            },
        )
        with urllib.request.urlopen(req, timeout=10) as response:
            content = response.read()
            if len(content) > 200:
                output_path.parent.mkdir(parents=True, exist_ok=True)
                with open(output_path, "wb") as f:
                    f.write(content)
                return True
    except Exception as e:
        print(f"Warning: Failed to download {url}: {e}")
    return False


def setup_datasets() -> Dict[str, Any]:
    """Initialize and populate datasets folder with real and synthetic samples."""
    DATASETS_DIR.mkdir(parents=True, exist_ok=True)
    REAL_DIR.mkdir(parents=True, exist_ok=True)

    manifest = {"real_samples": [], "synthetic_samples": []}

    # 1. Download real curated samples
    for sample in CURATED_REAL_SAMPLES:
        ext = "jpg"
        out_file = REAL_DIR / f"{sample['id']}.{ext}"
        if not out_file.exists():
            download_sample(sample["url"], out_file)

        if out_file.exists():
            manifest["real_samples"].append({
                "id": sample["id"],
                "file_path": str(out_file),
                "ground_truth": sample["ground_truth"],
                "vehicle_type": sample.get("vehicle_type", "car"),
                "plate_type": sample.get("plate_type", "standard"),
            })

    # 2. Generate synthetic plates
    from synthetic_generator import generate_dataset
    synthetic_dir = DATASETS_DIR / "synthetic"
    synth_meta = generate_dataset(synthetic_dir, num_samples=25)
    manifest["synthetic_samples"] = synth_meta

    # Save manifest
    with open(INDEX_FILE, "w", encoding="utf-8") as f:
        json.dump(manifest, f, indent=2)

    return manifest


if __name__ == "__main__":
    result = setup_datasets()
    print(f"Dataset setup completed: {len(result['real_samples'])} real, {len(result['synthetic_samples'])} synthetic.")
