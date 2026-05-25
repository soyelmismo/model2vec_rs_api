.PHONY: all check lint format test build release clean ci

all: check lint test build

# ── Code quality ──────────────────────────────────────────────────────────────

check:
	cargo check --all-targets

lint:
	cargo clippy --all-targets -- -D warnings

format:
	cargo fmt --all

format-check:
	cargo fmt --all --check

test:
	cargo test --all-targets

# ── Build ─────────────────────────────────────────────────────────────────────

build:
	cargo build

release:
	cargo build --release

# ── Housekeeping ──────────────────────────────────────────────────────────────

clean:
	cargo clean

# ── CI (runs everything) ──────────────────────────────────────────────────────

ci: format-check lint check test build
	@echo "✅ All CI checks passed"
