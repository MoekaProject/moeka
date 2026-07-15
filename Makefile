SHELL := /bin/bash

APP_NAME := moeka
APP_DISPLAY_NAME := Moeka
TARGET ?= x86_64-apple-darwin
PROFILE ?= dist
VERSION := $(shell awk -F '"' '/^version = / { print $$2; exit }' Cargo.toml)
BUNDLE_ID ?= app.moeka.moeka
BUILD_VERSION ?= $(VERSION)

DIST_DIR := dist
MACOS_BUILD_DIR := target/macos/$(TARGET)
DMG_STAGING_DIR := $(MACOS_BUILD_DIR)/dmg-root
APP_DIR := $(MACOS_BUILD_DIR)/$(APP_DISPLAY_NAME).app
CONTENTS_DIR := $(APP_DIR)/Contents
MACOS_DIR := $(CONTENTS_DIR)/MacOS
RESOURCES_DIR := $(CONTENTS_DIR)/Resources
BINARY_PATH := target/$(TARGET)/$(PROFILE)/$(APP_NAME)
DMG_PATH := $(DIST_DIR)/$(APP_DISPLAY_NAME).dmg
INFO_PLIST_TEMPLATE := packaging/macos/Info.plist
ICON_FILE := packaging/macos/AppIcon.icns
ICON_SOURCE := packaging/macos/AppIcon-1024.png
ICON_SCRIPT := scripts/generate_app_icon.sh
DMG_BACKGROUND := install-bg.jpg
DMG_SCRIPT := scripts/create_dmg.sh
VOLUME_NAME := $(APP_DISPLAY_NAME)
TAG ?= $(firstword $(filter v%,$(MAKECMDGOALS)))
RELEASE_REPO ?= MoekaProject/release
DIST_SUFFIXES := arm64 universal

.PHONY: dmg app build-release clean-dmg clean-app tag dist dist-arm64 dist-universal publish

dist: $(ICON_FILE)
	@rustup target add aarch64-apple-darwin x86_64-apple-darwin
	@$(MAKE) -j2 dist-arm64 dist-universal
	@ls -lh "$(DIST_DIR)"

dist-arm64:
	@$(MAKE) dmg \
		TARGET=aarch64-apple-darwin \
		DMG_PATH="$(DIST_DIR)/$(APP_DISPLAY_NAME)-macos-arm64.dmg" \
		VOLUME_NAME="$(APP_DISPLAY_NAME)-arm64"

dist-universal:
	@$(MAKE) dmg \
		TARGET=universal-apple-darwin \
		DMG_PATH="$(DIST_DIR)/$(APP_DISPLAY_NAME)-macos-universal.dmg" \
		VOLUME_NAME="$(APP_DISPLAY_NAME)-universal"

publish:
	@if [[ ! "$(TAG)" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$$ ]]; then \
		echo "Usage: make publish vX.Y.Z (run 'make dist' first)"; \
		exit 1; \
	fi
	@command -v gh >/dev/null || { echo "gh CLI is required: brew install gh && gh auth login"; exit 1; }
	@set -euo pipefail; \
	assets=(); \
	for suffix in $(DIST_SUFFIXES); do \
		asset="$(DIST_DIR)/$(APP_DISPLAY_NAME)-macos-$$suffix.dmg"; \
		[[ -f "$$asset" ]] || { echo "Missing $$asset — run 'make dist' first"; exit 1; }; \
		assets+=("$$asset"); \
	done; \
	notes="$$(git for-each-ref "refs/tags/$(TAG)" --format='%(contents)')"; \
	[[ -n "$$notes" ]] || notes="Release $(TAG)"; \
	if gh release view "$(TAG)" --repo "$(RELEASE_REPO)" >/dev/null 2>&1; then \
		gh release delete "$(TAG)" --repo "$(RELEASE_REPO)" --cleanup-tag --yes; \
	fi; \
	gh release create "$(TAG)" \
		--repo "$(RELEASE_REPO)" \
		--title "$(TAG)" \
		--notes "$$notes" \
		--latest; \
	gh release upload "$(TAG)" --repo "$(RELEASE_REPO)" --clobber "$${assets[@]}"

tag:
	@if [[ ! "$(TAG)" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$$ ]]; then \
		echo "Usage: make tag vX.Y.Z"; \
		exit 1; \
	fi
	@git tag --delete "$(TAG)" >/dev/null 2>&1 || true
	@git tag "$(TAG)"
	@git push -f origin "$(TAG)"

v%:
	@:

dmg: $(DMG_PATH)

$(DMG_PATH): app $(DMG_BACKGROUND) $(DMG_SCRIPT)
	@mkdir -p "$(DIST_DIR)"
	@rm -rf "$(DMG_STAGING_DIR)"
	@mkdir -p "$(DMG_STAGING_DIR)"
	@cp -R "$(APP_DIR)" "$(DMG_STAGING_DIR)/$(APP_DISPLAY_NAME).app"
	@ln -sfn /Applications "$(DMG_STAGING_DIR)/Applications"
	@rm -f "$(DMG_PATH)"
	@bash "$(DMG_SCRIPT)" "$(DMG_STAGING_DIR)" "$(DMG_PATH)" "$(VOLUME_NAME)" "$(APP_DISPLAY_NAME)" "$(DMG_BACKGROUND)"
	@echo "Created $(DMG_PATH)"

app: $(APP_DIR)

$(ICON_FILE): $(ICON_SOURCE) $(ICON_SCRIPT)
	@bash "$(ICON_SCRIPT)" "$(ICON_SOURCE)" "$(ICON_FILE)"

$(APP_DIR): build-release $(INFO_PLIST_TEMPLATE) $(ICON_FILE)
	@rm -rf "$(APP_DIR)"
	@mkdir -p "$(MACOS_DIR)" "$(RESOURCES_DIR)"
	@cp "$(BINARY_PATH)" "$(MACOS_DIR)/$(APP_NAME)"
	@chmod +x "$(MACOS_DIR)/$(APP_NAME)"
	@rsync -a \
		--exclude '.DS_Store' \
		assets "$(RESOURCES_DIR)/"
	@rsync -a \
		--exclude '.DS_Store' \
		docs "$(RESOURCES_DIR)/"
	@cp -R packaging/macos/*.lproj "$(RESOURCES_DIR)/"
	@sed \
		-e 's|__APP_EXECUTABLE__|$(APP_NAME)|g' \
		-e 's|__APP_NAME__|$(APP_DISPLAY_NAME)|g' \
		-e 's|__BUNDLE_IDENTIFIER__|$(BUNDLE_ID)|g' \
		-e 's|__APP_VERSION__|$(VERSION)|g' \
		-e 's|__APP_BUILD_VERSION__|$(BUILD_VERSION)|g' \
		"$(INFO_PLIST_TEMPLATE)" > "$(CONTENTS_DIR)/Info.plist"
	@if [[ -f "$(ICON_FILE)" ]]; then \
		cp "$(ICON_FILE)" "$(RESOURCES_DIR)/AppIcon.icns"; \
		plutil -insert CFBundleIconFile -string AppIcon.icns "$(CONTENTS_DIR)/Info.plist"; \
	fi
	@touch "$(APP_DIR)"
	@echo "Created $(APP_DIR)"

build-release:
	@if [[ "$(TARGET)" == "universal-apple-darwin" ]]; then \
		rustup target add aarch64-apple-darwin x86_64-apple-darwin; \
		cargo build --profile "$(PROFILE)" --target aarch64-apple-darwin; \
		cargo build --profile "$(PROFILE)" --target x86_64-apple-darwin; \
		mkdir -p "target/$(TARGET)/$(PROFILE)"; \
		lipo -create \
			"target/aarch64-apple-darwin/$(PROFILE)/$(APP_NAME)" \
			"target/x86_64-apple-darwin/$(PROFILE)/$(APP_NAME)" \
			-output "$(BINARY_PATH)"; \
	else \
		rustup target add "$(TARGET)"; \
		cargo build --profile "$(PROFILE)" --target "$(TARGET)"; \
	fi

clean-dmg:
	@rm -f "$(DMG_PATH)"
	@rm -rf "$(DMG_STAGING_DIR)"

clean-app:
	@rm -rf "$(APP_DIR)" "$(MACOS_BUILD_DIR)"
