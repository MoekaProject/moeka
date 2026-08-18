#!/usr/bin/env sh
set -eu

package_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
install_root=${XDG_DATA_HOME:-"$HOME/.local/share"}
bin_dir=${HOME}/.local/bin
app_dir=${install_root}/applications
icon_dir=${install_root}/icons/hicolor/256x256/apps

mkdir -p "$bin_dir" "$app_dir" "$icon_dir"
install -m 0755 "$package_dir/Moeka" "$bin_dir/moeka"
sed "s|^Exec=.*|Exec=$bin_dir/moeka %F|" \
  "$package_dir/app.moeka.moeka.desktop" > "$app_dir/app.moeka.moeka.desktop"
chmod 0644 "$app_dir/app.moeka.moeka.desktop"
install -m 0644 "$package_dir/app.moeka.moeka.png" "$icon_dir/app.moeka.moeka.png"

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$app_dir" >/dev/null 2>&1 || true
fi

printf 'Installed Moeka to %s\n' "$bin_dir/moeka"
printf 'The desktop launcher may take a moment to appear in the application menu.\n'
