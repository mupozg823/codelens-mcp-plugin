"""
Export MiniLM-L12-CodeSearchNet to ONNX INT8 for CodeLens integration.
Produces both ARM64 (Apple Silicon) and AVX2 (x86) quantized models.
"""

import os
import shutil
from pathlib import Path
from sentence_transformers import (
    SentenceTransformer,
    export_dynamic_quantized_onnx_model,
)

MODEL_ID = "isuruwijesiri/all-MiniLM-L12-v2-code-search-512"
OUTPUT_DIR = Path("./codelens-code-search")


def main():
    print("1. Loading model...")
    model = SentenceTransformer(MODEL_ID, backend="onnx")

    # Export ARM64 (Apple Silicon)
    arm64_dir = OUTPUT_DIR / "arm64"
    print(f"\n2. Exporting ONNX INT8 (ARM64) → {arm64_dir}")
    export_dynamic_quantized_onnx_model(
        model,
        quantization_config="arm64",
        model_name_or_path=str(arm64_dir),
    )

    # Export AVX2 (x86_64)
    avx2_dir = OUTPUT_DIR / "avx2"
    print(f"\n3. Exporting ONNX INT8 (AVX2) → {avx2_dir}")
    export_dynamic_quantized_onnx_model(
        model,
        quantization_config="avx2",
        model_name_or_path=str(avx2_dir),
    )

    # Report sizes
    print("\n4. Model sizes:")
    for variant in ["arm64", "avx2"]:
        d = OUTPUT_DIR / variant
        total = sum(f.stat().st_size for f in d.rglob("*") if f.is_file())
        onnx_files = list(d.rglob("*.onnx"))
        onnx_size = sum(f.stat().st_size for f in onnx_files)
        print(
            f"   {variant}: {total/1024/1024:.1f}MB total, {onnx_size/1024/1024:.1f}MB ONNX"
        )

    # Verify the exported model works
    print("\n5. Verification...")
    verify_model = SentenceTransformer(str(arm64_dir), backend="onnx")
    query_emb = verify_model.encode(["read a file and return contents"])
    code_emb = verify_model.encode(
        [
            "fn read_file(path: &str) -> String { std::fs::read_to_string(path).unwrap() }"
        ]
    )
    import numpy as np

    sim = np.dot(query_emb[0], code_emb[0]) / (
        np.linalg.norm(query_emb[0]) * np.linalg.norm(code_emb[0])
    )
    print(f"   Cosine similarity: {sim:.4f} (should be > 0.5)")
    print(f"   Status: {'PASS' if sim > 0.5 else 'FAIL'}")

    print(f"\nDone! Models exported to {OUTPUT_DIR}/")


if __name__ == "__main__":
    main()
