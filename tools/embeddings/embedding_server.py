#!/usr/bin/env python3
"""Servidor local de embeddings (Propuesta A) — intfloat/multilingual-e5-large.

API compatible con OpenAI-ish:
    POST /v1/embeddings
    {"input": ["texto", ...], "mode": "query" | "passage"}
    -> {"data": [{"index": 0, "embedding": [f32...]}, ...]}

Prefijos obligatorios de e5: "query: " (consultas) / "passage: " (documentos).
Vectores L2-normalizados (coseno = producto punto). Solo 127.0.0.1.

Uso:  .venv/Scripts/python.exe embedding_server.py [--port 8081]
"""
from __future__ import annotations

import argparse
import json
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

try:
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
except Exception:
    pass

MODEL = None
PREFIX = {"query": "query: ", "passage": "passage: "}


def embed(texts: list[str], mode: str) -> list[list[float]]:
    """Devuelve vectores L2-normalizados para los textos, con el prefijo e5 correcto."""
    global MODEL
    if MODEL is None:
        raise RuntimeError("modelo no cargado todavía")
    pref = PREFIX.get(mode, "query: ")
    vecs = MODEL.encode([pref + t for t in texts], normalize_embeddings=True, convert_to_numpy=True)
    return [v.astype("float32").tolist() for v in vecs]


class Handler(BaseHTTPRequestHandler):
    def do_POST(self):
        try:
            length = int(self.headers.get("Content-Length", 0))
            payload = json.loads(self.rfile.read(length))
            texts = payload.get("input")
            if isinstance(texts, str):
                texts = [texts]
            if not isinstance(texts, list) or not texts:
                raise ValueError("'input' debe ser una lista de textos no vacía")
            mode = payload.get("mode", "query")
            vecs = embed(texts, mode)
            body = json.dumps(
                {"data": [{"index": i, "embedding": v} for i, v in enumerate(vecs)]},
                ensure_ascii=False,
            ).encode("utf-8")
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        except Exception as e:  # noqa: BLE001 — devolver el error al cliente
            body = json.dumps({"error": str(e)}).encode("utf-8")
            self.send_response(500)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

    def log_message(self, *args):  # silenciar logs por request
        pass


def main() -> int:
    ap = argparse.ArgumentParser(description="Servidor local de embeddings e5-large")
    ap.add_argument("--port", type=int, default=8081)
    ap.add_argument("--model", default="intfloat/multilingual-e5-large")
    args = ap.parse_args()

    from sentence_transformers import SentenceTransformer

    print(f"[embedding-server] cargando {args.model} (primera vez: descarga ~2.2 GB de HuggingFace)...", flush=True)
    global MODEL
    MODEL = SentenceTransformer(args.model)
    dims = MODEL.get_sentence_embedding_dimension()
    print(f"[embedding-server] listo en 127.0.0.1:{args.port} | dims={dims} | prefijos e5 activos", flush=True)
    ThreadingHTTPServer(("127.0.0.1", args.port), Handler).serve_forever()
    return 0


if __name__ == "__main__":
    sys.exit(main())
