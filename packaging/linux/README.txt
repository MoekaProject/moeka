Moeka for Linux
===============

Run without installing:

  ./Moeka

Install for the current user (binary, application menu entry and icon):

  ./install.sh

Uninstall the application while keeping settings and cache:

  moeka-uninstall

Remove the application, settings and cache:

  moeka-uninstall --purge

The XCB/XKB libraries linked directly by Moeka are included in `lib/`, so the
archive starts without installing libxkbcommon-x11 separately. Ubuntu Desktop
still needs its normal graphics stack and fonts:

  sudo apt update
  sudo apt install libfontconfig1 libvulkan1 xdg-desktop-portal-gtk \
    fonts-dejavu-core fonts-noto-cjk wl-clipboard xclip

Debian uses the same package names. Fedora GNOME:

  sudo dnf install fontconfig vulkan-loader xdg-desktop-portal-gtk \
    google-noto-sans-cjk-fonts wl-clipboard xclip

Arch Linux GNOME:

  sudo pacman -S fontconfig vulkan-icd-loader xdg-desktop-portal-gtk \
    noto-fonts-cjk wl-clipboard xclip

On KDE Plasma, install `xdg-desktop-portal-kde` instead of the GTK portal.

The executable is architecture-specific. Use the arm64 archive on an ARM64
Ubuntu installation and the x86_64 archive on an Intel or AMD installation.
The build targets glibc 2.35 and supports Ubuntu 22.04+, Debian 12+, Fedora 36+
and current Arch Linux. Debian 11 and older distributions are not supported.

Build from source on Ubuntu Desktop:

  sudo apt update
  sudo apt install build-essential clang cmake patchelf pkg-config libasound2-dev \
    libfontconfig-dev libssl-dev libvulkan-dev libwayland-dev libxcb1-dev \
    libxcb-render0-dev libxcb-shape0-dev libxcb-shm0-dev libxcb-xfixes0-dev \
    libxcb-xkb-dev libxkbcommon-dev libxkbcommon-x11-dev
  make linux

Build from source on Fedora:

  sudo dnf install gcc-c++ clang cmake patchelf pkgconf-pkg-config alsa-lib-devel \
    fontconfig-devel openssl-devel vulkan-loader-devel wayland-devel libxcb-devel \
    libxkbcommon-devel libxkbcommon-x11-devel
  make linux

Build from source on Arch Linux:

  sudo pacman -S base-devel clang cmake patchelf pkgconf alsa-lib fontconfig \
    openssl vulkan-headers vulkan-icd-loader wayland libxcb libxkbcommon
  make linux

Cross-build both architectures from macOS:

  brew install zig zstd patchelf
  cargo install cargo-zigbuild --locked
  make linux

The first cross-build downloads a checksummed, minimal Ubuntu 22.04 XCB/XKB
sysroot into target/linux-sysroots. Later builds reuse it.

On macOS, `make linux` builds both architectures. On Linux, it builds the
native architecture only; use the matching archive for that machine.
