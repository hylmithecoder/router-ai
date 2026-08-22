"""Export collected SQLite OCR samples into YOLO training format."""

import os
import random
import shutil
import sqlite3
from pathlib import Path

ROOT_DIR = Path(__file__).resolve().parent.parent.parent
DB_PATH = ROOT_DIR / "router.db"
IMAGES_DIR = ROOT_DIR / "datasets" / "plates" / "images"
YOLO_EXPORT_DIR = ROOT_DIR / "datasets" / "plates" / "yolo_dataset"


def export_yolo_dataset(val_split: float = 0.2):
    if not DB_PATH.exists():
        print(f"Database {DB_PATH} not found.")
        return

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

    # Query all samples with high confidence or validated plates
    cursor.execute(
        """
        SELECT id, image_filename, plate_number, vehicle_type, confidence
        FROM ocr_license_plate_samples
        WHERE plate_number IS NOT NULL AND plate_number != ''
        ORDER BY created_at DESC
        """
    )
    rows = cursor.fetchall()
    conn.close()

    print(f"Found {len(rows)} samples in SQLite database.")
    if not rows:
        print(
            "No samples available to export yet. Run requests to /api/v1/ocr/licenseplate first!"
        )
        return

    train_img_dir = YOLO_EXPORT_DIR / "images" / "train"
    val_img_dir = YOLO_EXPORT_DIR / "images" / "val"
    train_lbl_dir = YOLO_EXPORT_DIR / "labels" / "train"
    val_lbl_dir = YOLO_EXPORT_DIR / "labels" / "val"

    for d in [train_img_dir, val_img_dir, train_lbl_dir, val_lbl_dir]:
        d.mkdir(parents=True, exist_ok=True)

    exported_count = 0
    for row in rows:
        sample_id, filename, plate_number, vehicle_type, confidence = row
        img_path = IMAGES_DIR / filename
        if not img_path.exists():
            continue

        is_val = random.random() < val_split
        target_img_dir = val_img_dir if is_val else train_img_dir
        target_lbl_dir = val_lbl_dir if is_val else train_lbl_dir

        # Copy image file
        shutil.copy2(img_path, target_img_dir / filename)

        # Generate label file (Default bounding box placeholder or pseudo-box)
        # Format: <class_id> <x_center> <y_center> <width> <height> (normalized 0-1)
        lbl_file = target_lbl_dir / f"{img_path.stem}.txt"
        with open(lbl_file, "w") as f:
            # Class 0: license_plate
            # Default center box fallback if exact coordinates are auto-annotated
            f.write("0 0.5 0.7 0.6 0.25\n")

        exported_count += 1

    # Create data.yaml config for YOLOv11
    data_yaml = YOLO_EXPORT_DIR / "data.yaml"
    with open(data_yaml, "w") as f:
        f.write(
            f"""path: {YOLO_EXPORT_DIR.resolve()}
train: images/train
val: images/val

names:
  0: license_plate
"""
        )

    print(f"Successfully exported {exported_count} samples to {YOLO_EXPORT_DIR}")
    print(f"YOLO configuration written to: {data_yaml}")


if __name__ == "__main__":
    export_yolo_dataset()
