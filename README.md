# AI Router (Rust + Next.js)

A personal AI router: an OpenAI-compatible gateway that fronts Groq (and any
OpenAI-compatible provider) with API-key auth, per-key daily token quotas,
automatic failover between providers, usage tracking in SQLite, and a Next.js
dashboard to monitor everything.

Built to power Discord automoderation later — but it is a general-purpose
OpenAI-compatible endpoint you can point any client at.

## Architecture

```
┌─────────────┐   OpenAI-compatible   ┌──────────────────────┐   fallback chain   ┌──────────────┐
│ Discord bot │ ───────────────────▶ │  Rust AI Router       │ ─────────────────▶ │  Groq key 1  │
│ / your app  │   POST /v1/chat/...  │  (axum + sqlite)      │   429/5xx/timeout │  Groq key 2  │
└─────────────┘                      │  - auth (API keys)    │ ─────────────────▶ │ local agent │
┌─────────────┐                      │  - quota enforcement  │                    │ CLIs        │
│ Next.js     │ ───────────────────▶ │  - usage logging      │                    └──────────────┘
│ dashboard   │   admin API          │  - provider failover  │
└─────────────┘                      └──────────────────────┘
```

- **Router API** (port 5790): `POST /v1/chat/completions` (JSON + SSE streaming),
  `POST /api/v1/chat` (unified Groq/local-agent routing), and `GET /v1/models` —
  protected by per-key bearer tokens with daily token quotas.
- **Admin API** (same server): usage stats, request log, key management, provider
  health — protected by a master key.
- **WebUI** (Next.js, port 3000): dashboard with quota gauge, 7-day chart,
  request log, personal key management, encrypted Groq-key management, and
  local agent binary status.

## Quick start

```bash
# 1. Configure the backend
cp .env.example .env
#    edit .env: ROUTER_MASTER_KEY=..., GROQ_API_KEYS=...

# 2. Dev mode: run the whole stack with one command
make setup          # copy env templates + install webui deps (once)
make run            # Rust backend (:5790) + Next.js dev server (:3000)

# 3. Single binary: serve the dashboard AND the API from one port
make build          # static musl binary -> dist/router-api-ai
make run-prod       # http://127.0.0.1:5790 serves dashboard + API
```

The static binary (`dist/router-api-ai`) embeds nothing at build time — it serves
the statically exported dashboard from `webui/out` on the same port the API lives
on, so one process, one port, one deployment. The dashboard uses relative API
URLs in this mode (the Makefile exports the dashboard with an empty
`NEXT_PUBLIC_ROUTER_API_URL`).

### Make targets

| Target | Description |
|---|---|
| `make setup` | copy `.env` templates + `bun install` |
| `make webui` | export the Next.js dashboard into `webui/out` |
| `make build` | `webui` + musl static release binary → `dist/router-api-ai` |
| `make run` | dev: Rust backend (:5790) + Next dev server (:3000) together |
| `make run-prod` | run the single binary |
| `make clean` | remove build artifacts |

`make build` needs a musl C toolchain. It auto-detects `zig` (recommended,
self-contained) or falls back to `musl-gcc`; the target can be overridden with
`make build TARGET=aarch64-unknown-linux-musl`.

## Try the endpoints

```bash
# Health probes
curl http://127.0.0.1:5790/health
curl http://127.0.0.1:5790/health/ready

# OpenAI-compatible chat completion (uses your API key)
curl http://127.0.0.1:5790/v1/chat/completions \
  -H "Authorization: Bearer sk-router-xxx" \
  -H "Content-Type: application/json" \
  -d '{"model":"llama-3.3-70b-versatile","messages":[{"role":"user","content":"hello"}]}'

# Streaming (SSE)
curl -N http://127.0.0.1:5790/v1/chat/completions \
  -H "Authorization: Bearer sk-router-xxx" \
  -H "Content-Type: application/json" \
  -d '{"model":"llama-3.3-70b-versatile","stream":true,"messages":[{"role":"user","content":"hello"}]}'

# Unified routing. `provider` can be `auto`, `groq`, `opencode`, `codex`,
# `claude`, `agy`, or a concrete provider id. Omitting it uses `auto` here.
curl http://127.0.0.1:5790/api/v1/chat \
  -H "Authorization: Bearer sk-router-xxx" \
  -H "Content-Type: application/json" \
  -d '{"provider":"opencode","messages":[{"role":"user","content":"hello"}]}'

# List models
curl http://127.0.0.1:5790/v1/models -H "Authorization: Bearer sk-router-xxx"

# Admin: usage summary, request log, keys, providers
curl http://127.0.0.1:5790/api/v1/admin/usage/summary -H "Authorization: Bearer $ROUTER_MASTER_KEY"
curl http://127.0.0.1:5790/api/v1/admin/usage/log?limit=50 -H "Authorization: Bearer $ROUTER_MASTER_KEY"
curl http://127.0.0.1:5790/api/v1/admin/keys -H "Authorization: Bearer $ROUTER_MASTER_KEY"
curl http://127.0.0.1:5790/api/v1/admin/providers -H "Authorization: Bearer $ROUTER_MASTER_KEY"

# Add a Groq key. It is encrypted at rest and never returned by list endpoints.
curl -X POST http://127.0.0.1:5790/api/v1/admin/providers \
  -H "Authorization: Bearer $ROUTER_MASTER_KEY" \
  -H "Content-Type: application/json" \
  -d '{"kind":"groq","name":"Groq #3","api_key":"gsk_..."}'
```

Any OpenAI SDK works — just point `base_url` at `http://127.0.0.1:5790/v1`.

## Configuration

| Variable | Default | Description |
|---|---|---|
| `ROUTER_MASTER_KEY` | *(required)* | Admin key for the dashboard and admin API |
| `ROUTER_API_KEYS` | — | Personal keys seeded at startup (comma-separated) |
| `GROQ_API_KEYS` | — | Upstream keys in fallback order. Optional per-key base URL: `key@https://.../v1` |
| `ROUTER_GROQ_BASE_URL` | `https://api.groq.com/openai/v1` | Default upstream base URL |
| `ROUTER_DEFAULT_MODEL` | `llama-3.3-70b-versatile` | Model served by the router |
| `ROUTER_DB_PATH` | `router.db` | SQLite database file |
| `ROUTER_STATIC_DIR` | `webui/out` | Directory of the exported dashboard (served on the same port) |
| `ROUTER_PROVIDER_COOLDOWN_SECS` | `60` | Cooldown after a provider failure |
| `ROUTER_DAILY_QUOTA_TOKENS` | `1000000` | Default per-key daily token quota (0 = unlimited) |
| `ROUTER_CLI_TIMEOUT_SECS` | `120` | Maximum runtime for a local agent CLI request |
| `ROUTER_AGENT_WORKDIR` | `.` | Working directory for local agent CLIs |

## How failover works

1. `/v1/chat/completions` tries Groq providers in the order listed in
   `GROQ_API_KEYS`. `/api/v1/chat` defaults to `auto`, which includes persisted
   Groq keys and locally discovered `opencode`, `codex`, `claude`, and `agy`
   providers. Set `provider` to a concrete id/kind to select one backend.
2. On `429` / `5xx` / timeout / network error, the provider enters cooldown
   (skipped for `ROUTER_PROVIDER_COOLDOWN_SECS`) and the next selected provider is tried.
3. Failures are counted in SQLite and shown on the dashboard Providers page,
   where you can also enable/disable providers without restarting.
4. If all providers fail, the request gets `503` and is logged as such.

## Project layout (backend)

```text
src/
├── main.rs / lib.rs     # entry point + module map
├── config.rs            # environment-based configuration
├── state.rs             # AppState: db + router
├── server.rs            # listener + graceful shutdown
├── middleware.rs        # API-key auth, master-key auth, quota, request-id
├── error.rs             # AppError -> JSON errors
├── routes/              # /health, /v1, /api/v1, /api/v1/admin
├── handlers/            # chat (OpenAI-compatible), admin, health
├── ai/
│   ├── dto.rs           # OpenAI-compatible request/response types
│   ├── provider.rs      # HTTP provider + safe local CLI execution
│   ├── router.rs        # concurrent fallback and selector routing
│   └── usage_stream.rs  # SSE passthrough with usage extraction
└── database/sqlite.rs   # keys / usage / encrypted provider tables
```

## Testing

```bash
cargo test                # unit + integration (providers are mocked, no real keys needed)
cd webui && bun run lint && bun run build
```

## Production checklist

- [ ] Set real values in environment (never commit `.env`)
- [ ] Restrict `CorsLayer::permissive()` to real origins
- [ ] `make build` then `make run-prod` (single static binary on one port)

## License

MIT OR Apache-2.0.
