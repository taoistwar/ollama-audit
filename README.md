# ollama-audit

A small Rust reverse proxy for [Ollama](https://ollama.com/) HTTP APIs. It forwards **POST** requests unchanged and, when configured, sends traces to [Langfuse](https://langfuse.com/) via the public [ingestion API](https://langfuse.com/docs/api) (`POST /api/public/ingestion`).

[中文版说明](README_zh.md)

## Features

- Transparent proxy: same path and query as the client, body and headers forwarded to Ollama.
- **Streaming** responses: chunks are streamed to the client while the full stream is buffered to build a best-effort `output` for Langfuse after the stream ends.
- **Non-streaming** responses: full body is read, then a Langfuse `generation-update` is sent.
- Langfuse is **optional**: if public/secret keys are not both set, the proxy still runs and only skips ingestion.
- **Rolling file logs** via [`tracing-appender`](https://crates.io/crates/tracing-appender): files under `LOG_DIR` with prefix `ollama-proxy`, rotated daily by default; stdout mirror unless disabled.

## Requirements

- Rust toolchain (edition 2024 as specified in `Cargo.toml`).

## Configuration

Environment variables are read from the process environment. If a `.env` file is present in the working directory, it is loaded first via [dotenvy](https://crates.io/crates/dotenvy) (missing file is ignored).

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `OLLAMA_URL` | No | `http://127.0.0.1:11434` | Ollama base URL (no API path here; client paths are appended). A trailing slash is optional and normalized away. |
| `BIND_ADDR` | No | `127.0.0.1:5000` | Address the proxy listens on (`host:port`). |
| `LANGFUSE_PUBLIC_KEY` | For Langfuse | — | Langfuse API public key (HTTP Basic username). |
| `LANGFUSE_SECRET_KEY` | For Langfuse | — | Langfuse API secret key (HTTP Basic password). |
| `LANGFUSE_BASE_URL` | No | `https://cloud.langfuse.com` | Langfuse host (e.g. self-hosted `http://localhost:3000`). |

Langfuse is enabled only when **both** `LANGFUSE_PUBLIC_KEY` and `LANGFUSE_SECRET_KEY` are non-empty.

| `AUDIT_LOG_ALWAYS` | No | (off) | If `1`, `true`, `yes`, or `on` (case-insensitive), emit local audit logs (`target: ollama_audit`) for every request and response even when Langfuse succeeds. |
| `LOG_DIR` | No | `logs` | Directory for rolling log files (created if missing). |
| `LOG_ROTATION` | No | `daily` | `daily`, `hourly` / `hour`, or `minutely` / `minute`. |
| `LOG_DISABLE_STDOUT` | No | (off) | If `true` / `1` / `yes` / `on`, only write logs to files (no console). |
| `RUST_LOG` | No | `info` | [`tracing` filter](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html), e.g. `info,ollama_audit=debug`. |

**Security:** Do not commit real API keys. Keep `.env` out of version control (see `.gitignore`). The default `logs/` directory is listed in `.gitignore`.

## Run

```bash
cargo run
```

Release build:

```bash
cargo build --release
./target/release/ollama-audit
```

## Point clients at the proxy

Configure your Ollama client or SDK to use the proxy base URL (the value of `BIND_ADDR` as `http://host:port`) instead of Ollama directly. Example with `curl`:

```bash
curl http://127.0.0.1:5000/api/chat -d '{"model":"llama3","messages":[{"role":"user","content":"hi"}],"stream":false}'
```

Only **POST** is registered on the router; other methods are not proxied.

## Langfuse events

For each proxied POST:

1. **`trace-create`** — trace name like `ollama /api/chat`, metadata includes `path` and `source: ollama-audit`, `input` is the request JSON (or a string if not valid JSON).
2. **`generation-create`** — linked to the trace, `model` from the request body when present, `input` same as above.
3. **`generation-update`** — after the Ollama response is complete, `output` is derived from JSON fields such as `message.content` or `response`, or from newline-delimited JSON chunks when `stream: true`.

Ingestion runs in background tasks; failures are logged at `warn` and do not change the HTTP response to the client.

## License

If you add a license file, describe it here.
