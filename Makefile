# Detect install prefix: use ~/.cargo if it's in PATH, otherwise ~/.local
CARGO_BIN_IN_PATH := $(shell echo "$$PATH" | tr ':' '\n' | grep -q "$$HOME/.cargo/bin" && echo yes)
ifeq ($(CARGO_BIN_IN_PATH),yes)
  DEFAULT_PREFIX := $(HOME)/.cargo
else
  DEFAULT_PREFIX := $(HOME)/.local
endif
PREFIX ?= $(DEFAULT_PREFIX)

.PHONY: build install uninstall

build:
	cargo build --release

install: build
	@mkdir -p $(PREFIX)/bin
	cp target/release/treeyard $(PREFIX)/bin/treeyard
	@echo "Installed to $(PREFIX)/bin/treeyard"

uninstall:
	rm -f $(PREFIX)/bin/treeyard
	@echo "Removed $(PREFIX)/bin/treeyard"
