# WhisperMe Makefile
#
# Targets:
#   make deps          - Install system dependencies and download model
#   make build         - Build the project (debug)
#   make release       - Build the project (release)
#   make build-release - Build both debug and release
#   make test          - Run all tests
#   make run           - Run the daemon
#   make clean         - Clean build artifacts
#   make install       - Install to ~/.local/bin
#
# Note: WHISPER_DONT_GENERATE_BINDINGS=1 is set in .cargo/config.toml

.PHONY: all build release build-release test run clean install deps deps-system deps-model deps-submodules help

# Default target
all: build

# Directories
PREFIX ?= $(HOME)/.local
BINDIR := $(PREFIX)/bin
MODELDIR := $(HOME)/.local/share/whisperme/models
CONFIGDIR := $(HOME)/.config/whisperme

# Model configuration
MODEL ?= base
MODEL_FILE := ggml-$(MODEL).bin
MODEL_URL := https://huggingface.co/ggerganov/whisper.cpp/resolve/main/$(MODEL_FILE)

# Marker files for dependency tracking
DEPS_MARKER := .deps-installed
MODEL_MARKER := $(MODELDIR)/$(MODEL_FILE)
CONFIG_MARKER := $(CONFIGDIR)/config.ini
SUBMODULE_MARKER := vendor/whisper-rs/sys/whisper.cpp/.git

#------------------------------------------------------------------------------
# Build targets
#------------------------------------------------------------------------------

build: $(DEPS_MARKER) $(SUBMODULE_MARKER)
	cargo build

release: $(DEPS_MARKER) $(SUBMODULE_MARKER)
	cargo build --release

build-release: build release

#------------------------------------------------------------------------------
# Test target
#------------------------------------------------------------------------------

test: $(DEPS_MARKER)
	cargo test
	@echo ""
	@echo "To run visual UI test (opens a window):"
	@echo "  cargo test --test ui_integration test_ui_window_with_audio -- --ignored"

#------------------------------------------------------------------------------
# Run target
#------------------------------------------------------------------------------

run: build $(MODEL_MARKER) $(CONFIG_MARKER)
	cargo run --bin whispermed

transcribe-start: build $(MODEL_MARKER) $(CONFIG_MARKER)
	cargo run --bin whisperme start

transcribe-stop: build $(MODEL_MARKER) $(CONFIG_MARKER)
	cargo run --bin whisperme stop

config: build $(MODEL_MARKER) $(CONFIG_MARKER)
	cargo run --bin whisperme config


#------------------------------------------------------------------------------
# Dependency targets
#------------------------------------------------------------------------------

deps: deps-system deps-submodules deps-model $(CONFIG_MARKER)
	@touch $(DEPS_MARKER)
	@echo "All dependencies installed"

deps-submodules: $(SUBMODULE_MARKER)

$(SUBMODULE_MARKER):
	@echo "Initializing submodules..."
	git submodule update --init --recursive

deps-system:
	@echo "Checking system dependencies..."
	@command -v xdotool >/dev/null 2>&1 || { \
		echo "xdotool not found. Install with:"; \
		echo "  Debian/Ubuntu: sudo apt install xdotool"; \
		echo "  Arch: sudo pacman -S xdotool"; \
		echo "  Fedora: sudo dnf install xdotool"; \
		exit 1; \
	}
	@command -v cargo >/dev/null 2>&1 || { \
		echo "Rust/Cargo not found. Install with:"; \
		echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"; \
		exit 1; \
	}
	@echo "System dependencies OK"

deps-model: $(MODEL_MARKER)

$(MODEL_MARKER):
	@echo "Downloading Whisper model: $(MODEL_FILE)..."
	@mkdir -p $(MODELDIR)
	curl -L -o $(MODEL_MARKER) $(MODEL_URL)
	@echo "Model downloaded to $(MODEL_MARKER)"

$(CONFIG_MARKER):
	@echo "Creating default config..."
	@mkdir -p $(CONFIGDIR)
	@echo "[whisper]" > $(CONFIG_MARKER)
	@echo "model = $(MODEL)" >> $(CONFIG_MARKER)
	@echo "language = auto" >> $(CONFIG_MARKER)
	@echo "" >> $(CONFIG_MARKER)
	@echo "[ui]" >> $(CONFIG_MARKER)
	@echo "enabled = true" >> $(CONFIG_MARKER)
	@echo "position = bottom-right" >> $(CONFIG_MARKER)
	@echo "Config created at $(CONFIG_MARKER)"

$(DEPS_MARKER): deps-system
	@touch $(DEPS_MARKER)

#------------------------------------------------------------------------------
# Install target
#------------------------------------------------------------------------------

install: release $(MODEL_MARKER) $(CONFIG_MARKER)
	@mkdir -p $(BINDIR)
	cp target/release/whispermed $(BINDIR)/
	cp target/release/whisperme $(BINDIR)/
	cp target/release/whisperme-config $(BINDIR)/
	@echo "Installed to $(BINDIR)"
	@echo ""
	@echo "Make sure $(BINDIR) is in your PATH:"
	@echo "  export PATH=\"$(BINDIR):\$$PATH\""

#------------------------------------------------------------------------------
# Clean target
#------------------------------------------------------------------------------

clean:
	cargo clean
	rm -f $(DEPS_MARKER)

#------------------------------------------------------------------------------
# Help
#------------------------------------------------------------------------------

help:
	@echo "WhisperMe Makefile"
	@echo ""
	@echo "Usage: make [target]"
	@echo ""
	@echo "Targets:"
	@echo "  deps      - Install system dependencies and download model"
	@echo "  build     - Build the project (debug)"
	@echo "  release   - Build the project (release)"
	@echo "  test      - Run all tests"
	@echo "  run       - Run the daemon"
	@echo "  install   - Install to ~/.local/bin"
	@echo "  clean     - Clean build artifacts"
	@echo "  help      - Show this help"
	@echo ""
	@echo "Options:"
	@echo "  MODEL=base    - Whisper model to download (default: base)"
	@echo "                  Options: tiny, base, small, medium, large-v3"
	@echo "                  Add .en suffix for English-only: base.en, small.en"
	@echo ""
	@echo "Examples:"
	@echo "  make deps              # Install dependencies with base model"
	@echo "  make deps MODEL=small  # Install with small model"
	@echo "  make build             # Build debug version"
	@echo "  make run               # Run the daemon"
