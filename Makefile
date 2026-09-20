PROFILE ?= dev
FEATURES ?=
APP_NAME ?= Request Eagle
BUNDLE_ID ?= com.egortokarev.requesteagle
BUILD_DIR := target/$(if $(filter dev,$(PROFILE)),debug,$(PROFILE))
APP := $(BUILD_DIR)/$(APP_NAME).app
OS := $(shell uname -s)

.PHONY: build bundle run dev release clean-bundle

# Assemble an .app bundle so local runs get the real bundle identity
# (icon, name, Info.plist) instead of the bare-executable treatment.
bundle: build
	rm -rf "$(APP)"
	mkdir -p "$(APP)/Contents/MacOS" "$(APP)/Contents/Resources"
	cp "$(BUILD_DIR)/request-eagle" "$(APP)/Contents/MacOS/request-eagle"
	cp packaging/macos/AppIconEagleV2.icns "$(APP)/Contents/Resources/AppIconEagleV2.icns"
	VERSION=$$(sed -n 's/^version = "\(.*\)"/\1/p' crates/request-eagle/Cargo.toml | head -1); \
	BUILD_VERSION=$$(printf %s "$$VERSION" | tr -cd '0-9'); \
	sed -e "s/__VERSION__/$$VERSION/g" -e "s/__BUILD_VERSION__/$${BUILD_VERSION:-1}/g" \
		packaging/macos/Info.plist > "$(APP)/Contents/Info.plist"
	plutil -replace CFBundleDisplayName -string "$(APP_NAME)" "$(APP)/Contents/Info.plist"
	plutil -replace CFBundleName -string "$(APP_NAME)" "$(APP)/Contents/Info.plist"
	plutil -replace CFBundleIdentifier -string "$(BUNDLE_ID)" "$(APP)/Contents/Info.plist"
	plutil -lint "$(APP)/Contents/Info.plist"
	codesign --force --entitlements packaging/macos/entitlements.plist --sign - "$(APP)"

build:
	cargo build -p request-eagle --profile "$(PROFILE)" $(if $(FEATURES),--features "$(FEATURES)")

# Run in the foreground with logs in the terminal, bundling on macOS.
ifeq ($(OS),Darwin)
run: bundle
	"$(APP)/Contents/MacOS/request-eagle"
else
run: build
	./scripts/run.sh "$(BUILD_DIR)/request-eagle"
endif

# Rebuild an optimized app with GPUI's frame monitor whenever files change.
dev:
	+@./scripts/dev.sh

release:
	./scripts/build-macos.sh

clean-bundle:
	rm -rf "$(APP)"
