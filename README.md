# model2vec-api — API Reference

> Minimal, OpenAI-compatible embeddings server powered by [model2vec-rs](https://github.com/MinishLab/model2vec-rs).
>
> **No frameworks.** No hyper, axum, or tower — just raw HTTP/1.1 over Tokio.

---

## Table of Contents

- [Quick Start](#quick-start)
- [Endpoints](#endpoints)
  - [POST /v1/embeddings](#post-v1embeddings)
  - [POST /embeddings](#post-embeddings)
  - [GET /v1/models](#get-v1models)
  - [GET /models](#get-models)
  - [GET /health](#get-health)
- [Authentication](#authentication)
- [Configuration](#configuration)
- [Model Management](#model-management)
- [Error Handling](#error-handling)
- [Examples](#examples)
  - [cURL](#curl)
  - [Python (openai)](#python-openai)
  - [Python (httpx)](#python-httpx)
  - [JavaScript](#javascript)
- [Docker Deployment](#docker-deployment)
- [Local Development](#local-development)

---

## Quick Start

```bash
# Start with default model (minishlab/potion-base-8M)
docker compose up -d

# Verify it's running
curl http://localhost:22671/health

# Generate an embedding
curl -X POST http://localhost:22671/v1/embeddings \
  -H "Content-Type: application/json" \
  -d '{"model":"base","input":"Hello world"}'
```

---

## Endpoints

### POST `/v1/embeddings`

Creates an embedding vector representing the input text. Fully compatible with the [OpenAI Embeddings API](https://platform.openai.com/docs/api-reference/embeddings) schema.

**Also available at:** `POST /embeddings`

#### Request Body

| Field   | Type                 | Required | Description                                                        |
|---------|----------------------|----------|--------------------------------------------------------------------|
| `model` | `string`             | **yes**  | Model alias (as defined in the `M2V_MODELS` env var, e.g. `"base"`).  |
| `input` | `string` \| `string[]` | **yes**  | Text to embed. Accepts a single string or a batch array of strings. |

#### Example Request

```json
{
  "model": "base",
  "input": "The quick brown fox jumps over the lazy dog"
}
```

Batch request:

```json
{
  "model": "base",
  "input": [
    "First document to embed",
    "Second document to embed",
    "Third document to embed"
  ]
}
```

#### Example Response

```json
{
  "object": "list",
  "data": [
    {
      "object": "embedding",
      "embedding": [0.0123, -0.0456, 0.0789, ...],
      "index": 0
    }
  ],
  "model": "base",
  "usage": {
    "prompt_tokens": 12,
    "total_tokens": 12
  }
}
```

| Field            | Type      | Description                                               |
|------------------|-----------|-----------------------------------------------------------|
| `object`         | `string`  | Always `"list"`.                                          |
| `data`           | `array`   | Array of embedding objects, one per input string.         |
| `data[].object`  | `string`  | Always `"embedding"`.                                     |
| `data[].embedding` | `number[]` | The embedding vector (dimension depends on the model).  |
| `data[].index`   | `number`  | Index of the input in the batch (zero-based).             |
| `model`          | `string`  | The model alias used.                                     |
| `usage`          | `object`  | Token usage — counted by the model's native tokenizer. |

#### Status Codes

| Code | Description                                                      |
|------|------------------------------------------------------------------|
| 200  | Embeddings generated successfully.                               |
| 400  | Invalid request body (malformed JSON, empty input, missing field). |
| 401  | Authentication required (see [Authentication](#authentication)).  |
| 404  | Model alias not found (not in the configured `M2V_MODELS` list).     |

---

### GET `/v1/models`

Lists all loaded models and their aliases. OpenAI-compatible schema.

**Also available at:** `GET /models`

#### Example Response

```json
{
  "object": "list",
  "data": [
    {
      "id": "base",
      "object": "model",
      "owned_by": "model2vec-api"
    },
    {
      "id": "code",
      "object": "model",
      "owned_by": "model2vec-api"
    }
  ]
}
```

#### Status Codes

| Code | Description                            |
|------|----------------------------------------|
| 200  | Models listed successfully.            |
| 401  | Authentication required (if configured). |

---

### GET `/health`

Simple health-check endpoint. Does **not** require authentication.

#### Example Response

```json
{
  "status": "ok"
}
```

#### Status Codes

| Code | Description              |
|------|--------------------------|
| 200  | Service is healthy.      |

---

## Authentication

Authentication is **optional**. When enabled, all API requests (except `/health`) must include a `Bearer` token in the `Authorization` header.

### Enable Authentication

Set the `M2V_API_KEY` environment variable:

```bash
M2V_API_KEY="sk-your-secret-token" docker compose up -d
```

### Authenticated Request

```bash
curl -X POST http://localhost:22671/v1/embeddings \
  -H "Authorization: Bearer sk-your-secret-token" \
  -H "Content-Type: application/json" \
  -d '{"model":"base","input":"Hello world"}'
```

### Unauthenticated Response (401)

```json
{
  "error": {
    "message": "unauthorized",
    "type": "api_error",
    "code": 401
  }
}
```

---

## Configuration

All configuration is done through environment variables.

| Variable      | Default                        | Description                                                           |
|---------------|--------------------------------|-----------------------------------------------------------------------|
| `M2V_MODELS`      | `base:minishlab/potion-base-8M` | Comma-separated list of `alias:path` entries (see below).           |
| `M2V_LISTEN_ADDR` | `0.0.0.0:22671`               | Host and port the server binds to.                                    |
| `M2V_API_KEY`     | _(disabled)_                   | Bearer token required on all API requests. Leave unset to disable auth. |
| `M2V_HF_TOKEN`    | _(none)_                       | Hugging Face token for private or gated models.                       |
| `M2V_LOG_LEVEL`   | `info`                         | Log level: `error`, `warn`, `info`, `debug`, `trace`.                  |

### Model Configuration Syntax

```
M2V_MODELS=<alias>:<path>[,<alias>:<path>...]
```

- **`<alias>`** — a short name used in the `model` field of API requests (e.g. `"base"`, `"code"`, `"large"`).
- **`<path>`** — a Hugging Face repo ID (e.g. `minishlab/potion-base-8M`) or an absolute path to a local model directory.

#### Examples

```bash
# Single model (default)
M2V_MODELS=base:minishlab/potion-base-8M

# Multiple models from HuggingFace
M2V_MODELS=base:minishlab/potion-base-8M,large:minishlab/potion-base-32M

# HuggingFace + local model
M2V_MODELS=base:minishlab/potion-base-8M,mymodel:/models/my-local-model

# Multilingual + code-specific models
M2V_MODELS=multi:minishlab/potion-multilingual-128M,code:minishlab/potion-code-16M

# Private HuggingFace model (requires M2V_HF_TOKEN)
M2V_MODELS=private:my-org/my-private-model
```

### Using a `.env` file

The server reads a `.env` file from the current working directory at startup:

```bash
# .env
M2V_MODELS=base:minishlab/potion-base-8M,code:minishlab/potion-code-16M
M2V_API_KEY=sk-your-secret
M2V_HF_TOKEN=hf_xxxxxxxxxxxxxxxxxxxx
M2V_LOG_LEVEL=debug
```

---

## Model Management

### How Models Are Loaded

Models are loaded **at startup** based on the `M2V_MODELS` environment variable. Each model alias maps to either:

1. **A Hugging Face repo** — downloaded and cached locally on first load (e.g., `minishlab/potion-base-8M`).
2. **A local path** — must be an absolute path inside the container (e.g., `/models/my-model`).

### Model Cache

Hugging Face models are cached under `/models` inside the container. Using a Docker volume prevents re-downloading on restarts:

```yaml
volumes:
  - model-cache:/models   # named volume
  # or a bind mount:
  - /host/path/models:/models
```

### Adding Models at Runtime

Currently, models are loaded once at startup. To add or remove models, restart the container with an updated `M2V_MODELS` variable.

---

## Error Handling

All errors follow a consistent JSON schema:

```json
{
  "error": {
    "message": "description of the problem",
    "type": "api_error",
    "code": 400
  }
}
```

### Error Codes

| Code | Meaning              | Typical Cause                                           |
|------|----------------------|---------------------------------------------------------|
| 400  | Bad Request          | Invalid JSON, empty input, or malformed request body.   |
| 401  | Unauthorized         | Missing or invalid `Authorization` header.              |
| 404  | Not Found            | Unknown model alias or endpoint.                        |
| 405  | Method Not Allowed   | Using the wrong HTTP method (e.g. GET on `/v1/embeddings`). |
| 413  | Payload Too Large    | Request body exceeds 16 MiB limit.                      |
| 500  | Internal Server Error| Unexpected server-side failure.                         |

---

## Examples

### cURL

```bash
# Health check
curl http://localhost:22671/health

# List models
curl http://localhost:22671/v1/models

# Single embedding
curl -X POST http://localhost:22671/v1/embeddings \
  -H "Content-Type: application/json" \
  -d '{"model":"base","input":"Hello, world!"}'

# Batch embedding
curl -X POST http://localhost:22671/v1/embeddings \
  -H "Content-Type: application/json" \
  -d '{
    "model": "base",
    "input": [
      "First piece of text",
      "Second piece of text",
      "Third piece of text"
    ]
  }'

# With authentication
curl -X POST http://localhost:22671/v1/embeddings \
  -H "Authorization: Bearer sk-your-secret-token" \
  -H "Content-Type: application/json" \
  -d '{"model":"base","input":"Hello, world!"}'
```

### Python (openai)

The API is compatible with the official OpenAI Python client:

```python
from openai import OpenAI

client = OpenAI(
    base_url="http://localhost:22671/v1",
    api_key="sk-not-needed-if-auth-disabled",  # can be any value when auth is off
)

response = client.embeddings.create(
    model="base",
    input="Hello, world!",
)

print(response.data[0].embedding[:5])  # first 5 dimensions
```

Batch embedding:

```python
response = client.embeddings.create(
    model="base",
    input=[
        "First document",
        "Second document",
        "Third document",
    ],
)

for item in response.data:
    print(f"Index {item.index}: {len(item.embedding)} dimensions")
```

### Python (httpx)

```python
import httpx

response = httpx.post(
    "http://localhost:22671/v1/embeddings",
    json={"model": "base", "input": "Hello, world!"},
)
data = response.json()
embedding = data["data"][0]["embedding"]
print(f"Dimension: {len(embedding)}")
```

With authentication:

```python
response = httpx.post(
    "http://localhost:22671/v1/embeddings",
    headers={"Authorization": "Bearer sk-your-secret-token"},
    json={"model": "base", "input": "Hello, world!"},
)
```

### JavaScript

```javascript
const response = await fetch("http://localhost:22671/v1/embeddings", {
  method: "POST",
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify({ model: "base", input: "Hello, world!" }),
});

const data = await response.json();
console.log(data.data[0].embedding);
```

---

## Docker Deployment

### Using docker-compose (recommended)

```bash
# Default: potion-base-8M on port 22671
docker compose up -d

# Custom models and port
M2V_MODELS="base:minishlab/potion-base-8M,code:minishlab/potion-code-16M" \
PORT=8080 \
docker compose up -d
```

### Using the deploy script

```bash
# Deploy to a remote server
./deploy.sh

# Custom port
./deploy.sh --port 8080

# Custom models
./deploy.sh --models "base:minishlab/potion-base-8M,code:minishlab/potion-code-16M"

# With environment overrides
M2V_API_KEY="sk-..." M2V_HF_TOKEN="hf_..." ./deploy.sh
```

### Manual Docker

```bash
docker build -t model2vec-api .

docker run -d \
  --name model2vec-api \
  --restart unless-stopped \
  -p 22671:22671 \
  -v model-cache:/models \
  -e M2V_MODELS="base:minishlab/potion-base-8M" \
  -e M2V_API_KEY="sk-your-secret" \
  model2vec-api
```

---

## Local Development

### Prerequisites

- Rust 2024 edition toolchain
- A C compiler (for native dependency linking)

### Running from source

```bash
# Clone and build
cargo build --release

# Copy example config
cp .env.example .env
# Edit .env to set your models

# Run
./target/release/model2vec-api
```

### Docker build (multi-stage)

```dockerfile
# The Dockerfile uses a two-stage build:
#   1. rust:slim → compiles the binary
#   2. gcr.io/distroless/cc-debian13:nonroot → minimal runtime image
#
# Build with:
docker build -t model2vec-api .
```

---

## Performance Considerations

- **Request body limit:** 16 MiB (configurable at compile time via `MAX_BODY` in `src/server.rs`).
- **Concurrency:** Each connection runs in its own Tokio task — fully concurrent.
- **Keep-alive:** HTTP/1.1 persistent connections are supported and enabled by default.
- **Model dimension:** Varies by model (e.g., potion-base-8M produces 256-dimensional vectors, potion-base-32M produces 512-dimensional vectors).
- **Binary size:** Release builds are stripped and compiled with LTO for minimal size (~3–5 MB).
