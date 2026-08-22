#!/usr/bin/env python3
"""CLI Entrypoint for Local ALPR Engine.

Usage:
  python main.py infer <image_path_or_base64>
  cat image.base64 | python main.py infer
  python main.py benchmark
"""

import json
import sys
from pathlib import Path

# Add project directory to sys.path
sys.path.insert(0, str(Path(__file__).resolve().parent))

from engine import LocalALPREngine


def run_infer(image_input: str):
    engine = LocalALPREngine()
    result = engine.recognize(image_input)
    # Output pure JSON to stdout
    print(json.dumps(result, indent=2))


def main():
    if len(sys.argv) < 2:
        print(
            json.dumps(
                {
                    "error": "Usage: python main.py infer <image_path_or_base64_or_stdin>",
                    "plate_number": "",
                    "confidence": "low",
                }
            )
        )
        sys.exit(1)

    command = sys.argv[1].lower()

    if command == "infer":
        if len(sys.argv) >= 3:
            image_input = sys.argv[2]
        else:
            # Read from stdin
            image_input = sys.stdin.read().strip()

        if not image_input:
            print(
                json.dumps(
                    {
                        "error": "No image input provided",
                        "plate_number": "",
                        "confidence": "low",
                    }
                )
            )
            sys.exit(1)

        run_infer(image_input)

    elif command == "benchmark":
        from benchmark import run_benchmark

        run_benchmark()

    else:
        print(json.dumps({"error": f"Unknown command: {command}"}))
        sys.exit(1)


if __name__ == "__main__":
    main()
