# 🦀 RAG Portfolio Financiero & Estadístico (Rust Core)

Motor de **Retrieval-Augmented Generation (RAG) de Alta Fidelidad** desarrollado en **Rust puro**, diseñado para análisis cuantitativo y cualitativo de reportes financieros (memorias anuales, balances contables auditados) y reportes macroeconómicos.

Empaquetado en un **único binario ejecutable (`rag_core` de ~8.2 MB)** con cero dependencias de Python ni entornos virtuales en tiempo de ejecución. Integra búsqueda híbrida en memoria (< 30 ms) y un **Guardrail Numérico Determinista con Autocorrección** (< 1 ms) que garantiza **0 alucinaciones de cifras validadas**.

> **Nota histórica:** La versión inicial del prototipo fue desarrollada en Python (LangChain/ChromaDB), disponible como referencia en el historial del repositorio. El motor actual fue reescrito 100% en Rust nativo para máximo rendimiento y empaquetado standalone.

---

## 🚀 Características Principales

* ⚡ **Rendimiento Nativo en Rust:** Ingesta de corpus en milisegundos y retrieval híbrido en **2 a 30 ms**.
* 🛡️ **Guardrail Numérico v3:** Validación dual (números crudos + frases monetarias con multiplicadores), corrección *grounded* con fuente vía índice invertido, **corte de rebote** (no quema reintentos con el mismo set de alucinaciones), **sanitización por frase** y **atribución por indicador** (emisor × indicador, flag `--strict-attribution`).
* 🔍 **Búsqueda Híbrida RRF (Tantivy BM25 + TF-IDF):** Fusión por *Reciprocal Rank Fusion* con **multi-query por entidad**: si el query menciona varios emisores (ej. "Banco Mundial, Ferreycorp y Financiera Efectiva"), se segmenta y ejecuta una sub-consulta por emisor garantizando representación de cada tema en el top-k.
* 🔤 **Normalización de acentos:** "dolares" ≡ "dólares" en el índice (campo `content_idx` normalizado; el contenido original se conserva para citas).
* 📑 **Sanitizador Geométrico de Tablas:** Separa encabezados de categorías contables en mayúsculas (`SOLVENCIA`, `CALIDAD DE ACTIVOS`) para evitar deformaciones en la lectura del LLM.
* 🤖 **Modelos Locales (GGUF en CPU):** Optimizado para la familia Qwen (`qwen2.5-3b-instruct`, `Qwen3.8-2B`, `Qwen3.8-4B`) vía `llama-server` o cualquier endpoint compatible OpenAI **sin credenciales** (solo local).
* 📦 **Portabilidad Total:** Un solo binario compilado en modo Release sin requerir Python, Node.js ni permisos de administrador.
* 📊 **Telemetría JSON:** `--json` emite `attempt_log`, alucinaciones, correcciones, hallazgos de atribución y latencias para auditoría.

---

## 🏗️ Arquitectura del Pipeline en Rust

```
 ┌────────────────────────────────────────────────────────────────────────────────────────────────┐
 │                                    BINARIO: `rag_core` (~8.2 MB)                              │
 │                                                                                                │
 │   ┌──────────────────────┐      ┌────────────────────────┐      ┌───────────────────────────┐  │
 │   │  1. Ingesta & Parser │      │  2. Búsqueda Híbrida   │      │  3. Guardrail Numérico    │  │
 │   │  • lopdf / Markdown  │      │  • Tantivy (BM25)      │      │  • Inverted Number Index  │  │
 │   │  • Chunking Paginado │      │  • TF-IDF + RRF        │      │  • Self-Correction Loop   │  │
 │   │  • Table Sanitizer   │      │  • Multi-query/emisor  │      │  • Atribución por indicador│  │
 │   └──────────┬───────────┘      └───────────┬────────────┘      └─────────────┬─────────────┘  │
 │              │                              │                                 │                │
 │              ▼                              ▼                                 ▼                │
 │       data/corpus.bin               data/tantivy_index/              llama-server (Puerto 8080)│
 └────────────────────────────────────────────────────────────────────────────────────────────────┘
```

---

## 📂 Estructura del Repositorio

```
rag-portfolio-financiero/
├── Cargo.toml                  # Manifiesto de dependencias Rust
├── Cargo.lock                  # Árbol de dependencias bloqueadas
├── src/                        # Código fuente del motor en Rust
│   ├── main.rs                 # CLI (query, ingest, benchmark) y flags
│   ├── lib.rs                  # Módulos exportados de la biblioteca
│   ├── ingest.rs               # Parser lopdf, chunks parent/child, sanitizador de tablas
│   ├── retriever.rs            # BM25 Tantivy + TF-IDF + RRF + multi-query por entidad
│   ├── guardrail.rs            # Validación numérica, autocorrección, atribución
│   ├── numeric.rs              # Extracción de frases monetarias, porcentajes, años
│   ├── indicators.rs           # Diccionario de indicadores y expansión de consultas
│   └── llm.rs                  # Cliente HTTP OpenAI-compatible local
├── tests/                      # Pruebas de integración (49 tests en total)
│   └── integration_test.rs
├── tools/embeddings/           # (Opcional) servidor e5-large + precomputo de vectores
│   ├── embedding_server.py
│   └── compute_embeddings.py
├── data/                       # Corpus documental (PDFs públicos + markdown paginado)
│   ├── pdfs/
│   ├── markdown/
│   └── document_metadata.json
├── LICENSE                     # Licencia MIT
└── README.md                   # Documentación técnica
```

---

## 🛠️ Compilación y Pruebas

### Requisitos
* **Rust & Cargo** (edition 2021; versión 1.75 o superior).

### Compilar
```bash
cargo build --release
```
> En Windows con la toolchain MinGW (WinLibs), el proyecto local usa `run_cargo.bat` (no versionado: contiene la ruta de tu instalación) como atajo: `run_cargo.bat build --release`.

### Tests
```bash
cargo test
```
Ejecuta **49 tests** (38 unit + 11 integración): normalización numérica, RRF, Tantivy, multi-query, guardrail v3 (incluida la detección del eco del prompt de reintento) y chunking, en ~0.1 s.

---

## 💻 Uso del CLI (`rag_core`)

### 1. Ingesta y generación de índices
```bash
target/release/rag_core ingest --force
```
Reconstruye `data/corpus.bin`, `data/corpus.json` y el índice Tantivy desde `data/markdown/`.

### 2. Consulta en lenguaje natural (requiere llama-server en :8080)
```bash
# Consulta general
target/release/rag_core query "En la Memoria Anual 2025 de Financiera Efectiva, ¿cuáles fueron el ROE, el ROA y el ratio de capital global?"

# Filtrando por emisor
target/release/rag_core query "Según la Memoria Anual 2025 de Ferreycorp, ¿a cuánto ascendieron las ventas totales?" -i Ferreycorp

# Telemetría JSON (auditoría: intentos, correcciones, atribución, latencias)
target/release/rag_core query "¿Cuál fue el PBI proyectado por el Banco Mundial para Perú en 2024?" --json

# Atribución estricta: una cifra de un segmento reportada como total invalida la respuesta
target/release/rag_core query "¿Cuáles fueron las ventas totales de Ferreycorp en dólares?" --strict-attribution
```

### 3. Benchmark interno (velocidad de retrieval y estrés del guardrail)
```bash
target/release/rag_core benchmark
```

### 4. (Opcional) Tercer ranker semántico con e5-large
El motor funciona con BM25 + TF-IDF sin configuración adicional. Para activar el componente semántico opcional:
```bash
# Precomputar vectores de los chunks (una vez, ~15-30 min en CPU)
python tools/embeddings/compute_embeddings.py

# Servir embeddings localmente (modelo descargado de HuggingFace la primera vez)
python tools/embeddings/embedding_server.py --port 8081

# Usar el tercer ranker
target/release/rag_core query "..." --embeddings-url http://127.0.0.1:8081/v1/embeddings
```

---

## 📊 Evaluación de Modelos (GGUF en CPU i7-12700, benchmark en vivo)

Precisión = promedio de datos correctos por respuesta (benchmark 2026-08, 12 runs por modelo, ground truth de 3 preguntas: banca retail, cross-corpus y atribución). El guardrail validó el 100% de las cifras entregadas; los reintentos con alucinación se cortan por rebote.

| Modelo | Tamaño | Velocidad | Precisión promedio | Estabilidad |
|---|---|---|---|---|
| **`qwen2.5-3b-instruct-q4_k_m`** | 1.96 GB | 13.5 t/s | ~67% | ✅ sin timeouts |
| **`Qwen3.8-4B-Q4_K_M`** | 2.59 GB | 9.3 t/s | ~69% | ⚠️ 1 timeout en 12 runs |
| **`Qwen3.8-2B-Q4_K_M`** | 1.22 GB | 19.5 t/s | ~54% | ✅ sin timeouts |

**Recomendado para probar:** `qwen2.5-3b-instruct-q4_k_m` — mejor balance velocidad/precisión y el modelo por defecto del motor.

---

## 📄 Licencia

Distribuido bajo la Licencia MIT. Consulte [LICENSE](LICENSE).
