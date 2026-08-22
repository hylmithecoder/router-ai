"""Train YOLOv11 license plate detector and export directly to ONNX format."""

import shutil
import sys
from pathlib import Path

ROOT_DIR = Path(__file__).resolve().parent.parent.parent
WEIGHTS_DIR = Path(__file__).resolve().parent.parent / "weights"
DATASET_YAML = ROOT_DIR / "datasets" / "plates" / "yolo_dataset" / "data.yaml"


def train_and_export(epochs: int = 50, batch_size: int = 16, img_size: int = 640):
    try:
        from ultralytics import YOLO
    except ImportError:
        print(
            "Ultralytics not installed. Please install with `pip install ultralytics`."
        )
        sys.exit(1)

    if not DATASET_YAML.exists():
        print(
            f"Dataset config {DATASET_YAML} not found. Running dataset_exporter first..."
        )
        from dataset_exporter import export_yolo_dataset

        export_yolo_dataset()

    print("=" * 60)
    print("Starting YOLOv11 License Plate Detector Training")
    print("=" * 60)

    # Initialize YOLOv11 nano model for fast training and lightweight ONNX export
    model = YOLO("yolo11n.pt")

    # Train model
    results = model.train(
        data=str(DATASET_YAML),
        epochs=epochs,
        batch=batch_size,
        imgsz=img_size,
        project=str(ROOT_DIR / "runs" / "plate_detector"),
        name="yolo11n_plate",
        exist_ok=True,
    )

    print("\nTraining completed! Exporting model to ONNX format...")
    # Export best model to ONNX (dynamic batch size, opset 17, simplified)
    onnx_path = model.export(format="onnx", dynamic=True, simplify=True)
    print(f"ONNX Model exported to: {onnx_path}")

    # Copy to weights directory for local engine use
    WEIGHTS_DIR.mkdir(parents=True, exist_ok=True)
    target_onnx = WEIGHTS_DIR / "plate_detector.onnx"
    shutil.copy2(onnx_path, target_onnx)
    print(f"Model successfully installed to: {target_onnx}")
    print("Local ALPR engine will now use the trained YOLOv11 ONNX model!")


if __name__ == "__main__":
    epochs = int(sys.argv[1]) if len(sys.argv) > 1 else 50
    train_and_export(epochs=epochs)
