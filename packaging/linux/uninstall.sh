#!/usr/bin/env sh
set -eu

install_root=${XDG_DATA_HOME:-"$HOME/.local/share"}
config_root=${XDG_CONFIG_HOME:-"$HOME/.config"}/moeka
cache_root=${XDG_CACHE_HOME:-"$HOME/.cache"}/moeka
bin_dir=${HOME}/.local/bin
app_dir=${install_root}/applications
icon_root=${install_root}/icons/hicolor
icon_file=${icon_root}/256x256/apps/app.moeka.moeka.png
moeka_dir=${install_root}/moeka

purge=false
case "${1:-}" in
  "") ;;
  --purge) purge=true ;;
  *)
    printf 'Usage: %s [--purge]\n' "$0" >&2
    exit 2
    ;;
esac

rm -f "$bin_dir/moeka" "$bin_dir/moeka-uninstall"
rm -f "$app_dir/app.moeka.moeka.desktop" "$icon_file"
rm -rf "$moeka_dir"

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$app_dir" >/dev/null 2>&1 || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -f -t "$icon_root" >/dev/null 2>&1 || true
fi

if $purge; then
  rm -rf "$config_root" "$cache_root"
  printf 'Removed Moeka, settings and cache.\n'
else
  printf 'Removed Moeka. Settings remain in %s\n' "$config_root"
  printf 'Run this script with --purge to remove settings and cache too.\n'
fi
