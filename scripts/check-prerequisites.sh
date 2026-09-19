#!/bin/sh
# Read-only prerequisite check. Local packages are opt-in; nothing is installed.
set -u

newest_project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
newest_failed=0
newest_local=0

case "${1:-}" in
  --local-sysroot) newest_local=1 ;;
  "") ;;
  *) printf 'Usage: sh scripts/check-prerequisites.sh [--local-sysroot]\n'; exit 2 ;;
esac

if [ -d "${HOME}/.cargo/bin" ]; then
  PATH="${HOME}/.cargo/bin:${PATH}"
  export PATH
fi

if [ "$newest_local" -eq 1 ]; then
  newest_sysroot="$newest_project_dir/.build-tools/sysroot"
  PKG_CONFIG_PATH="$newest_sysroot/usr/lib/x86_64-linux-gnu/pkgconfig:$newest_sysroot/usr/share/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
  export PKG_CONFIG_PATH
  printf 'Local development packages: %s\n' "$newest_sysroot"
fi

for newest_command in node npm cargo rustc cc pkg-config; do
  if command -v "$newest_command" >/dev/null 2>&1; then
    printf 'OK  %s: %s\n' "$newest_command" "$(command -v "$newest_command")"
  else
    printf 'MISSING  %s\n' "$newest_command"
    newest_failed=1
  fi
done

if [ "$(uname -s)" = Linux ] && command -v pkg-config >/dev/null 2>&1; then
  for newest_library in 'glib-2.0 >= 2.70' 'gtk+-3.0 >= 3.24' 'webkit2gtk-4.1 >= 2.40' 'javascriptcoregtk-4.1 >= 2.40' 'libsoup-3.0 >= 3.0'; do
    if pkg-config --exists "$newest_library"; then
      printf 'OK  %s\n' "$newest_library"
    else
      printf 'MISSING  %s\n' "$newest_library"
      newest_failed=1
    fi
  done
fi

if [ "$newest_failed" -ne 0 ]; then
  printf '\nSee docs/linux-build.md for installation and local-build instructions.\n'
  exit 1
fi

printf '\nBuild prerequisites found. This check does not compile the application.\n'
