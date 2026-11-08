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
RELEASE_REPO ?= MoekaProject/community
DEVELOPER_ID_APPLICATION ?= $(shell security find-identity -v -p codesigning 2>/dev/null | sed -nE 's/.*"(Developer ID Application:[^"]+)".*/\1/p' | head -1)
TEAM_ID ?= $(shell printf '%s\n' '$(DEVELOPER_ID_APPLICATION)' | sed -nE 's/.*\(([A-Z0-9]{10})\)$$/\1/p')
NOTARY_PROFILE ?= moeka-notary
DIST_SUFFIXES := arm64 x86_64
ZIP_PATHS := \
	$(DIST_DIR)/$(APP_DISPLAY_NAME)-macos-arm64.zip \
	$(DIST_DIR)/$(APP_DISPLAY_NAME)-macos-x86_64.zip
WINDOWS_TARGETS := x86_64-pc-windows-msvc aarch64-pc-windows-msvc
WINDOWS_ZIP_PATHS := \
	$(DIST_DIR)/$(APP_DISPLAY_NAME)-windows-arm64.zip \
	$(DIST_DIR)/$(APP_DISPLAY_NAME)-windows-x86_64.zip
XWIN_SHIMS := packaging/windows/xwin-shims
LINUX_TARGETS := x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu
LINUX_ARCHIVES := \
	$(DIST_DIR)/$(APP_DISPLAY_NAME)-linux-arm64.zip \
	$(DIST_DIR)/$(APP_DISPLAY_NAME)-linux-x86_64.zip
LINUX_DESKTOP_FILE := packaging/linux/app.moeka.moeka.desktop
LINUX_INSTALL_SCRIPT := packaging/linux/install.sh
LINUX_UNINSTALL_SCRIPT := packaging/linux/uninstall.sh
LINUX_README := packaging/linux/README.txt
LINUX_ICON_SOURCE := packaging/linux/app.moeka.moeka.png
LINUX_SYSROOT_SCRIPT := scripts/prepare_linux_sysroot.py
LINUX_SYSROOT_ROOT ?= target/linux-sysroots
LINUX_GLIBC_VERSION ?= 2.35
LINUX_BUNDLED_LIBS := \
	libXau.so.6 \
	libXdmcp.so.6 \
	libbsd.so.0 \
	libmd.so.0 \
	libxcb.so.1 \
	libxcb-xkb.so.1 \
	libxkbcommon.so.0 \
	libxkbcommon-x11.so.0

# Mac App Store channel. Universal, sandboxed, and built with `--features mas`,
# which drops the terminal and self-update capabilities.
MAS_TARGETS := aarch64-apple-darwin x86_64-apple-darwin
MAS_BUILD_DIR := target/macos/mas
MAS_APP_DIR := $(MAS_BUILD_DIR)/$(APP_DISPLAY_NAME).app
MAS_CONTENTS_DIR := $(MAS_APP_DIR)/Contents
MAS_ENTITLEMENTS := packaging/macos/mas.entitlements
MAS_PROVISION_PROFILE ?= packaging/macos/mas.provisionprofile
MAS_PKG_PATH := $(DIST_DIR)/$(APP_DISPLAY_NAME)-mas.pkg
MAS_CATEGORY ?= public.app-category.productivity
APPLE_DISTRIBUTION ?= $(shell security find-identity -v 2>/dev/null | sed -nE 's/.*"(Apple Distribution:[^"]+)".*/\1/p' | head -1)
MAS_INSTALLER_IDENTITY ?= $(shell security find-identity -v 2>/dev/null | sed -nE 's/.*"(3rd Party Mac Developer Installer:[^"]+)".*/\1/p' | head -1)
# Private frameworks the App Store rejects on sight, whether or not they run.
# `_CGS[A-Z]` matches the private WindowServer family without catching the
# public `CGShieldingWindowLevel` that cocoa references.
MAS_FORBIDDEN_SYMBOLS := _CGS[A-Z]|_AXUIElement|_AXIsProcessTrusted

.PHONY: dmg dmg-one dmg-arm64 dmg-x86_64 zip zip-arm64 zip-x86_64 zip-one signed-release signed-release-one signed-release-arm64 signed-release-x86_64 notary-setup app build-release clean-dmg clean-app tag dist publish publish-beta publish-check test-publish-release publish-tools-check check-targets windows windows-one windows-arm64 windows-x86_64 windows-icon linux linux-one linux-arm64 linux-x86_64 mas mas-app clean-mas

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

# Cross-builds a self-contained Moeka.exe for both architectures and zips each
# one. Requires `cargo install cargo-xwin` plus the `llvm-lib`/`lld-link` links
# described in packaging/windows/README.md.
windows:
	@source scripts/release-network.sh; release_retry_read rustup target add $(WINDOWS_TARGETS)
	@$(MAKE) windows-x86_64
	@$(MAKE) windows-arm64
	@ls -lh $(WINDOWS_ZIP_PATHS)

windows-x86_64:
	@$(MAKE) windows-one WINDOWS_TARGET=x86_64-pc-windows-msvc WINDOWS_SUFFIX=x86_64

windows-arm64:
	@$(MAKE) windows-one WINDOWS_TARGET=aarch64-pc-windows-msvc WINDOWS_SUFFIX=arm64

windows-one:
	@mkdir -p "$(DIST_DIR)"
	@chmod +x $(XWIN_SHIMS)/*
	PATH="$(CURDIR)/$(XWIN_SHIMS):$$PATH" \
		CARGO_INCREMENTAL=0 cargo xwin build --release --locked --target "$(WINDOWS_TARGET)"
	@set -euo pipefail; \
	zip_path="$(CURDIR)/$(DIST_DIR)/$(APP_DISPLAY_NAME)-windows-$(WINDOWS_SUFFIX).zip"; \
	stage="$$(mktemp -d)"; \
	cp "target/$(WINDOWS_TARGET)/release/$(APP_NAME).exe" "$$stage/$(APP_DISPLAY_NAME).exe"; \
	rm -f "$$zip_path"; \
	(cd "$$stage" && zip -q "$$zip_path" "$(APP_DISPLAY_NAME).exe"); \
	rm -rf "$$stage"

# Regenerates packaging/windows/moeka.ico from the app-icon master. Needs
# Pillow, and the result is committed, so this is not part of `make windows`.
windows-icon:
	@python3 scripts/generate_windows_icon.py

# Linux hosts build natively. macOS hosts use Zig plus a minimal, checksummed
# Ubuntu 22.04 sysroot containing the XCB/XKB libraries GPUI links directly.
linux:
	@case "$(HOST_TARGET)" in \
		*-apple-darwin) \
			source scripts/release-network.sh; release_retry_read rustup target add $(LINUX_TARGETS) || exit 1; \
			$(MAKE) linux-x86_64; \
			$(MAKE) linux-arm64; \
			ls -lh $(LINUX_ARCHIVES); \
			;; \
		x86_64-unknown-linux-gnu) $(MAKE) linux-x86_64 ;; \
		aarch64-unknown-linux-gnu) $(MAKE) linux-arm64 ;; \
		*) echo "Unsupported Linux packaging host: $(HOST_TARGET)"; exit 1 ;; \
	esac

linux-x86_64:
	@$(MAKE) linux-one \
		LINUX_TARGET=x86_64-unknown-linux-gnu \
		LINUX_SUFFIX=x86_64 \
		LINUX_TRIPLET=x86_64-linux-gnu

linux-arm64:
	@$(MAKE) linux-one \
		LINUX_TARGET=aarch64-unknown-linux-gnu \
		LINUX_SUFFIX=arm64 \
		LINUX_TRIPLET=aarch64-linux-gnu

linux-one:
	@source scripts/release-network.sh; release_retry_read rustup target add "$(LINUX_TARGET)"
	@set -euo pipefail; \
	case "$(HOST_TARGET)" in \
		"$(LINUX_TARGET)") \
			CARGO_INCREMENTAL=0 cargo build --release --locked --target "$(LINUX_TARGET)"; \
			library_dir="$$(dirname "$$(ldconfig -p | awk '/libxkbcommon-x11\.so\.0 / { print $$NF; exit }')")"; \
			;; \
		*-apple-darwin) \
			for tool in zig cargo-zigbuild python3 ar zstd patchelf; do \
				command -v "$$tool" >/dev/null || { echo "$$tool is required for Linux cross-compilation"; exit 1; }; \
			done; \
			sysroot="$(CURDIR)/$(LINUX_SYSROOT_ROOT)/$(LINUX_SUFFIX)"; \
			python3 "$(LINUX_SYSROOT_SCRIPT)" "$(LINUX_SUFFIX)" "$$sysroot"; \
			library_dir="$$sysroot/usr/lib/$(LINUX_TRIPLET)"; \
			RUSTFLAGS="$${RUSTFLAGS:+$$RUSTFLAGS }-L native=$$library_dir" \
				CARGO_INCREMENTAL=0 cargo zigbuild --release --locked \
				--target "$(LINUX_TARGET).$(LINUX_GLIBC_VERSION)"; \
			;; \
		*) \
			echo "Unsupported host for $(LINUX_TARGET): $(HOST_TARGET)"; \
			exit 1; \
			;; \
	esac; \
	binary="target/$(LINUX_TARGET)/release/$(APP_NAME)"; \
	if [[ "$(HOST_TARGET)" == "$(LINUX_TARGET)" ]]; then \
		if ldd "$$binary" | tee /dev/stderr | grep -q 'not found'; then \
			echo "Linux package has unresolved runtime libraries"; \
			exit 1; \
		fi; \
	else \
		file "$$binary" | grep -q 'ELF 64-bit' || { echo "Not a 64-bit Linux ELF: $$binary"; exit 1; }; \
	fi; \
	stage="$$(mktemp -d)"; \
	package="$$stage/$(APP_DISPLAY_NAME)-linux-$(LINUX_SUFFIX)"; \
	mkdir -p "$$package/lib" "$(DIST_DIR)"; \
	cp "$$binary" "$$package/$(APP_DISPLAY_NAME)"; \
	chmod +x "$$package/$(APP_DISPLAY_NAME)"; \
	patchelf --set-rpath '$$ORIGIN/lib' "$$package/$(APP_DISPLAY_NAME)"; \
	for library in $(LINUX_BUNDLED_LIBS); do \
		source="$$library_dir/$$library"; \
		[[ -e "$$source" ]] || { echo "Missing Linux runtime library: $$source"; exit 1; }; \
		cp -L "$$source" "$$package/lib/$$library"; \
		patchelf --set-rpath '$$ORIGIN' "$$package/lib/$$library"; \
	done; \
	cp "$(LINUX_DESKTOP_FILE)" "$$package/"; \
	cp "$(LINUX_INSTALL_SCRIPT)" "$$package/install.sh"; \
	chmod +x "$$package/install.sh"; \
	cp "$(LINUX_UNINSTALL_SCRIPT)" "$$package/uninstall.sh"; \
	chmod +x "$$package/uninstall.sh"; \
	cp "$(LINUX_README)" "$$package/README.txt"; \
	cp "$(LINUX_ICON_SOURCE)" "$$package/app.moeka.moeka.png"; \
	archive="$(CURDIR)/$(DIST_DIR)/$(APP_DISPLAY_NAME)-linux-$(LINUX_SUFFIX).zip"; \
	rm -f "$$archive"; \
	(cd "$$stage" && zip -qr "$$archive" "$$(basename "$$package")"); \
	rm -rf "$$stage"; \
	echo "Created $$archive"

publish: publish-check
	@bash scripts/publish-release.sh publish "$(TAG)" "$(RELEASE_REPO)"
	@$(MAKE) publish-homebrew TAG="$(TAG)" || { \
		echo "The application release is already published. Retry only: make publish-homebrew TAG=$(TAG)" >&2; \
		exit 1; \
	}

.PHONY: publish-homebrew homebrew-check test-publish-homebrew
publish-homebrew:
	@bash scripts/publish-homebrew.sh "$(TAG)"

homebrew-check:
	@bash scripts/publish-homebrew.sh "$(TAG)" check

test-publish-homebrew:
	@bash scripts/test-publish-homebrew.sh

publish-beta: publish-check
	@bash scripts/publish-release.sh publish-beta "$(TAG)" "$(RELEASE_REPO)"

publish-check:
	@bash scripts/publish-release.sh check "$(TAG)" "$(RELEASE_REPO)"

test-publish-release:
	@bash scripts/test-publish-release.sh
	@python3 scripts/test-prepare-linux-sysroot.py

# Only the publishing helper calls this, after the read-only safety checks.
publish-tools-check:
	@for tool in zig cargo-zigbuild python3 ar zstd patchelf jq; do \
		command -v "$$tool" >/dev/null || { echo "$$tool is required for Linux cross-compilation"; exit 1; }; \
	done
	@test -n "$(DEVELOPER_ID_APPLICATION)" || { \
		echo "No Developer ID Application identity found in Keychain."; \
		echo "Create it in Xcode > Settings > Accounts > Manage Certificates."; \
		exit 1; \
	}
	@security find-identity -v -p codesigning | grep -F '"$(DEVELOPER_ID_APPLICATION)"' >/dev/null || { \
		echo "Signing identity not available: $(DEVELOPER_ID_APPLICATION)"; \
		exit 1; \
	}
	@source scripts/release-network.sh; \
	release_retry_read xcrun notarytool history --keychain-profile "$(NOTARY_PROFILE)" >/dev/null || { \
		echo "Cannot verify notary profile or reach Apple: $(NOTARY_PROFILE). See the error above."; \
		exit 1; \
	}
	@$(MAKE) check-targets

# Compile-checks the cross targets before anything is pushed or built. Catches
# cfg-gated breakage (a macOS-only import used by shared code) in a couple of
# minutes instead of halfway through a release.
check-targets:
	@case "$(HOST_TARGET)" in \
		*-apple-darwin) ;; \
		*) echo "check-targets only runs on macOS hosts; skipping."; exit 0 ;; \
	esac; \
	set -euo pipefail; \
	source scripts/release-network.sh; \
	release_retry_read rustup target add $(WINDOWS_TARGETS) $(LINUX_TARGETS) >/dev/null; \
	chmod +x $(XWIN_SHIMS)/*; \
	for target in $(WINDOWS_TARGETS); do \
		echo "==> checking $$target"; \
		PATH="$(CURDIR)/$(XWIN_SHIMS):$$PATH" CARGO_INCREMENTAL=0 \
			cargo xwin check --release --locked --target "$$target"; \
	done; \
	for target in $(LINUX_TARGETS); do \
		echo "==> checking $$target"; \
		CARGO_INCREMENTAL=0 cargo-zigbuild check --release --locked \
			--target "$$target.$(LINUX_GLIBC_VERSION)"; \
	done; \
	echo "All cross targets compile."

notary-setup:
	@test -n "$(APPLE_ID)" || { echo "Usage: make notary-setup APPLE_ID=your-apple-id@example.com"; exit 1; }
	@test -n "$(TEAM_ID)" || { echo "Unable to detect Team ID from signing identity."; exit 1; }
	@xcrun notarytool store-credentials "$(NOTARY_PROFILE)" \
		--apple-id "$(APPLE_ID)" \
		--team-id "$(TEAM_ID)"

tag:
	@echo "Standalone tagging is disabled; use make publish vX.Y.Z after review." >&2
	@exit 1

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
	@source scripts/release-network.sh; release_retry_read rustup target add aarch64-apple-darwin x86_64-apple-darwin
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
	@if [[ "$(TARGET)" != "$(HOST_TARGET)" ]]; then source scripts/release-network.sh; release_retry_read rustup target add "$(TARGET)"; fi
	@CARGO_INCREMENTAL=0 cargo build --release --locked $(CARGO_TARGET_ARG)

# Unsigned universal App Store bundle. Signing is a separate step so the guard
# below fails the build before any certificate is involved.
mas-app: $(ICON_FILE) $(INFO_PLIST_TEMPLATE) $(MAS_ENTITLEMENTS)
	@rustup target add $(MAS_TARGETS)
	@for target in $(MAS_TARGETS); do \
		CARGO_INCREMENTAL=0 cargo build --release --locked --features mas --target "$$target" || exit 1; \
	done
	@set -euo pipefail; \
	for target in $(MAS_TARGETS); do \
		binary="target/$$target/$(PROFILE)/$(APP_NAME)"; \
		if nm -u "$$binary" | grep -qE '$(MAS_FORBIDDEN_SYMBOLS)'; then \
			echo "error: $$binary references a private Apple API:" >&2; \
			nm -u "$$binary" | grep -E '$(MAS_FORBIDDEN_SYMBOLS)' >&2; \
			exit 1; \
		fi; \
	done
	@rm -rf "$(MAS_APP_DIR)"
	@mkdir -p "$(MAS_CONTENTS_DIR)/MacOS" "$(MAS_CONTENTS_DIR)/Resources"
	@lipo -create -output "$(MAS_CONTENTS_DIR)/MacOS/$(APP_NAME)" \
		$(foreach target,$(MAS_TARGETS),"target/$(target)/$(PROFILE)/$(APP_NAME)")
	@chmod +x "$(MAS_CONTENTS_DIR)/MacOS/$(APP_NAME)"
	@rsync -a --exclude '.DS_Store' assets "$(MAS_CONTENTS_DIR)/Resources/"
	@rsync -a --exclude '.DS_Store' docs "$(MAS_CONTENTS_DIR)/Resources/"
	@cp -R packaging/macos/*.lproj "$(MAS_CONTENTS_DIR)/Resources/"
	@cp "$(ICON_FILE)" "$(MAS_CONTENTS_DIR)/Resources/AppIcon.icns"
	@sed \
		-e 's|__APP_EXECUTABLE__|$(APP_NAME)|g' \
		-e 's|__APP_NAME__|$(APP_DISPLAY_NAME)|g' \
		-e 's|__BUNDLE_IDENTIFIER__|$(BUNDLE_ID)|g' \
		-e 's|__APP_VERSION__|$(VERSION)|g' \
		-e 's|__APP_BUILD_VERSION__|$(BUILD_VERSION)|g' \
		"$(INFO_PLIST_TEMPLATE)" > "$(MAS_CONTENTS_DIR)/Info.plist"
	@plutil -insert CFBundleIconFile -string AppIcon.icns "$(MAS_CONTENTS_DIR)/Info.plist"
	@plutil -insert LSApplicationCategoryType -string "$(MAS_CATEGORY)" "$(MAS_CONTENTS_DIR)/Info.plist"
	@plutil -insert ITSAppUsesNonExemptEncryption -bool false "$(MAS_CONTENTS_DIR)/Info.plist"
	@$(LSREGISTER) -u "$(MAS_APP_DIR)" >/dev/null 2>&1 || true
	@echo "Created $(MAS_APP_DIR)"

# Signs the bundle for distribution and wraps it in the installer package that
# Transporter uploads. Notarization does not apply to this channel.
mas: mas-app
	@set -euo pipefail; \
	if [[ -z "$(APPLE_DISTRIBUTION)" ]]; then \
		echo "error: no 'Apple Distribution' identity in the keychain" >&2; exit 1; \
	fi; \
	if [[ -z "$(MAS_INSTALLER_IDENTITY)" ]]; then \
		echo "error: no '3rd Party Mac Developer Installer' identity in the keychain" >&2; exit 1; \
	fi; \
	if [[ ! -f "$(MAS_PROVISION_PROFILE)" ]]; then \
		echo "error: missing $(MAS_PROVISION_PROFILE) (Mac App Store provisioning profile)" >&2; \
		exit 1; \
	fi; \
	cp "$(MAS_PROVISION_PROFILE)" "$(MAS_CONTENTS_DIR)/embedded.provisionprofile"; \
	codesign --force --timestamp --options runtime \
		--sign "$(APPLE_DISTRIBUTION)" \
		--entitlements "$(MAS_ENTITLEMENTS)" \
		"$(MAS_APP_DIR)"; \
	codesign --verify --strict --verbose=2 "$(MAS_APP_DIR)"; \
	mkdir -p "$(DIST_DIR)"; \
	rm -f "$(MAS_PKG_PATH)"; \
	productbuild --component "$(MAS_APP_DIR)" /Applications \
		--sign "$(MAS_INSTALLER_IDENTITY)" "$(MAS_PKG_PATH)"; \
	echo "Created $(MAS_PKG_PATH)"; \
	echo "Upload with: xcrun altool --upload-app -f $(MAS_PKG_PATH) -t macos"

clean-mas:
	@rm -rf "$(MAS_BUILD_DIR)" "$(MAS_PKG_PATH)"

clean-dmg:
	@rm -f "$(DMG_PATH)"
	@rm -rf "$(DMG_STAGING_DIR)"

clean-app:
	@rm -rf "$(APP_DIR)" "$(MACOS_BUILD_DIR)"
