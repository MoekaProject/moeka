Moeka for Linux
===============

Run without installing:

  ./Moeka

Install for the current user (binary, application menu entry and icon):

  ./install.sh

Ubuntu Desktop 22.04 runtime packages:

  sudo apt update
  sudo apt install libasound2 libfontconfig1 libssl3 libvulkan1 \
    libwayland-client0 libxcb1 libxcb-shape0 libxcb-xfixes0 \
    libxkbcommon0 libxkbcommon-x11-0 xdg-desktop-portal-gtk \
    fonts-dejavu-core fonts-noto-cjk wl-clipboard xclip

On Ubuntu 24.04 and later, the corresponding ALSA and OpenSSL packages may be
named libasound2t64 and libssl3t64. Ubuntu Desktop normally installs them as
dependencies already.

The executable is architecture-specific. Use the arm64 archive on an ARM64
Ubuntu installation and the x86_64 archive on an Intel or AMD installation.

Build from source on Ubuntu Desktop:

  sudo apt update
  sudo apt install build-essential clang cmake pkg-config libasound2-dev \
    libfontconfig-dev libssl-dev libvulkan-dev libwayland-dev libxcb1-dev \
    libxcb-render0-dev libxcb-shape0-dev libxcb-shm0-dev libxcb-xfixes0-dev \
    libxcb-xkb-dev libxkbcommon-dev libxkbcommon-x11-dev
  make linux