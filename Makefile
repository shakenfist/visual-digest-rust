# visual-digest-rust - build, lint, and release targets.
#
# Everything that touches Rust runs inside the `visual-digest-rust-dev`
# devcontainer image (built from .devcontainer/), so no native Rust
# toolchain is required on the host. Modelled on ryll's Makefile.

IMAGE := visual-digest-rust-dev
DEVCONTAINER_DIR := .devcontainer
CARGO_CACHE := .cargo-cache

# Detect user/group for permission-safe container runs.
UID := $(shell id -u)
GID := $(shell id -g)

# Shared docker-run invocation for working inside the devcontainer.
# Mirrors scripts/check-rust.sh: the cargo cache is bind-mounted so
# downloads persist between runs, and HOME points at /build where the
# image installed the toolchain.
DOCKER_RUN := docker run --rm \
	-v "$(CURDIR)":/workspace \
	-v "$(CURDIR)/$(CARGO_CACHE)/registry":/build/.cargo/registry \
	-v "$(CURDIR)/$(CARGO_CACHE)/git":/build/.cargo/git \
	-w /workspace \
	-u $(UID):$(GID) \
	-e HOME=/build

.PHONY: all help devcontainer ensure-cache build test lint lint-fix \
	propose-release tag-release publish-crates

all: build

help:
	@echo "visual-digest-rust targets:"
	@echo "  make build                  - Build the workspace (debug)"
	@echo "  make test                   - Run tests (all features)"
	@echo "  make lint                   - rustfmt --check + clippy -D warnings"
	@echo "  make lint-fix               - rustfmt + clippy --fix"
	@echo "  make devcontainer           - Build the development container"
	@echo ""
	@echo "Release (shakenfist-visual-digest only; digest-decode is publish = false):"
	@echo "  make propose-release X.Y.Z  - Branch off main, bump version, push for PR"
	@echo "  make tag-release X.Y.Z      - After PR merge: tag main"
	@echo "  make publish-crates         - Publish to crates.io (needs CARGO_REGISTRY_TOKEN; IRREVERSIBLE)"

# Build the devcontainer image.
devcontainer:
	docker build -t $(IMAGE) $(DEVCONTAINER_DIR)

# Create cargo cache directories.
$(CARGO_CACHE)/registry $(CARGO_CACHE)/git:
	mkdir -p $@

# Ensure the cargo cache is writable by the build user. A previous
# root-owned docker run can leave these owned by root.
ensure-cache: devcontainer $(CARGO_CACHE)/registry $(CARGO_CACHE)/git
	@if [ ! -w "$(CARGO_CACHE)/registry" ] || [ ! -w "$(CARGO_CACHE)/git" ]; then \
		echo "Fixing cargo cache permissions..."; \
		docker run --rm \
			-v "$(CURDIR)/$(CARGO_CACHE)":/cache \
			$(IMAGE) \
			chown -R $(UID):$(GID) /cache; \
	fi

# Build the whole workspace (debug).
build: ensure-cache
	$(DOCKER_RUN) $(IMAGE) cargo build --workspace

# Run tests with all features enabled.
test: ensure-cache
	$(DOCKER_RUN) $(IMAGE) cargo test --workspace --all-features

# Lint via the shared helper (rustfmt --check + clippy -D warnings),
# which also builds the image and fixes cache ownership as needed.
lint:
	./scripts/check-rust.sh check

lint-fix:
	./scripts/check-rust.sh fix

# Cutting a release is a two-phase operation so the version bump goes
# through the normal PR review gate rather than landing directly on
# main.
#
# Phase 1: `make propose-release X.Y.Z` creates a release-X.Y.Z branch
# off main, bumps the shakenfist-visual-digest version, and pushes the
# branch for review.
#
# Phase 2: after the PR merges, `make tag-release X.Y.Z` tags the merge
# commit on main. Then `make publish-crates` uploads to crates.io.
#
# The second word of MAKECMDGOALS is the version; the no-op rule below
# catches X.Y.Z-shaped goals so make does not complain about "no rule
# to make target X.Y.Z".
RELEASE_VERSION := $(word 2,$(MAKECMDGOALS))

propose-release:
	@if [ -z "$(RELEASE_VERSION)" ]; then \
		echo "usage: make propose-release X.Y.Z"; exit 1; \
	fi
	./tools/propose-release.sh $(RELEASE_VERSION)

tag-release:
	@if [ -z "$(RELEASE_VERSION)" ]; then \
		echo "usage: make tag-release X.Y.Z"; exit 1; \
	fi
	./tools/tag-release.sh $(RELEASE_VERSION)

# Absorb a version-shaped second word as a no-op target. This only
# matches X.Y.Z numeric forms, so typos in real targets still fail
# loudly.
ifneq ($(filter propose-release tag-release,$(MAKECMDGOALS)),)
$(RELEASE_VERSION):
	@:
endif

# Publish shakenfist-visual-digest to crates.io from the current
# checkout. Forwards CARGO_REGISTRY_TOKEN into the devcontainer. Run
# after tag-release, on the merged release commit.
publish-crates: ensure-cache
	$(DOCKER_RUN) -e CARGO_REGISTRY_TOKEN $(IMAGE) tools/publish-crates.sh
