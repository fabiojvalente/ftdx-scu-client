# SCU-LAN10 Client — common developer tasks.
#
# The native desktop app and the browser (WebAssembly) app share one codebase;
# these targets wrap the cargo and wasm-bindgen invocations.

SHELL := /bin/bash
CARGO ?= cargo
APP := scu-app
WEB_DIR := app/dist
PORT ?= 8080

.DEFAULT_GOAL := help

.PHONY: help native run bridge rigctld web web-trunk serve test fmt lint clean

help: ## Show this help
	@printf "SCU-LAN10 Client targets:\n\n"
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) \
		| awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-12s\033[0m %s\n", $$1, $$2}'

native: ## Build the native desktop app (release)
	$(CARGO) build --release -p $(APP)

run: ## Build and run the native desktop app
	$(CARGO) run --release -p $(APP)

bridge: ## Run the WebSocket/UDP bridge needed by the web app
	$(CARGO) run --release -p scu-bridge -- --listen 0.0.0.0:9000

rigctld: ## Run the headless rigctld server (needs --host/--user/--pass)
	$(CARGO) run --release -p scu-rigctld -- $(ARGS)

web: ## Build the browser (WebAssembly) bundle into app/dist
	./scripts/build-web.sh

web-trunk: ## Build the browser bundle with Trunk instead of the script
	cd app && trunk build --release

serve: web ## Build the web bundle and serve it locally on port 8080
	python3 -m http.server $(PORT) --directory $(WEB_DIR)

test: ## Run the workspace test suite
	$(CARGO) test --workspace

fmt: ## Format the workspace
	$(CARGO) fmt --all

lint: ## Run clippy with warnings denied
	$(CARGO) clippy --workspace --all-targets -- -D warnings

clean: ## Remove build artifacts and the web bundle
	$(CARGO) clean
	rm -rf $(WEB_DIR)
