# ─────────────────────────────────────────────────────────────────────────────
# AI Router — build & run
#
# Targets:
#   make setup      copy .env files + install webui deps
#   make webui      build the Next.js dashboard (static export -> webui/out)
#   make build      build the static musl binary + dashboard, output dist/router-api-ai
#   make run        dev mode: Rust backend (:5790) + Next.js dev server (:3000)
#   make run-prod   run the single static binary (serves API + dashboard on one port)
#   make clean      remove build artifacts
#
# Static build requires:
#   rustup target add x86_64-unknown-linux-musl
#   sudo apt install musl-tools      (Debian/Ubuntu; provides musl-gcc for sqlite C code)
# ─────────────────────────────────────────────────────────────────────────────

TARGET  ?= x86_64-unknown-linux-musl
BINARY  := router_api_ai
OUT     := dist/router-api-ai
WEBUI   := webui
CARGO   := cargo
BUN     := bun

# Base URL inlined into the exported dashboard at build time.
# Empty (default) = relative URLs, so the single binary serves dashboard + API
# from the same origin. Set this only when deploying the dashboard separately.
NEXT_PUBLIC_ROUTER_API_URL ?=

# C toolchain for the musl target (needed by rusqlite's bundled SQLite).
# Prefer the bundled zig wrapper (self-contained), fall back to musl-gcc.
ifeq ($(shell command -v zig 2>/dev/null),)
MUSL_CC ?= musl-gcc
else
MUSL_CC ?= $(CURDIR)/scripts/zig-cc
endif

export CC_$(shell echo $(TARGET) | tr '-' '_')  := $(MUSL_CC)
export CARGO_TARGET_$(shell echo $(TARGET) | tr '-' '_')_LINKER := $(MUSL_CC)

.PHONY: help setup webui build run run-prod clean

help:
	@echo "targets: setup | webui | build | run | run-prod | clean"

## Copy env templates and install frontend dependencies (idempotent).
setup:
	cp -n .env.example .env || true
	cp -n $(WEBUI)/.env.local.example $(WEBUI)/.env.local || true
	cd $(WEBUI) && $(BUN) install

## Build the dashboard as a static export (webui/out).
webui:
	cd $(WEBUI) && NEXT_PUBLIC_ROUTER_API_URL="$(NEXT_PUBLIC_ROUTER_API_URL)" $(BUN) run build

## Build the fully static musl binary + dashboard into dist/router-api-ai.
build: webui
	rustup target add $(TARGET)
	$(CARGO) build --release --target $(TARGET)
	mkdir -p dist
	cp target/$(TARGET)/release/$(BINARY) $(OUT)
	@file $(OUT)
	@echo "→ single binary: $(OUT)  (serves API + dashboard on one port)"

## Dev mode: run both stacks side by side.
run:
	@trap 'kill 0' INT TERM EXIT; \
		(cd $(WEBUI) && $(BUN) run dev) & \
		$(CARGO) run & \
		wait

## Production mode: run the single binary (needs `make build` first).
run-prod: build
	@echo "→ http://127.0.0.1:5790  (dashboard + API on the same port)"
	./$(OUT)

clean:
	$(CARGO) clean
	rm -rf $(WEBUI)/out dist