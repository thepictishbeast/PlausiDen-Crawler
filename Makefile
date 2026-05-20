# PlausiDen-Crawler Makefile — discovery + common operations.

.PHONY: help
help: ## Show this help.
	@printf '\n\033[1mPlausiDen-Crawler — Makefile help\033[0m\n\n'
	@printf 'For the full surface see:\n'
	@printf '  AGENTS.md           — orientation for AI agents (read first)\n'
	@printf '  TOOLS.md            — canonical crawler command index\n'
	@printf '  crawler --help      — live CLI surface\n\n'
	@printf 'Common operations:\n\n'
	@awk 'BEGIN {FS = ":.*?## "} /^[a-zA-Z0-9_.-]+:.*?## / {printf "  \033[36m%-22s\033[0m %s\n", $$1, $$2}' $(MAKEFILE_LIST)
	@printf '\n'

# ----------------------------------------------------------------
# Build + test
# ----------------------------------------------------------------

.PHONY: build
build: ## Build the entire workspace (debug profile).
	cargo build --workspace

.PHONY: release
release: ## Build the workspace, release profile.
	cargo build --workspace --release

.PHONY: test
test: ## Run every workspace test.
	cargo test --workspace

.PHONY: clippy
clippy: ## Run clippy across the workspace.
	cargo clippy --workspace --all-targets -- -D warnings

.PHONY: fmt
fmt: ## Format the workspace (rustfmt).
	cargo fmt --all

.PHONY: fmt-check
fmt-check: ## Verify formatting (CI use).
	cargo fmt --all -- --check

# ----------------------------------------------------------------
# Journey runners
# ----------------------------------------------------------------

CRAWLER := ./target/release/crawler

.PHONY: crawler-cli
crawler-cli: ## Build the release crawler binary.
	cargo build --release -p crawler-runner

.PHONY: loom-edit-smoke
loom-edit-smoke: crawler-cli ## Run the loom edit serve smoke journey.
	$(CRAWLER) --journey journeys/loom-edit-server.json --headless

.PHONY: forge-build-smoke
forge-build-smoke: crawler-cli ## Run the Forge build smoke journey.
	$(CRAWLER) --journey journeys/forge-skillshots-build.json --headless

.PHONY: lfi-landing-smoke
lfi-landing-smoke: crawler-cli ## Run the LFI landing smoke journey.
	$(CRAWLER) --journey journeys/lfi-landing-smoke.json --headless

# ----------------------------------------------------------------
# Maintenance
# ----------------------------------------------------------------

.PHONY: clean
clean: ## Remove cargo build artifacts.
	cargo clean

.PHONY: docs
docs: ## Generate workspace rustdoc.
	cargo doc --workspace --no-deps

.PHONY: ci
ci: fmt-check clippy test ## CI gate set locally.

# ----------------------------------------------------------------
# Zombie cleanup (task #182)
# ----------------------------------------------------------------

.PHONY: kill-chromium-zombies
kill-chromium-zombies: ## Workaround for #182: kill stuck chromium-shell processes.
	pkill -9 chromium-shell || true
	pkill -9 chrome_crashpad_handler || true
