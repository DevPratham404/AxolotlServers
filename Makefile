# Cluster Console — developer tasks
# Requires Rust 1.85+ (stable) and Node 20+.

SHELL := /bin/bash
WEB := frontend
BACKEND := backend

.PHONY: help dev web-install web-dev web-build api daemon fmt lint test build check mcp-install mcp-test mcp

help:
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-14s\033[0m %s\n", $$1, $$2}'

dev: ## Boot daemon + API + Vite together (start-dev.sh)
	./start-dev.sh

web-install: ## Install frontend dependencies
	cd $(WEB) && npm install

web-dev: ## Run the Vite dev server (proxies /api and /ws to :8080)
	cd $(WEB) && npm run dev

web-build: ## Type-check and build the frontend
	cd $(WEB) && npm run build

api: ## Run the control-plane API
	cd $(BACKEND) && cargo run -p cluster-api

daemon: ## Run the node daemon
	cd $(BACKEND) && cargo run -p cluster-daemon

fmt: ## Format all Rust sources
	cd $(BACKEND) && cargo fmt --all

lint: ## Clippy, warnings as errors
	cd $(BACKEND) && cargo clippy --workspace --all-targets -- -D warnings

test: ## Run Rust tests
	cd $(BACKEND) && cargo test --workspace

build: ## Build both Rust binaries (release)
	cd $(BACKEND) && cargo build --release --workspace

check: ## Type-check the whole workspace
	cd $(BACKEND) && cargo check --workspace --all-targets

mcp-install: ## Install MCP server dependencies
	cd mcp && npm install

mcp-test: ## Run MCP server tests
	cd mcp && npm test

mcp: ## Run the MCP server over stdio (requires CLUSTER_API_TOKEN)
	cd mcp && npm start
