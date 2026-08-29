#!/bin/sh
set -eu

app_path=${1:-/Applications/Moeka.app}
executable="$app_path/Contents/MacOS/moeka"

if [ ! -x "$executable" ]; then
    printf 'Moeka executable not found: %s\n' "$executable" >&2
    printf 'Usage: %s [/path/to/Moeka.app]\n' "$0" >&2
    exit 1
fi

printf '%s\n' 'Moeka GPU diagnostic is enabled.'
printf '%s\n' 'Keep the mouse still for 30 seconds, move it over the editor for 30 seconds, then quit Moeka normally.'
printf '%s\n' 'The app will show the generated JSON timeline and CPU flamegraph paths when profiling stops.'

MOEKA_PROFILE=1 \
MOEKA_PROFILE_CPU=1 \
MOEKA_PROFILE_INTERVAL_MS=250 \
exec "$executable"
