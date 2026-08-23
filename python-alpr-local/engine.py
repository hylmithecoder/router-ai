"""Local ALPR engine: pretrained plate detector + plate-specific OCR, on CPU.

Both stages use models trained on license plates rather than general-purpose
tooling, which is the whole point of this rewrite:

* **Detection** - a YOLOv9 ONNX model trained on plates, via `fast_alpr`. The
  previous Canny + `approxPolyDP` heuristic only fired on a clean quadrilateral
  contour, so on real vehicle photos it almost always fell through to "crop the
  bottom 65% of the image" and handed a whole car to the OCR stage.
* **Recognition** - a CCT model trained on plate glyphs, which also returns a
  real per-character confidence. The previous stage shelled out to Tesseract's
  English LSTM model: trained on prose, dictionary-corrected into English words,
  and unable to honour a character whitelist, so 0/O, 1/I and 8/B were
  completely unconstrained.

Indonesian-specific structure (group classes, area codes, two-row motorcycle
plates, the validity sticker) is applied afterwards by `plate_format`.

Set `ALPR_ENABLE_TESSERACT=1` to keep the old Tesseract path available as a
fallback when the ONNX models cannot be loaded.
"""

from __future__ import annotations

import base64
import os
import subprocess
import urllib.request
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

import plate_format

try:
    import cv2
    import numpy as np
except ImportError:
    cv2 = None
    np = None

try:
    from fast_alpr import ALPR
except ImportError:
    ALPR = None


BASE_DIR = Path(__file__).resolve().parent

# Detector/OCR checkpoints are downloaded and cached by fast_alpr on first use.
#
# Measured on the 13 real harvested photos (benchmark.py), CER lower is better:
#     yolo-v9-t-384   CER 0.232   p50 258ms
#     yolo-v9-t-640   CER 0.462   p50 438ms
#     yolo-v9-s-608   CER 0.233   p50 674ms
# t-384 ties the large model at a third of the cost, so it is the default. Treat
# that as provisional: n=13 is far too small to separate 0.232 from 0.233. The
# one case where s-608 won was a distant, low-resolution plate that t-384 missed
# entirely, so switch via ALPR_DETECTOR_MODEL if far-field plates matter.
DETECTOR_MODEL = os.getenv("ALPR_DETECTOR_MODEL", "yolo-v9-t-384-license-plate-end2end")
OCR_MODEL = os.getenv("ALPR_OCR_MODEL", "cct-s-v2-global-model")
DETECTOR_CONF = float(os.getenv("ALPR_DETECTOR_CONF", "0.35"))
ENABLE_TESSERACT = os.getenv("ALPR_ENABLE_TESSERACT", "0") == "1"

# Fraction of the detected box added as margin on each side before cropping.
BBOX_PAD_RATIO = float(os.getenv("ALPR_BBOX_PAD", "0.06"))

# A single-row car plate is roughly 3-4.5x wider than tall; a two-row motorcycle
# plate is much squarer. Below this ratio it is worth looking for stacked rows.
TWO_ROW_ASPECT_MAX = 2.6


class LocalALPREngine:
    """Local ALPR engine for Indonesian and standard vehicle license plates."""

    def __init__(
        self,
        detector_model: str = DETECTOR_MODEL,
        ocr_model: str = OCR_MODEL,
        detector_conf: float = DETECTOR_CONF,
    ):
        self.alpr = None
        self.load_error: Optional[str] = None

        if ALPR is None:
            self.load_error = "fast_alpr is not installed (run python-alpr-local/setup.sh)"
        else:
            try:
                self.alpr = ALPR(
                    detector_model=detector_model,
                    ocr_model=ocr_model,
                    detector_conf_thresh=detector_conf,
                    detector_providers=["CPUExecutionProvider"],
                    ocr_device="cpu",
                )
            except Exception as e:  # model download / onnxruntime failure
                self.load_error = f"failed to initialize ALPR models: {e}"

    # ------------------------------------------------------------------ public

    def recognize(self, image_input: "str | bytes | np.ndarray") -> Dict[str, Any]:
        """Recognize a license plate from an image source.

        Returns a dict with the API's existing keys plus `confidence_score`
        (a measured 0..1 float) and `bbox` (the detected plate region, which the
        router harvests as a free training label).
        """
        img = self._load_image(image_input)
        if img is None:
            return self._empty_result("Failed to decode input image")

        if self.alpr is not None:
            result = self._recognize_with_alpr(img)
            if result is not None:
                return result

        if ENABLE_TESSERACT:
            return self._recognize_with_tesseract(img)

        return self._empty_result(
            self.load_error or "Local ALPR did not detect any license plate"
        )

    # ---------------------------------------------------------------- pipeline

    def _recognize_with_alpr(self, img: "np.ndarray") -> Optional[Dict[str, Any]]:
        """Run detection + OCR, returning None when no plate was detected."""
        try:
            detections = self.alpr.predict(img)
        except Exception as e:
            print(f"Warning: ALPR inference failed: {e}")
            return None

        if detections:
            # Prefer the plate the detector is most sure about.
            best = max(detections, key=lambda d: d.detection.confidence)
            bbox = _pad_bbox(best.detection.bounding_box, img.shape[1], img.shape[0])
            if bbox.is_empty:
                return None
            rows, cols = bbox.as_slices()
            crop = img[rows, cols]
            # Read the padded crop *in addition to* fast_alpr's own read of the
            # tight box. A tight box can clip the last character ("WAJ" -> "WA"),
            # while extra margin can pull in a bumper edge; scoring picks whichever
            # actually fits the plate format instead of guessing up front.
            whole_ocr = [best.ocr, self._ocr_image(crop)]
            det_conf = float(best.detection.confidence)
            bbox_xyxy = list(bbox.xyxy)
        else:
            # The detector is trained on scenes containing a vehicle, so it does
            # not fire on an image that is *already* a tight plate crop -- which
            # is exactly what callers often post. Read the frame directly instead.
            crop = self._as_plate_crop(img)
            if crop is None:
                return None
            whole_ocr = [self._ocr_image(crop)]
            det_conf = 0.0
            bbox_xyxy = [0, 0, img.shape[1], img.shape[0]]

        candidates, row_texts, row_count = self._read_crop(crop, whole_ocr)
        winner = plate_format.best(candidates)

        # Two stacked text rows is the reliable signal for a motorcycle plate --
        # far better than the old guess based on the whole photo's aspect ratio.
        vehicle_type = "motorcycle" if row_count >= 2 else "car"

        if winner is None:
            return {
                **self._empty_result("Local ALPR detected a plate but could not read it"),
                "vehicle_type": vehicle_type,
                "raw_text": plate_format.join_rows(row_texts),
                "bbox": bbox_xyxy,
                "detector_confidence": round(det_conf, 4),
            }

        return {
            "plate_number": winner.plate,
            "vehicle_type": vehicle_type,
            "confidence": winner.confidence_bucket,
            "confidence_score": round(winner.score, 4),
            "raw_text": plate_format.join_rows(row_texts),
            "description": f"Processed by Local ALPR ({DETECTOR_MODEL} + {OCR_MODEL})",
            "bbox": bbox_xyxy,
            "detector_confidence": round(det_conf, 4),
        }

    def _as_plate_crop(self, img: "np.ndarray") -> "Optional[np.ndarray]":
        """Treat the frame itself as a plate crop, when its shape allows it.

        Guards against feeding a full vehicle photo to the OCR stage: only
        images already shaped like a plate qualify. Both single-row (wide) and
        two-row motorcycle (squarer) proportions are accepted.
        """
        h, w = img.shape[:2]
        if h == 0 or w == 0:
            return None
        return img if 1.0 <= w / float(h) <= 6.0 else None

    def _read_crop(
        self, crop: "np.ndarray", whole_crop_ocr: List[Any]
    ) -> Tuple[List[plate_format.PlateCandidate], List[str], int]:
        """Read a plate crop as a whole and, if it is stacked, row by row.

        Indonesian motorcycle plates put the area code on one row and the number
        on the next, so a single-line read of the whole crop mangles them. Every
        reading is returned as a candidate and scored against the others, so
        whichever genuinely fits the plate format wins.
        """
        candidates: List[plate_format.PlateCandidate] = []
        row_texts: List[str] = []

        for ocr in whole_crop_ocr:
            if ocr is None or not ocr.text:
                continue
            candidates.append(plate_format.parse(ocr.text, _mean_confidence(ocr.confidence)))
            if not row_texts:
                row_texts.append(ocr.text)

        rows = self._split_rows(crop)
        if len(rows) < 2:
            return candidates, row_texts, len(rows) or 1

        per_row: List[str] = []
        confidences: List[float] = []
        for row_img in rows:
            ocr = self._ocr_image(row_img)
            if ocr is None or not ocr.text:
                continue
            per_row.append(ocr.text)
            confidences.append(_mean_confidence(ocr.confidence))

        if per_row:
            # join_rows drops the validity row (e.g. "05.28") so its digits cannot
            # be mistaken for the plate number.
            joined = plate_format.join_rows(per_row)
            kept = [c for text, c in zip(per_row, confidences)
                    if not plate_format.is_expiry_line(text)]
            candidates.append(
                plate_format.parse(joined, sum(kept) / len(kept) if kept else 0.0)
            )
            row_texts = per_row

        return candidates, row_texts, len(rows)

    def _split_rows(self, crop: "np.ndarray") -> List["np.ndarray"]:
        """Split a plate crop into horizontal text rows via ink projection.

        Returns a single-element list when the plate is one row, which is the
        common case for cars.
        """
        h, w = crop.shape[:2]
        if h == 0 or w == 0:
            return []
        if w / float(h) > TWO_ROW_ASPECT_MAX:
            return [crop]

        gray = cv2.cvtColor(crop, cv2.COLOR_BGR2GRAY)
        _, binary = cv2.threshold(gray, 0, 255, cv2.THRESH_BINARY_INV + cv2.THRESH_OTSU)
        # Characters must be the minority of pixels; if not, Otsu picked the
        # background and the image needs inverting (black plate, white glyphs).
        if binary.mean() > 127:
            binary = cv2.bitwise_not(binary)

        ink_per_row = (binary > 0).sum(axis=1)
        min_ink = max(1, int(0.06 * w))

        bands: List[Tuple[int, int]] = []
        start = None
        for y, ink in enumerate(ink_per_row):
            if ink >= min_ink and start is None:
                start = y
            elif ink < min_ink and start is not None:
                bands.append((start, y))
                start = None
        if start is not None:
            bands.append((start, h))

        # Drop bands too thin to hold characters (borders, bolts, screw shadows).
        min_band = max(4, int(0.12 * h))
        bands = [(y1, y2) for y1, y2 in bands if y2 - y1 >= min_band]

        if len(bands) < 2:
            return [crop]

        pad = max(2, int(0.04 * h))
        return [
            crop[max(0, y1 - pad) : min(h, y2 + pad), :]
            for y1, y2 in bands[:3]  # area code, number, validity sticker
        ]

    def _ocr_image(self, img: "np.ndarray") -> Any:
        """Run the OCR stage directly on an already-cropped image."""
        try:
            return self.alpr.ocr.predict(img)
        except Exception as e:
            print(f"Warning: OCR on plate row failed: {e}")
            return None

    # ------------------------------------------------------------ image input

    def _load_image(self, image_input: "str | bytes | np.ndarray") -> "Optional[np.ndarray]":
        """Load an image into OpenCV BGR format from any supported input type."""
        if cv2 is None or np is None:
            return None

        if isinstance(image_input, np.ndarray):
            return image_input

        if isinstance(image_input, str):
            trimmed = image_input.strip()
            # Data URI
            if trimmed.startswith("data:image/") and ";base64," in trimmed:
                _, b64_data = trimmed.split(";base64,", 1)
                try:
                    return self._bytes_to_cv2(base64.b64decode(b64_data))
                except Exception:
                    pass

            # Raw base64 string
            if (
                not trimmed.startswith("http://")
                and not trimmed.startswith("https://")
                and not os.path.exists(trimmed)
            ):
                try:
                    img_bytes = base64.b64decode(trimmed)
                    if len(img_bytes) > 50:
                        return self._bytes_to_cv2(img_bytes)
                except Exception:
                    pass

            # Local file path
            if os.path.exists(trimmed):
                return cv2.imread(trimmed)

            # Remote URL
            if trimmed.startswith("http://") or trimmed.startswith("https://"):
                try:
                    req = urllib.request.Request(
                        trimmed,
                        headers={
                            "User-Agent": "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
                            "Accept": "image/*,*/*;q=0.8",
                        },
                    )
                    with urllib.request.urlopen(req, timeout=8) as response:
                        img_bytes = response.read()
                        if len(img_bytes) > 100:
                            return self._bytes_to_cv2(img_bytes)
                except Exception:
                    pass

        if isinstance(image_input, (bytes, bytearray)):
            return self._bytes_to_cv2(image_input)

        return None

    def _bytes_to_cv2(self, img_bytes: bytes) -> "Optional[np.ndarray]":
        nparr = np.frombuffer(img_bytes, np.uint8)
        return cv2.imdecode(nparr, cv2.IMREAD_COLOR)

    # ---------------------------------------------------------------- helpers

    @staticmethod
    def _empty_result(description: str) -> Dict[str, Any]:
        return {
            "plate_number": "",
            "vehicle_type": "unknown",
            "confidence": "low",
            "confidence_score": 0.0,
            "raw_text": "",
            "description": description,
            "bbox": None,
            "detector_confidence": 0.0,
        }

    @staticmethod
    def format_indonesian_plate(raw_text: str) -> Tuple[str, bool]:
        """Format a raw string as an Indonesian plate. Kept for external callers."""
        candidate = plate_format.parse(raw_text)
        return candidate.plate, candidate.matched_format and candidate.valid_area_code

    # --------------------------------------------------------------- fallback

    def _recognize_with_tesseract(self, img: "np.ndarray") -> Dict[str, Any]:
        """Legacy Tesseract path, only reachable with ALPR_ENABLE_TESSERACT=1.

        Retained as an emergency fallback for hosts that cannot run the ONNX
        models. Its accuracy on real photos is poor -- see the module docstring.
        """
        gray = cv2.cvtColor(img, cv2.COLOR_BGR2GRAY)
        clahe = cv2.createCLAHE(clipLimit=2.5, tileGridSize=(8, 8))
        enhanced = clahe.apply(gray)
        resized = cv2.resize(enhanced, None, fx=2.5, fy=2.5, interpolation=cv2.INTER_CUBIC)

        _, otsu = cv2.threshold(resized, 0, 255, cv2.THRESH_BINARY + cv2.THRESH_OTSU)
        variants = [
            otsu,
            cv2.bitwise_not(otsu),
            resized,
            cv2.adaptiveThreshold(
                resized, 255, cv2.ADAPTIVE_THRESH_GAUSSIAN_C, cv2.THRESH_BINARY, 15, 4
            ),
        ]

        # Score every variant and keep the best, rather than returning the first
        # one whose text happens to match the (very loose) plate pattern.
        texts = [t for t in (self._run_tesseract(v) for v in variants) if t]
        winner = plate_format.best([plate_format.parse(t, 0.5) for t in texts])

        if winner is None:
            return self._empty_result("Local ALPR (Tesseract fallback) could not read a plate")

        return {
            "plate_number": winner.plate,
            "vehicle_type": "unknown",
            "confidence": winner.confidence_bucket,
            "confidence_score": round(winner.score, 4),
            "raw_text": winner.raw_text,
            "description": "Processed by Local ALPR (Tesseract fallback)",
            "bbox": None,
            "detector_confidence": 0.0,
        }

    def _run_tesseract(self, img_np: "np.ndarray") -> str:
        """Run the tesseract CLI on a processed image."""
        try:
            success, buffer = cv2.imencode(".png", img_np)
            if not success:
                return ""

            tesseract_cmd = "tesseract"
            for test_path in ("/usr/bin/tesseract", "/usr/local/bin/tesseract"):
                if os.path.exists(test_path):
                    tesseract_cmd = test_path
                    break

            proc = subprocess.run(
                [
                    tesseract_cmd, "stdin", "stdout",
                    "--oem", "1",
                    "-l", "eng",
                    "--psm", "7",
                    # The English dictionaries rewrite plate strings into words;
                    # plates are not words, so both are disabled.
                    "-c", "load_system_dawg=0",
                    "-c", "load_freq_dawg=0",
                    "-c", "tessedit_char_whitelist=ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789. ",
                ],
                input=buffer.tobytes(),
                capture_output=True,
                timeout=5,
            )
            if proc.returncode == 0:
                raw = proc.stdout.decode("utf-8", errors="ignore").strip()
                return " ".join(raw.split())
        except Exception:
            pass
        return ""


def _pad_bbox(bbox: Any, frame_width: int, frame_height: int) -> Any:
    """Grow a detected plate box slightly before cropping.

    Detector boxes sit tight against the outermost glyphs, which shaves the edge
    off the first and last characters -- the reason reads came back as "BK 5379
    WA" instead of "BK 5379 WAJ". A few percent of margin gives the OCR stage a
    whole glyph to work with.
    """
    pad_x = max(2, int(bbox.width * BBOX_PAD_RATIO))
    pad_y = max(2, int(bbox.height * BBOX_PAD_RATIO))
    return type(bbox)(
        bbox.x1 - pad_x, bbox.y1 - pad_y, bbox.x2 + pad_x, bbox.y2 + pad_y
    ).clamp(frame_width, frame_height)


def _mean_confidence(confidence: "float | list[float] | None") -> float:
    """Reduce the OCR stage's per-character confidences to a single 0..1 value."""
    if confidence is None:
        return 0.0
    if isinstance(confidence, (int, float)):
        return float(confidence)
    values = [float(c) for c in confidence]
    return sum(values) / len(values) if values else 0.0
