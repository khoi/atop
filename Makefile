# Derived values (DO NOT TOUCH).
#
CURRENT_MAKEFILE_PATH := $(abspath $(lastword $(MAKEFILE_LIST)))
CURRENT_MAKEFILE_DIR := $(patsubst %/,%,$(dir $(CURRENT_MAKEFILE_PATH)))

BIN_NAME := atop
DEST_DIR := $(HOME)/.local/bin
BUILD_BIN := target/release/$(BIN_NAME)

.PHONY: help build-release ensure-bin-dir install-bin install

help:  # Display this help.
	@-+echo "Run make with one of the following targets:"
	@-+echo
	@-+grep -Eh "^[a-z-]+:.*#" $(CURRENT_MAKEFILE_PATH) | sed -E 's/^(.*:)(.*#+)(.*)/  \1 @@@ \3 /' | column -t -s "@@@"

build-release: # Build release binary
	@set -e; \
	echo "Building release binary..."; \
	cargo build --release >/dev/null; \
	if [ ! -f "$(BUILD_BIN)" ]; then echo "Error: compiled binary not found at $(BUILD_BIN)"; exit 1; fi

ensure-bin-dir: 
	@set -e; \
	if [ ! -d "$(DEST_DIR)" ]; then mkdir -p "$(DEST_DIR)"; fi; \
	if ! printf "%s" ":$$PATH:" | grep -q ":$(DEST_DIR):"; then \
	  echo "Warning: $(DEST_DIR) is not in PATH. Add it to use '$(BIN_NAME)' without a path."; \
	fi

install-bin: build-release ensure-bin-dir 
	@set -e; \
	if [ -w "$(DEST_DIR)" ]; then \
	  install -m 0755 "$(BUILD_BIN)" "$(DEST_DIR)/$(BIN_NAME)"; \
	  echo "Installed $(BIN_NAME) to $(DEST_DIR)"; \
	else \
	  echo "Error: $(DEST_DIR) is not writable."; \
	  echo "Try: sudo install -m 0755 \"$(BUILD_BIN)\" \"$(DEST_DIR)/$(BIN_NAME)\""; \
	  exit 1; \
	fi

install: build-release ensure-bin-dir install-bin # Install binary into ~/.local/bin
