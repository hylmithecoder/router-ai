"""Export collected SQLite OCR samples into YOLO training format.

Only samples with a real detected bounding box are exported. The previous
version wrote a constant placeholder box (`0 0.5 0.7 0.6 0.25`) for *every*
image, which is worse than exporting nothing: training on it teaches the
detector that the plate always sits at 50%/70% of the frame regardless of the
picture, producing a model less useful than the heuristic it replaces.

Boxes come from the `bbox_*` columns the router fills in when the local ALPR
engine localizes a plate, so the way to grow this dataset is to keep serving
traffic through `/api/v1/ocr/licenseplate`.
"""

import random
import shutil
import sqlite3
from pathlib import Path

import cv2

ROOT_DIR = Path(__file__).resolve().parent.parent.parent
DB_PATH = ROOT_DIR / "router.db"
IMAGES_DIR = ROOT_DIR / "datasets" / "plates" / "images"
YOLO_EXPORT_DIR = ROOT_DIR / "datasets" / "plates" / "yolo_dataset"

# Fixed so that re-running the export does not reshuffle images between the
# train and val splits, which would leak validation images into training.
SPLIT_SEED = 1337


def export_yolo_dataset(val_split: float = 0.2) -> int:
    if not DB_PATH.exists():
        print(f"Database {DB_PATH} not found.")
        return 0

    conn = sqlite3.connect(f"file:{DB_PATH}?mode=ro", uri=True)
    try:
        rows = conn.execute(
            """
            SELECT id, image_filename, plate_number, bbox_x1, bbox_y1, bbox_x2, bbox_y2
            FROM ocr_license_plate_samples
            WHERE plate_number IS NOT NULL AND TRIM(plate_number) != ''
              AND bbox_x1 IS NOT NULL AND bbox_y1 IS NOT NULL
              AND bbox_x2 IS NOT NULL AND bbox_y2 IS NOT NULL
            ORDER BY created_at DESC
            """
        ).fetchall()
    except sqlite3.OperationalError as e:
        print(f"Could not read samples: {e}")
        print("Run the router once so it can migrate the samples table.")
        return 0
    finally:
        conn.close()

    print(f"Found {len(rows)} samples with a real bounding box.")
    if not rows:
        print(
            "Nothing to export. Serve requests through /api/v1/ocr/licenseplate "
            "so the local engine can record where it found each plate."
        )
        return 0

    dirs = {
        (split, kind): YOLO_EXPORT_DIR / kind / split
        for split in ("train", "val")
        for kind in ("images", "labels")
    }
    for d in dirs.values():
        d.mkdir(parents=True, exist_ok=True)

    rng = random.Random(SPLIT_SEED)
    exported = 0

    for sample_id, filename, _plate, x1, y1, x2, y2 in rows:
        img_path = IMAGES_DIR / filename
        if not img_path.exists():
            continue

        image = cv2.imread(str(img_path))
        if image is None:
            print(f"Skipping unreadable image: {filename}")
            continue
        height, width = image.shape[:2]

        label = _to_yolo_label(x1, y1, x2, y2, width, height)
        if label is None:
            print(f"Skipping out-of-bounds box for {filename}")
            continue

        split = "val" if rng.random() < val_split else "train"
        # Prefix with the sample id: two harvested images can share a filename,
        # and a plain copy would silently overwrite one with the other.
        stem = f"{sample_id}_{img_path.stem}"

        shutil.copy2(img_path, dirs[(split, "images")] / f"{stem}{img_path.suffix}")
        (dirs[(split, "labels")] / f"{stem}.txt").write_text(label + "\n")
        exported += 1

    data_yaml = YOLO_EXPORT_DIR / "data.yaml"
    data_yaml.write_text(
        f"""path: {YOLO_EXPORT_DIR.resolve()}
train: images/train
val: images/val

names:
  0: license_plate
"""
    )

    print(f"Exported {exported} samples to {YOLO_EXPORT_DIR}")
    print(f"YOLO configuration written to: {data_yaml}")
    return exported


def _to_yolo_label(
    x1: int, y1: int, x2: int, y2: int, width: int, height: int
) -> "str | None":
    """Convert a pixel box to a normalized YOLO line, or None if it is invalid."""
    if width <= 0 or height <= 0 or x2 <= x1 or y2 <= y1:
        return None

    cx = ((x1 + x2) / 2) / width
    cy = ((y1 + y2) / 2) / height
    bw = (x2 - x1) / width
    bh = (y2 - y1) / height

    if not all(0.0 <= v <= 1.0 for v in (cx, cy, bw, bh)):
        return None

    return f"0 {cx:.6f} {cy:.6f} {bw:.6f} {bh:.6f}"


if __name__ == "__main__":
    export_yolo_dataset()
