#!/usr/bin/env python3
"""Precomputa vectores e5-large de data/corpus.json -> data/embeddings.bin.

Formato del binario (compatible con bincode en Rust):
    u64 LE  : número de vectores
    por vector: u64 LE (dim) + dim × f32 LE

Los vectores se alinean por índice con corpus.children (mismo orden).
Prefijo "passage: " (e5) + truncado a 2000 chars (~512 tokens) por chunk.
"""
from __future__ import annotations

import json
import struct
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]  # raíz del proyecto (tools/embeddings -> tools -> raíz)
sys.path.insert(0, str(ROOT))

MAX_CHARS = 2000


def main() -> int:
    corpus_path = ROOT / "data" / "corpus.json"
    out_path = ROOT / "data" / "embeddings.bin"

    corpus = json.load(open(corpus_path, encoding="utf-8"))
    children = corpus["children"]
    print(f"[compute-embeddings] {len(children)} children desde {corpus_path.name}", flush=True)

    from sentence_transformers import SentenceTransformer

    model = SentenceTransformer("intfloat/multilingual-e5-large")
    texts = [("passage: " + c["content"][:MAX_CHARS]) for c in children]
    print(f"[compute-embeddings] embebiendo {len(texts)} chunks (puede tardar minutos)...", flush=True)
    vecs = model.encode(texts, normalize_embeddings=True, convert_to_numpy=True)

    with open(out_path, "wb") as fh:
        fh.write(struct.pack("<Q", len(vecs)))
        for v in vecs:
            fv = v.astype("float32")
            fh.write(struct.pack("<Q", len(fv)))
            fh.write(struct.pack(f"<{len(fv)}f", *fv.tolist()))

    size_mb = out_path.stat().st_size / 1e6
    print(f"[compute-embeddings] OK: {len(vecs)} vectores x {len(fv)} dims -> {out_path.name} ({size_mb:.1f} MB)", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
