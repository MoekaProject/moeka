SHELL := /bin/bash

APP_NAME := moeka
APP_DISPLAY_NAME := Moeka
LSREGISTER := /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
RUSTC ?= $(shell command -v rustc 2>/dev/null || printf '%s/.cargo/bin/rustc' "$$HOME")
HOST_TARGET := $(shell "$(RUSTC)" -vV | awk '/^host:/ { print $$2 }')
TARGET ?= $(HOST_TARGET)
PROFILE := release
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
ifeq ($(TARGET),$(HOST_TARGET))
BINARY_PATH := target/$(PROFILE)/$(APP_NAME)
CARGO_TARGET_ARG :=
else
BINARY_PATH := target/$(TARGET)/$(PROFILE)/$(APP_NAME)
CARGO_TARGET_ARG := --target "$(TARGET)"
endif
DMG_PATH := $(DIST_DIR)/$(APP_DISPLAY_NAME).dmg
INFO_PLIST_TEMPLATE := packaging/macos/Info.plist
ICON_FILE := packaging/macos/AppIcon.icns
ICON_SOURCE := packaging/macos/AppIcon-1024.png
ICON_SCRIPT := scripts/generate_app_icon.sh
DMG_BACKGROUND := install-bg.jpg
DMG_SCRIPT := scripts/create_dmg.sh
SIGNED_PACKAGE_SCRIPT := scripts/sign_notarize_package.sh
VOLUME_NAME := $(APP_DISPLAY_NAME)
TAG ?= $(firstword $(filter v%,$(MAKECMDGOALS)))
RELEASE_REPO ?= MoekaProject/release
DEVELOPER_ID_APPLICATION ?= $(shell security find-identity -v -p codesigning 2>/dev/null | sed -nE 's/.*"(Developer ID Application:[^"]+)".*/\1/p' | head -1)
TEAM_ID ?= $(shell printf '%s\n' '$(DEVELOPER_ID_APPLICATION)' | sed -nE 's/.*\(([A-Z0-9]{10})\)$$/\1/p')
NOTARY_PROFILE ?= moeka-notary
DIST_SUFFIXES := arm64 x86_64
ZIP_PATHS := \
	$(DIST_DIR)/$(APP_DISPLAY_NAME)-macos-arm64.zip \
	$(DIST_DIR)/$(APP_DISPLAY_NAME)-macos-x86_64.zip

.PHONY: dmg dmg-one dmg-arm64 dmg-x86_64 zip zip-arm64 zip-x86_64 zip-one signed-release signed-release-one signed-release-arm64 signed-release-x86_64 notary-setup app build-release clean-dmg clean-app tag dist publish publish-check

dmg: $(ICON_FILE)
	@rustup target add aarch64-apple-darwin x86_64-apple-darwin
	@$(MAKE) -j2 dmg-arm64 dmg-x86_64
	@ls -lh "$(DIST_DIR)"

dmg-arm64:
	@$(MAKE) dmg-one \
		TARGET=aarch64-apple-darwin \
		DMG_PATH="$(DIST_DIR)/$(APP_DISPLAY_NAME)-macos-arm64.dmg" \
		VOLUME_NAME="$(APP_DISPLAY_NAME)-arm64"

dmg-x86_64:
	@$(MAKE) dmg-one \
		TARGET=x86_64-apple-darwin \
		DMG_PATH="$(DIST_DIR)/$(APP_DISPLAY_NAME)-macos-x86_64.dmg" \
		VOLUME_NAME="$(APP_DISPLAY_NAME)-x86_64"

dist: dmg

publish: publish-check
	@git push --force-with-lease origin HEAD:main
	@$(MAKE) tag TAG="$(TAG)"
	@$(MAKE) signed-release
	@set -euo pipefail; \
	assets=(); \
	for suffix in $(DIST_SUFFIXES); do \
		asset="$(DIST_DIR)/$(APP_DISPLAY_NAME)-macos-$$suffix.dmg"; \
		[[ -f "$$asset" ]] || { echo "Missing $$asset"; exit 1; }; \
		assets+=("$$asset"); \
	done; \
	for asset in $(ZIP_PATHS); do \
		[[ -f "$$asset" ]] || { echo "Missing $$asset"; exit 1; }; \
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

publish-check:
	@if [[ ! "$(TAG)" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$$ ]]; then \
		echo "Usage: make publish vX.Y.Z"; \
		exit 1; \
	fi
	@if [[ "$(TAG)" != "v$(VERSION)" ]]; then \
		echo "Tag/version mismatch: TAG=$(TAG), Cargo.toml version=$(VERSION)."; \
		echo "Please bump Cargo.toml version or publish with matching tag."; \
		exit 1; \
	fi
	@if [[ -n "$$(git status --porcelain)" ]]; then \
		echo "Working tree is dirty; commit or stash before publishing $(TAG)."; \
		exit 1; \
	fi
	@command -v gh >/dev/null || { echo "gh CLI is required: brew install gh && gh auth login"; exit 1; }
	@test -n "$(DEVELOPER_ID_APPLICATION)" || { \
		echo "No Developer ID Application identity found in Keychain."; \
		echo "Create it in Xcode > Settings > Accounts > Manage Certificates."; \
		exit 1; \
	}
	@security find-identity -v -p codesigning | grep -F '"$(DEVELOPER_ID_APPLICATION)"' >/dev/null || { \
		echo "Signing identity not available: $(DEVELOPER_ID_APPLICATION)"; \
		exit 1; \
	}
	@xcrun notarytool history --keychain-profile "$(NOTARY_PROFILE)" >/dev/null 2>&1 || { \
		echo "Notary credentials not found: $(NOTARY_PROFILE)"; \
		echo "Run: make notary-setup APPLE_ID=your-apple-id@example.com"; \
		exit 1; \
	}

notary-setup:
	@test -n "$(APPLE_ID)" || { echo "Usage: make notary-setup APPLE_ID=your-apple-id@example.com"; exit 1; }
	@test -n "$(TEAM_ID)" || { echo "Unable to detect Team ID from signing identity."; exit 1; }
	@xcrun notarytool store-credentials "$(NOTARY_PROFILE)" \
		--apple-id "$(APPLE_ID)" \
		--team-id "$(TEAM_ID)"

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

zip:
	@$(MAKE) -j2 zip-arm64 zip-x86_64

zip-arm64:
	@$(MAKE) zip-one \
		TARGET=aarch64-apple-darwin \
		ZIP_PATH="$(DIST_DIR)/$(APP_DISPLAY_NAME)-macos-arm64.zip"

zip-x86_64:
	@$(MAKE) zip-one \
		TARGET=x86_64-apple-darwin \
		ZIP_PATH="$(DIST_DIR)/$(APP_DISPLAY_NAME)-macos-x86_64.zip"

zip-one: app
	@mkdir -p "$(DIST_DIR)"
	@rm -f "$(ZIP_PATH)"
	@ditto -c -k --sequesterRsrc --keepParent "$(APP_DIR)" "$(ZIP_PATH)"
	@echo "Created $(ZIP_PATH)"

signed-release:
	@rustup target add aarch64-apple-darwin x86_64-apple-darwin
	@$(MAKE) -j2 signed-release-arm64 signed-release-x86_64
	@ls -lh "$(DIST_DIR)"

signed-release-arm64:
	@$(MAKE) signed-release-one \
		TARGET=aarch64-apple-darwin \
		DMG_PATH="$(DIST_DIR)/$(APP_DISPLAY_NAME)-macos-arm64.dmg" \
		ZIP_PATH="$(DIST_DIR)/$(APP_DISPLAY_NAME)-macos-arm64.zip" \
		VOLUME_NAME="$(APP_DISPLAY_NAME)-arm64"

signed-release-x86_64:
	@$(MAKE) signed-release-one \
		TARGET=x86_64-apple-darwin \
		DMG_PATH="$(DIST_DIR)/$(APP_DISPLAY_NAME)-macos-x86_64.dmg" \
		ZIP_PATH="$(DIST_DIR)/$(APP_DISPLAY_NAME)-macos-x86_64.zip" \
		VOLUME_NAME="$(APP_DISPLAY_NAME)-x86_64"

signed-release-one: app $(DMG_BACKGROUND) $(DMG_SCRIPT) $(SIGNED_PACKAGE_SCRIPT)
	@bash "$(SIGNED_PACKAGE_SCRIPT)" \
		"$(APP_DIR)" \
		"$(DMG_STAGING_DIR)" \
		"$(DMG_PATH)" \
		"$(ZIP_PATH)" \
		"$(VOLUME_NAME)" \
		"$(APP_DISPLAY_NAME)" \
		"$(DMG_BACKGROUND)" \
		"$(DEVELOPER_ID_APPLICATION)" \
		"$(NOTARY_PROFILE)"

dmg-one: $(DMG_PATH)

$(DMG_PATH): app $(DMG_BACKGROUND) $(DMG_SCRIPT)
	@mkdir -p "$(DIST_DIR)"
	@rm -rf "$(DMG_STAGING_DIR)"
	@mkdir -p "$(DMG_STAGING_DIR)"
	@cp -R "$(APP_DIR)" "$(DMG_STAGING_DIR)/$(APP_DISPLAY_NAME).app"
	@ln -sfn /Applications "$(DMG_STAGING_DIR)/Applications"
	@rm -f "$(DMG_PATH)"
	@bash "$(DMG_SCRIPT)" "$(DMG_STAGING_DIR)" "$(DMG_PATH)" "$(VOLUME_NAME)" "$(APP_DISPLAY_NAME)" "$(DMG_BACKGROUND)"
	@$(LSREGISTER) -u "$(DMG_STAGING_DIR)/$(APP_DISPLAY_NAME).app" >/dev/null 2>&1 || true
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
	# Keep build products out of the system "Open With" list: LaunchServices
	# otherwise accumulates one entry per target/DMG copy of the bundle.
	@$(LSREGISTER) -u "$(APP_DIR)" >/dev/null 2>&1 || true
	@echo "Created $(APP_DIR)"

build-release:
	@if [[ "$(TARGET)" != "$(HOST_TARGET)" ]]; then rustup target add "$(TARGET)"; fi
	@cargo build --release $(CARGO_TARGET_ARG)

clean-dmg:
	@rm -f "$(DMG_PATH)"
	@rm -rf "$(DMG_STAGING_DIR)"

clean-app:
	@rm -rf "$(APP_DIR)" "$(MACOS_BUILD_DIR)"
