#!/usr/bin/env python3
"""Synthetic Indonesian License Plate Generator.

Generates realistic Indonesian vehicle license plate images for dataset augmentation,
training, and local ALPR benchmarking. Supports:
- Black plate with white text (Old format)
- White plate with black text (New standard format)
- Samsat area codes (B, D, L, DK, AB, F, N, BG, BK, etc.)
- Expiration subtext (e.g., '08.29')
- Perspective transform, noise, and illumination variance
"""

import os
import random
import re
from pathlib import Path
from typing import Dict, List, Optional, Tuple

try:
    import cv2
    import numpy as np
    from PIL import Image, ImageDraw, ImageFont, ImageFilter
except ImportError:
    cv2 = None
    np = None
    Image = None

AREA_CODES = [
    "B", "D", "L", "DK", "AB", "F", "N", "H", "AD", "E",
    "AG", "AE", "AA", "K", "M", "S", "W", "BG", "BK", "BM", "BA"
]

SUFFIXES = [
    "ABC", "XYZ", "BCA", "ZZ", "EFG", "JKL", "MN", "XY", "PR", "GT",
    "SR", "VIP", "RS", "CD", "AB", "DK", "SK", "MH", "NA", "ID"
]


def generate_random_plate_text() -> Tuple[str, str]:
    """Generate (plate_number, expiry_date)."""
    area = random.choice(AREA_CODES)
    num = str(random.randint(1, 9999))
    suffix = random.choice(SUFFIXES)
    month = f"{random.randint(1, 12):02d}"
    year = f"{random.randint(25, 31):02d}"
    plate = f"{area} {num} {suffix}"
    expiry = f"{month}.{year}"
    return plate, expiry


def create_synthetic_plate_image(
    plate_text: Optional[str] = None,
    expiry_text: Optional[str] = None,
    plate_type: str = "white_new",  # "white_new" or "black_old"
    width: int = 400,
    height: int = 140,
) -> Optional[np.ndarray]:
    """Create synthetic license plate image as OpenCV BGR numpy array."""
    if Image is None or cv2 is None or np is None:
        return None

    if not plate_text:
        plate_text, auto_expiry = generate_random_plate_text()
        if not expiry_text:
            expiry_text = auto_expiry

    if not expiry_text:
        expiry_text = "08.29"

    # Plate color settings
    if plate_type == "white_new":
        bg_color = (random.randint(235, 250), random.randint(235, 250), random.randint(235, 250))
        fg_color = (random.randint(10, 30), random.randint(10, 30), random.randint(10, 30))
        border_color = (30, 30, 30)
    else:
        bg_color = (random.randint(20, 35), random.randint(20, 35), random.randint(20, 35))
        fg_color = (random.randint(230, 250), random.randint(230, 250), random.randint(230, 250))
        border_color = (220, 220, 220)

    # Base PIL image
    img_pil = Image.new("RGB", (width, height), bg_color)
    draw = ImageDraw.Draw(img_pil)

    # Draw border
    border_width = random.randint(3, 5)
    draw.rectangle(
        [(border_width, border_width), (width - border_width, height - border_width)],
        outline=border_color,
        width=border_width,
    )

    # Fonts
    try:
        font_main = ImageFont.truetype("DejaVuSans-Bold.ttf", 52)
        font_sub = ImageFont.truetype("DejaVuSans-Bold.ttf", 20)
    except Exception:
        font_main = ImageFont.load_default()
        font_sub = ImageFont.load_default()

    # Draw main plate text centered
    bbox_main = draw.textbbox((0, 0), plate_text, font=font_main)
    w_main = bbox_main[2] - bbox_main[0]
    h_main = bbox_main[3] - bbox_main[1]
    x_main = (width - w_main) // 2
    y_main = int(height * 0.22)
    draw.text((x_main, y_main), plate_text, fill=fg_color, font=font_main)

    # Draw expiration date
    bbox_sub = draw.textbbox((0, 0), expiry_text, font=font_sub)
    w_sub = bbox_sub[2] - bbox_sub[0]
    x_sub = width - w_sub - 30
    y_sub = height - 35
    draw.text((x_sub, y_sub), expiry_text, fill=fg_color, font=font_sub)

    # Convert to OpenCV BGR
    img_cv = cv2.cvtColor(np.array(img_pil), cv2.COLOR_RGB2BGR)

    # Apply realistic distortions
    # 1. Gaussian noise
    noise = np.random.normal(0, random.uniform(2, 6), img_cv.shape).astype(np.float32)
    img_cv = np.clip(img_cv.astype(np.float32) + noise, 0, 255).astype(np.uint8)

    # 2. Slight perspective tilt / skew
    pts1 = np.float32([[0, 0], [width, 0], [0, height], [width, height]])
    dx1 = random.randint(0, 10)
    dy1 = random.randint(0, 8)
    dx2 = random.randint(0, 10)
    dy2 = random.randint(0, 8)
    pts2 = np.float32([
        [dx1, dy1],
        [width - dx2, dy1],
        [dx1 + random.randint(-5, 5), height - dy2],
        [width - dx2 + random.randint(-5, 5), height - dy2],
    ])
    matrix = cv2.getPerspectiveTransform(pts1, pts2)
    img_cv = cv2.warpPerspective(img_cv, matrix, (width, height), borderMode=cv2.BORDER_REPLICATE)

    return img_cv


def generate_dataset(output_dir: Path, num_samples: int = 50) -> List[Dict[str, str]]:
    """Generate a batch of labeled synthetic plates."""
    output_dir.mkdir(parents=True, exist_ok=True)
    metadata = []

    for i in range(num_samples):
        plate_text, expiry = generate_random_plate_text()
        plate_type = random.choice(["white_new", "black_old"])
        img = create_synthetic_plate_image(plate_text, expiry, plate_type)
        if img is not None:
            filename = f"plate_{i:04d}_{plate_text.replace(' ', '_')}.jpg"
            file_path = output_dir / filename
            cv2.imwrite(str(file_path), img)
            metadata.append({
                "id": f"sample_{i:04d}",
                "filename": filename,
                "file_path": str(file_path),
                "ground_truth": plate_text,
                "plate_type": plate_type,
                "expiry": expiry,
            })

    return metadata


if __name__ == "__main__":
    out = Path(__file__).resolve().parent / "datasets" / "synthetic"
    meta = generate_dataset(out, num_samples=30)
    print(f"Generated {len(meta)} synthetic Indonesian license plate samples in {out}")
