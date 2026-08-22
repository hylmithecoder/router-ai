# Local ALPR Engine (YOLOv11 ONNX + OpenCV)

Modul ini adalah engine **Local ALPR (Automatic License Plate Recognition)** yang terintegrasi secara otomatis sebagai *fallback engine* dan *continuous evaluation platform* untuk router API.

---

## 🚀 Fitur & Komponen

1. **Auto Dataset Harvesting**:
   - Setiap request ke `POST /api/v1/ocr/licenseplate` yang diproses oleh Vision AI (Nvidia/Gemma) otomatis disimpan gambarnya ke `datasets/plates/images/` dan metadata labelnya ke SQLite `ocr_license_plate_samples`.
2. **YOLOv11 Training & Auto-ONNX Exporter**:
   - `train/dataset_exporter.py`: Mengubah dataset dari SQLite menjadi format YOLO (`data.yaml`, `images/`, `labels/`).
   - `train/train_yolo.py`: Melatih YOLOv11 nano dan otomatis meng-export model hasil training ke `weights/plate_detector.onnx`.
3. **High-Performance ONNX Engine**:
   - `engine.py`: Menggunakan OpenCV + ONNX Runtime (CPU) untuk inferensi ultra-cepat (~10-30ms) dengan normalizer plat nomor Indonesia.
4. **Automated Benchmark & Testing Harness**:
   - `benchmark.py`: Menguji ulang seluruh gambar lokal dengan engine lokal dan membandingkannya terhadap Ground Truth (Exact Match Accuracy, Character Error Rate, dan Latency).

---

## 🛠️ Cara Penggunaan

### 1. Install Dependencies
```bash
pip install -r python-alpr-local/requirements.txt
```

### 2. Jalankan Inferensi Lokal Manual
```bash
# Inferensi menggunakan path gambar
python python-alpr-local/main.py infer /path/to/plate.jpg

# Atau menggunakan input base64 stdin
echo "<base64_string>" | python python-alpr-local/main.py infer
```

### 3. Jalankan Benchmark & Evaluasi Akurasi
```bash
python python-alpr-local/benchmark.py
```

### 4. Ekspor Dataset & Latih Model Baru
```bash
# Ekspor data dari SQLite ke format YOLO
python python-alpr-local/train/dataset_exporter.py

# Mulai training YOLOv11 & otomatis export ke ONNX
python python-alpr-local/train/train_yolo.py 50
```
