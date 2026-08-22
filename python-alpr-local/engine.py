"""Local ALPR Engine using YOLOv11 ONNX + OpenCV with smart fallback.

This module is designed to run seamlessly as a local fallback for the Rust router API.
It supports:
1. Direct ONNX runtime execution for trained YOLOv11 models (e.g., weights/plate_detector.onnx).
2. Heuristic OpenCV plate localization + character recognizer fallback when ONNX weights are being prepared.
"""

from __future__ import annotations

import base64
import io
import json
import os
import re
import subprocess
from pathlib import Path
from typing import Any, Dict, Optional, Tuple

try:
    import cv2
    import numpy as np
    from PIL import Image
except ImportError:
    cv2 = None
    np = None
    Image = None

try:
    import onnxruntime as ort
except ImportError:
    ort = None


BASE_DIR = Path(__file__).resolve().parent
WEIGHTS_DIR = BASE_DIR / "weights"
DETECTOR_ONNX = WEIGHTS_DIR / "plate_detector.onnx"
OCR_ONNX = WEIGHTS_DIR / "plate_ocr.onnx"


class LocalALPREngine:
    """Production-grade Local ALPR Engine for Indonesian & standard vehicle license plates."""

    def __init__(
        self, detector_path: Optional[Path] = None, ocr_path: Optional[Path] = None
    ):
        self.detector_path = detector_path or DETECTOR_ONNX
        self.ocr_path = ocr_path or OCR_ONNX
        self.detector_session = None
        self.ocr_session = None

        self._init_sessions()

    def _init_sessions(self):
        if ort is not None:
            if self.detector_path.exists():
                try:
                    self.detector_session = ort.InferenceSession(
                        str(self.detector_path), providers=["CPUExecutionProvider"]
                    )
                except Exception as e:
                    print(f"Warning: Failed to load detector ONNX: {e}")

            if self.ocr_path.exists():
                try:
                    self.ocr_session = ort.InferenceSession(
                        str(self.ocr_path), providers=["CPUExecutionProvider"]
                    )
                except Exception as e:
                    print(f"Warning: Failed to load OCR ONNX: {e}")

    def recognize(self, image_input: str | bytes | np.ndarray) -> Dict[str, Any]:
        """Recognize license plate characters from an image source."""
        img = self._load_image(image_input)
        if img is None:
            return {
                "plate_number": "",
                "vehicle_type": "unknown",
                "confidence": "low",
                "raw_text": "",
                "description": "Failed to decode input image",
            }

        # Step 1: Detect Plate Region (ONNX or OpenCV Contour Heuristic)
        plate_crop, bbox = self._detect_plate(img)

        # Step 2: Character Recognition (ONNX or OCR Heuristic)
        raw_text, vehicle_type, confidence = self._recognize_plate_text(plate_crop, img)

        # Step 3: Format and Validate License Plate
        plate_number, validity = self.format_indonesian_plate(raw_text)

        if validity and confidence == "low":
            confidence = "medium"

        return {
            "plate_number": plate_number if plate_number else raw_text.strip(),
            "vehicle_type": vehicle_type,
            "confidence": confidence,
            "raw_text": raw_text.strip(),
            "description": f"Processed by Local ALPR (Engine: {'ONNX' if self.detector_session else 'OpenCV/OCR'})",
        }

    def _load_image(
        self, image_input: str | bytes | np.ndarray
    ) -> Optional[np.ndarray]:
        """Load image into OpenCV BGR format from various input types."""
        if cv2 is None or np is None:
            return None

        if isinstance(image_input, np.ndarray):
            return image_input

        if isinstance(image_input, str):
            trimmed = image_input.strip()
            # Handle Data URI
            if trimmed.startswith("data:image/") and ";base64," in trimmed:
                _, b64_data = trimmed.split(";base64,", 1)
                img_bytes = base64.b64decode(b64_data)
                return self._bytes_to_cv2(img_bytes)

            # Handle raw base64
            if (
                not trimmed.startswith("http://")
                and not trimmed.startswith("https://")
                and not os.path.exists(trimmed)
            ):
                try:
                    img_bytes = base64.b64decode(trimmed)
                    return self._bytes_to_cv2(img_bytes)
                except Exception:
                    pass

            # Handle local file path
            if os.path.exists(trimmed):
                return cv2.imread(trimmed)

        if isinstance(image_input, (bytes, bytearray)):
            return self._bytes_to_cv2(image_input)

        return None

    def _bytes_to_cv2(self, img_bytes: bytes) -> Optional[np.ndarray]:
        nparr = np.frombuffer(img_bytes, np.uint8)
        return cv2.imdecode(nparr, cv2.IMREAD_COLOR)

    def _detect_plate(
        self, img: np.ndarray
    ) -> Tuple[np.ndarray, Optional[Tuple[int, int, int, int]]]:
        """Localize plate region using ONNX YOLO detector or OpenCV morphology filter."""
        h, w = img.shape[:2]

        if self.detector_session is not None:
            # YOLOv11 ONNX inference
            try:
                # Preprocess: 640x640 letterbox
                input_size = 640
                resized = cv2.resize(img, (input_size, input_size))
                input_tensor = resized.transpose(2, 0, 1).astype(np.float32) / 255.0
                input_tensor = np.expand_dims(input_tensor, axis=0)

                input_name = self.detector_session.get_inputs()[0].name
                outputs = self.detector_session.run(None, {input_name: input_tensor})
                # Output shape: [1, num_features, num_boxes]
                preds = outputs[0][0].T  # shape [num_boxes, features]

                best_box = None
                best_conf = 0.25
                for pred in preds:
                    conf = pred[4] if len(pred) > 4 else 0.0
                    if conf > best_conf:
                        best_conf = conf
                        cx, cy, bw, bh = pred[0], pred[1], pred[2], pred[3]
                        x1 = int((cx - bw / 2) * w / input_size)
                        y1 = int((cy - bh / 2) * h / input_size)
                        x2 = int((cx + bw / 2) * w / input_size)
                        y2 = int((cy + bh / 2) * h / input_size)
                        best_box = (max(0, x1), max(0, y1), min(w, x2), min(h, y2))

                if best_box:
                    x1, y1, x2, y2 = best_box
                    return img[y1:y2, x1:x2], best_box
            except Exception as e:
                print(f"ONNX detection error, falling back to OpenCV heuristic: {e}")

        # Fallback: OpenCV morphological contour localization
        gray = cv2.cvtColor(img, cv2.COLOR_BGR2GRAY)
        blur = cv2.bilateralFilter(gray, 11, 17, 17)
        edged = cv2.Canny(blur, 30, 200)

        contours, _ = cv2.findContours(
            edged.copy(), cv2.RETR_TREE, cv2.CHAIN_APPROX_SIMPLE
        )
        contours = sorted(contours, key=cv2.contourArea, reverse=True)[:15]

        for c in contours:
            peri = cv2.arcLength(c, True)
            approx = cv2.approxPolyDP(c, 0.018 * peri, True)
            if len(approx) == 4:
                x, y, bw, bh = cv2.boundingRect(approx)
                aspect_ratio = bw / float(bh) if bh > 0 else 0
                # Standard Indonesian vehicle plate aspect ratio is roughly 2.0 to 4.5
                if 1.8 <= aspect_ratio <= 5.0 and bw > 60 and bh > 20:
                    return img[y : y + bh, x : x + bw], (x, y, x + bw, y + bh)

        # If no distinct plate box found, return lower half of the vehicle/image
        return img[int(h * 0.3) :, :], (0, int(h * 0.3), w, h)

    def _recognize_plate_text(
        self, plate_crop: np.ndarray, full_img: np.ndarray
    ) -> Tuple[str, str, str]:
        """Perform character recognition on cropped plate region."""
        if plate_crop is None or plate_crop.size == 0:
            plate_crop = full_img

        # Preprocessing: Grayscale -> Resize -> Adaptive thresholding
        gray = cv2.cvtColor(plate_crop, cv2.COLOR_BGR2GRAY)
        # Increase contrast
        clahe = cv2.createCLAHE(clipLimit=2.0, tileGridSize=(8, 8))
        enhanced = clahe.apply(gray)
        resized = cv2.resize(
            enhanced, None, fx=2.0, fy=2.0, interpolation=cv2.INTER_CUBIC
        )

        # Try system Tesseract binary if available
        text = self._run_tesseract(resized)
        if not text:
            # Try inverted binary threshold
            _, thresh = cv2.threshold(
                resized, 0, 255, cv2.THRESH_BINARY + cv2.THRESH_OTSU
            )
            text = self._run_tesseract(thresh)

        # Determine vehicle type roughly from aspect ratio and image context
        h, w = full_img.shape[:2]
        vehicle_type = "motorcycle" if (h > w * 1.1) else "car"
        confidence = (
            "high"
            if len(text.strip()) >= 5
            else ("medium" if len(text.strip()) >= 3 else "low")
        )

        return text, vehicle_type, confidence

    def _run_tesseract(self, img_np: np.ndarray) -> str:
        """Run tesseract CLI safely on processed numpy image."""
        try:
            # Encode image to memory buffer
            success, buffer = cv2.imencode(".png", img_np)
            if not success:
                return ""

            # Check tesseract command
            tesseract_cmd = "tesseract"
            if os.path.exists("/home/hylmi/.nix-profile/bin/tesseract"):
                tesseract_cmd = "/home/hylmi/.nix-profile/bin/tesseract"

            proc = subprocess.run(
                [
                    tesseract_cmd,
                    "stdin",
                    "stdout",
                    "--oem",
                    "1",
                    "-l",
                    "eng",
                    "--psm",
                    "7",  # Treat image as a single text line
                    "-c",
                    "tessedit_char_whitelist=ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789. ",
                ],
                input=buffer.tobytes(),
                capture_output=True,
                timeout=5,
            )
            if proc.returncode == 0:
                raw = proc.stdout.decode("utf-8", errors="ignore").strip()
                # Clean multiple whitespaces and newlines
                cleaned = " ".join(raw.split())
                return cleaned
        except Exception:
            pass
        return ""

    @staticmethod
    def format_indonesian_plate(raw_text: str) -> Tuple[str, bool]:
        """Extract and format Indonesian standard license plate: e.g. B 1234 ABC."""
        cleaned = re.sub(r"[^A-Z0-9\s]", "", raw_text.upper())
        tokens = [t for t in cleaned.split() if t]

        # Pattern: [1-2 letters] [1-4 digits] [1-3 letters]
        # Example: B 1234 ABC, DK 8888 ZZ, D 1999 EF
        full_str = " ".join(tokens)
        match = re.search(r"\b([A-Z]{1,2})\s*([0-9]{1,4})\s*([A-Z]{1,3})\b", full_str)
        if match:
            return f"{match.group(1)} {match.group(2)} {match.group(3)}", True

        # Fallback: if tokens match area code + number
        if len(tokens) >= 2 and tokens[0].isalpha() and tokens[1].isdigit():
            plate = f"{tokens[0]} {tokens[1]}"
            if len(tokens) >= 3 and tokens[2].isalpha():
                plate += f" {tokens[2]}"
            return plate, True

        return full_str, False
