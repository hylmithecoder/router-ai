"""Indonesian license plate normalization, confusion correction, and scoring.

OCR models confuse glyph pairs that look alike (0/O, 1/I, 8/B, 5/S, 2/Z). An
unconstrained reader has no way to pick the right one, but an Indonesian plate
has a rigid shape that resolves nearly all of them:

    [1-2 letters: area code] [1-4 digits: number] [0-3 letters: suffix]
    e.g.  B 1234 ABC    DK 8888 ZZ    D 1999 EF

So group 1 and group 3 can only be letters and group 2 can only be digits, and
the area code must appear in the official list. This module applies both, then
scores candidates so the caller can pick the best read instead of the first one
that happens to match.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from typing import Iterable, Optional

# Official Indonesian vehicle registration area codes (kode wilayah).
AREA_CODES = frozenset(
    """
    A AA AB AD AE AG BA BB BD BE BG BH BK BL BM BN BP
    B D E F G H K L M N P R S T W Z
    DA DB DC DD DE DG DH DK DL DM DN DP DR DS DT DU DW
    EA EB ED
    KB KH KT KU
    PA PB
    """.split()
)

# Glyph confusions, resolved by which group the character sits in.
_TO_LETTER = {"0": "O", "1": "I", "2": "Z", "4": "A", "5": "S", "6": "G", "8": "B"}
_TO_DIGIT = {"O": "0", "Q": "0", "D": "0", "I": "1", "L": "1", "Z": "2", "A": "4",
             "S": "5", "G": "6", "T": "7", "B": "8"}

# Validity sticker on the bottom row of the plate, e.g. "05.28" or "05 28".
_EXPIRY_RE = re.compile(r"^\d{2}\s*[.\-]?\s*\d{2}$")

_PLATE_RE = re.compile(r"([A-Z]{1,2})\s*([0-9]{1,4})\s*([A-Z]{0,3})")
# Same shape, but tolerating glyphs that OCR may have put in the wrong class.
_LOOSE_RE = re.compile(r"([A-Z0-9]{1,2})\s*([A-Z0-9]{1,4})\s*([A-Z0-9]{0,3})")


@dataclass
class PlateCandidate:
    """One normalized reading of a plate, with the evidence behind it."""

    plate: str
    """Formatted plate, e.g. "B 1234 ABC". Empty when nothing plate-like was found."""

    raw_text: str
    """The OCR text this candidate was derived from."""

    ocr_confidence: float
    """Mean per-character confidence reported by the OCR model, 0..1."""

    matched_format: bool
    """Whether the text parsed into the area/number/suffix shape at all."""

    valid_area_code: bool
    """Whether the area code is a real Indonesian region code."""

    corrections: int = 0
    """How many characters had to be swapped to force the shape. More = shakier."""

    @property
    def score(self) -> float:
        """Combined 0..1 confidence used to rank candidates against each other.

        Measured OCR confidence is the base; structural agreement multiplies it.
        A read that does not look like a plate at all is heavily penalized, and
        each forced character swap costs a little.
        """
        if not self.plate:
            return 0.0
        score = max(self.ocr_confidence, 0.0)
        score *= 1.0 if self.matched_format else 0.35
        score *= 1.0 if self.valid_area_code else 0.6
        score *= max(0.5, 1.0 - 0.08 * self.corrections)
        return min(score, 1.0)

    @property
    def confidence_bucket(self) -> str:
        """Coarse label kept for the existing API/UI contract."""
        s = self.score
        if s >= 0.75:
            return "high"
        if s >= 0.45:
            return "medium"
        return "low"


def is_expiry_line(text: str) -> bool:
    """True for the month.year validity row, which must not join the plate number."""
    return bool(_EXPIRY_RE.match(text.strip()))


def _clean(text: str) -> str:
    return " ".join(re.sub(r"[^A-Z0-9\s]", " ", text.upper()).split())


def _coerce(group: str, mapping: dict[str, str]) -> tuple[str, int]:
    """Force every character in a group into its allowed class, counting swaps."""
    out = []
    swaps = 0
    for ch in group:
        if ch in mapping:
            out.append(mapping[ch])
            swaps += 1
        else:
            out.append(ch)
    return "".join(out), swaps


def parse(raw_text: str, ocr_confidence: float = 0.0) -> PlateCandidate:
    """Normalize one OCR reading into a scored plate candidate.

    Tries the strict shape first; if that fails, retries allowing digits and
    letters to be swapped into their correct class, which is what recovers reads
    like "8 1Z34 A8C" -> "B 1234 ABC".
    """
    cleaned = _clean(raw_text)
    if not cleaned:
        return PlateCandidate("", raw_text, ocr_confidence, False, False)

    match = _PLATE_RE.search(cleaned)
    corrections = 0

    if match:
        area, number, suffix = match.group(1), match.group(2), match.group(3)
    else:
        loose = _LOOSE_RE.search(cleaned)
        if not loose:
            return PlateCandidate("", raw_text, ocr_confidence, False, False)
        area, a_swaps = _coerce(loose.group(1), _TO_LETTER)
        number, n_swaps = _coerce(loose.group(2), _TO_DIGIT)
        suffix, s_swaps = _coerce(loose.group(3), _TO_LETTER)
        corrections = a_swaps + n_swaps + s_swaps

    # A bare number with no suffix is legal (older plates), but a lone letter is not.
    if not number:
        return PlateCandidate("", raw_text, ocr_confidence, False, False)

    plate = f"{area} {number} {suffix}".strip()
    return PlateCandidate(
        plate=plate,
        raw_text=raw_text,
        ocr_confidence=ocr_confidence,
        matched_format=match is not None,
        valid_area_code=area in AREA_CODES,
        corrections=corrections,
    )


def best(candidates: Iterable[PlateCandidate]) -> Optional[PlateCandidate]:
    """Pick the highest-scoring candidate, or None when none produced a plate."""
    scored = [c for c in candidates if c.plate]
    if not scored:
        return None
    return max(scored, key=lambda c: c.score)


def join_rows(rows: Iterable[str]) -> str:
    """Merge the text rows of a plate crop into one string.

    Motorcycle plates in Indonesia are two rows (area code on top, number and
    suffix below) plus a validity row. The validity row is dropped so its digits
    cannot be mistaken for the plate number.
    """
    return " ".join(r.strip() for r in rows if r.strip() and not is_expiry_line(r))
