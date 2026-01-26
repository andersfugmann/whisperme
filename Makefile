# WhisperMe Makefile
# Note: WHISPER_DONT_GENERATE_BINDINGS=1 is set in .cargo/config.toml

# Default target
.PHONY: all
all: build

# Directories
PREFIX ?= $(HOME)/.local
BINDIR := $(PREFIX)/bin

.PHONY: build
build: submodules ## Build the project (debug)
	cargo build

.PHONY: release
release: submodules ## Build the project (release)
	cargo build --release

.PHONY: build-release
build-release: build release ## Build both debug and release

.PHONY: test
test: ## Run all tests
	cargo test
	@echo ""
	@echo "To run visual UI test (opens a window):"
	@echo "  cargo test --test ui_integration test_ui_window_with_audio -- --ignored"

.PHONY: run
run: build ## Run the daemon
	cargo run --bin whispermed

.PHONY: transcribe-start
transcribe-start: build ## Start Transcription
	cargo run --bin whisperme start

.PHONY: transcribe-stop
transcribe-stop: build ## Stop Transcription
	cargo run --bin whisperme stop

.PHONY: config
config: build ## Open configuration dialog
	cargo run --bin whisperme config

vendor/whisper-rs/sys/whisper.cpp/.git:
	@echo "Initializing submodules..."
	git submodule update --init --recursive

.PHONY: submodules
submodules: | vendor/whisper-rs/sys/whisper.cpp/.git

.PHONY: install
install: release ## Install to ~/.local/bin
	@mkdir -p $(BINDIR)
	cp target/release/whispermed $(BINDIR)/
	cp target/release/whisperme $(BINDIR)/
	@echo "Installed to $(BINDIR)"
	@echo ""
	@echo "Make sure $(BINDIR) is in your PATH:"
	@echo "  export PATH=\"$(BINDIR):\$$PATH\""

.PHONY: clean
clean: ## Clean build artifacts
	cargo clean
	rm -rf debian/.debhelper debian/cargo debian/source debian/whisperme
	rm -f debian/debhelper-build-stamp debian/files

.PHONY: deb
deb: submodules ## Build Debian package
	dpkg-buildpackage -us -uc -b
	@ls -la ../whisperme_*.deb 2>/dev/null

.PHONY: help
help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[1;32m%-15s\033[0m %s\n", $$1, $$2}'
