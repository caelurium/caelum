.DEFAULT_GOAL := help
PROJECT ?= .
STRUCTURE_ARGS ?=
PACKAGES := --workspace
HOST := $(shell uname -s)
TEST_SESSION := $(if $(filter Linux,$(HOST)),dbus-run-session --,)

.PHONY: help dev app build cli cli-run format lint structure test check previews
help:
	@printf '%s\n' \
	  'Caelum native development:' \
	  '  make dev PROJECT=.     Run the native desktop app' \
	  '  make app               Build the native desktop package' \
	  '  make cli               Build the standalone CLI' \
	  '  make cli-run PROJECT=. Open a project in the installed app' \
	  '  make previews          Capture native reference screens' \
	  '  make check             Structure, format, lint and test'
ifeq ($(HOST),Darwin)
dev:
	python3 scripts/dev/macos_app.py -- --project "$(PROJECT)"
app:
	bash scripts/release/package_macos_app.sh
else
dev:
	cargo run --locked -p caelum-desktop -- --project "$(PROJECT)"
app:
	bash scripts/release/package_linux_assets.sh
endif
build: app cli
cli:
	cargo build --locked -p caelum-cli
cli-run:
	cargo run --locked -p caelum-cli -- "$(PROJECT)"
format:
	cargo fmt --all -- --check
lint:
	cargo clippy --locked $(PACKAGES) --all-targets -- -D warnings
structure:
	nix run github:caelurium/rust-structure/v1.0.0 -- --root . --base HEAD --config .github/rust-structure.json $(STRUCTURE_ARGS)
test:
	$(TEST_SESSION) cargo test --locked $(PACKAGES) -- --test-threads=4
	python3 -m unittest discover -s scripts/release -p 'test_*.py'
	python3 -m unittest discover -s scripts/profile -p 'test_*.py'
	python3 -m unittest discover -s scripts/dev -p 'test_*.py'
	python3 -m unittest discover -s apps/desktop/preview -p 'test_*.py'
check: structure format lint test
previews:
	python3 apps/desktop/preview/capture.py
