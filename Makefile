SHELL := /bin/bash

APP_NAME := markdown-editor
TARGET ?= x86_64-apple-darwin
PROFILE ?= release
VERSION := $(shell awk -F '"' '/^version = / { print $$2; exit }' Cargo.toml)
BUNDLE_ID ?= com.imeoer.$(APP_NAME)
BUILD_VERSION ?= $(VERSION)

DIST_DIR := dist
MACOS_BUILD_DIR := target/macos/$(TARGET)
DMG_STAGING_DIR := $(MACOS_BUILD_DIR)/dmg-root
APP_DIR := $(MACOS_BUILD_DIR)/$(APP_NAME).app
CONTENTS_DIR := $(APP_DIR)/Contents
MACOS_DIR := $(CONTENTS_DIR)/MacOS
RESOURCES_DIR := $(CONTENTS_DIR)/Resources
BINARY_PATH := target/$(TARGET)/$(PROFILE)/$(APP_NAME)
DMG_PATH := $(DIST_DIR)/$(APP_NAME)-$(TARGET).dmg
INFO_PLIST_TEMPLATE := packaging/macos/Info.plist
ICON_FILE := packaging/macos/AppIcon.icns
ICON_SOURCE := packaging/macos/AppIcon-1024.png
ICON_SCRIPT := scripts/generate_app_icon.sh
VOLUME_NAME := $(APP_NAME)

.PHONY: dmg app build-release clean-dmg clean-app

dmg: $(DMG_PATH)

$(DMG_PATH): app
	@mkdir -p "$(DIST_DIR)"
	@rm -rf "$(DMG_STAGING_DIR)"
	@mkdir -p "$(DMG_STAGING_DIR)"
	@cp -R "$(APP_DIR)" "$(DMG_STAGING_DIR)/$(APP_NAME).app"
	@ln -sfn /Applications "$(DMG_STAGING_DIR)/Applications"
	@rm -f "$(DMG_PATH)"
	@hdiutil create -volname "$(VOLUME_NAME)" -srcfolder "$(DMG_STAGING_DIR)" -ov -format UDZO "$(DMG_PATH)"
	@echo "Created $(DMG_PATH)"

app: $(APP_DIR)

$(ICON_FILE): $(ICON_SOURCE) $(ICON_SCRIPT)
	@bash "$(ICON_SCRIPT)" "$(ICON_SOURCE)" "$(ICON_FILE)"

$(APP_DIR): build-release $(INFO_PLIST_TEMPLATE) $(ICON_FILE)
	@rm -rf "$(APP_DIR)"
	@mkdir -p "$(MACOS_DIR)" "$(RESOURCES_DIR)"
	@cp "$(BINARY_PATH)" "$(MACOS_DIR)/$(APP_NAME)"
	@chmod +x "$(MACOS_DIR)/$(APP_NAME)"
	@cp -R assets "$(RESOURCES_DIR)/"
	@cp EDITOR.md "$(RESOURCES_DIR)/EDITOR.md"
	@sed \
		-e 's|__APP_EXECUTABLE__|$(APP_NAME)|g' \
		-e 's|__APP_NAME__|$(APP_NAME)|g' \
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
	@rustup target add "$(TARGET)"
	@cargo build --profile "$(PROFILE)" --target "$(TARGET)"

clean-dmg:
	@rm -f "$(DMG_PATH)"
	@rm -rf "$(DMG_STAGING_DIR)"

clean-app:
	@rm -rf "$(APP_DIR)" "$(MACOS_BUILD_DIR)"
