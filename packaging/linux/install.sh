#!/usr/bin/env sh
set -eu

package_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
install_root=${XDG_DATA_HOME:-"$HOME/.local/share"}
bin_dir=${HOME}/.local/bin
app_dir=${install_root}/applications
icon_dir=${install_root}/icons/hicolor/256x256/apps
moeka_dir=${install_root}/moeka

mkdir -p "$bin_dir" "$app_dir" "$icon_dir" "$moeka_dir"
install -m 0755 "$package_dir/Moeka" "$moeka_dir/Moeka"
rm -rf "$moeka_dir/lib"
cp -R "$package_dir/lib" "$moeka_dir/lib"
install -m 0755 "$package_dir/uninstall.sh" "$moeka_dir/uninstall.sh"
cat > "$bin_dir/moeka" <<EOF
#!/usr/bin/env sh
exec "$moeka_dir/Moeka" "\$@"
EOF
chmod 0755 "$bin_dir/moeka"
cat > "$bin_dir/moeka-uninstall" <<EOF
#!/usr/bin/env sh
exec "$moeka_dir/uninstall.sh" "\$@"
EOF
chmod 0755 "$bin_dir/moeka-uninstall"
sed "s|^Exec=.*|Exec=$bin_dir/moeka %F|" \
  "$package_dir/app.moeka.moeka.desktop" > "$app_dir/app.moeka.moeka.desktop"
chmod 0644 "$app_dir/app.moeka.moeka.desktop"
install -m 0644 "$package_dir/app.moeka.moeka.png" "$icon_dir/app.moeka.moeka.png"

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$app_dir" >/dev/null 2>&1 || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -f -t "$install_root/icons/hicolor" >/dev/null 2>&1 || true
fi

printf 'Installed Moeka to %s\n' "$moeka_dir"
printf 'Uninstall with: %s\n' "$bin_dir/moeka-uninstall"
printf 'The desktop launcher may take a moment to appear in the application menu.\n'
