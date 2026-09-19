# Linux desktop build

Newest Launcher uses Tauri 2 and the system WebKitGTK webview. Install the
prerequisites before building the desktop application. The browser preview does
not provide desktop filesystem, Java, or process functionality.

## Standard Ubuntu / Debian setup

The packages below follow the [official Tauri prerequisites](https://v2.tauri.app/start/prerequisites/).
They require administrator access and are installed by the user:

```sh
sudo apt update
sudo apt install build-essential curl wget file pkg-config libssl-dev \
  libwebkit2gtk-4.1-dev libgtk-3-dev libxdo-dev \
  libayatana-appindicator3-dev librsvg2-dev
```

Install stable Rust using [rustup](https://rustup.rs/) if Cargo is not available:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
. "$HOME/.cargo/env"
```

Install a current Node.js LTS release, then run from the project directory:

```sh
sh scripts/check-prerequisites.sh
npm ci
npm run build
npx tauri build
```

This project uses a Cargo workspace, so the default Rust target directory is
`target` at the project root. Tauri places distributable packages under
`target/release/bundle`. A successful web build alone does not verify a native
build.

## Local development packages in this workspace

This Ubuntu 24.04 workspace already had GTK, WebKitGTK, JavaScriptCoreGTK,
libsoup and related runtime libraries, but lacked their development packages.
The development packages and their missing dependencies were downloaded from
the configured official Ubuntu repositories with `apt-get download`, then
extracted using `dpkg-deb --extract` into `.build-tools/sysroot`. No packages
were installed system-wide and no administrator privileges were used.

The extracted `.pc` files point to this workspace's local include/library
directories. Development symlinks resolve to the installed system runtime
libraries. This directory is a machine-local build aid, not an application
dependency to commit or distribute. If the workspace is moved, recreate the
local packages or use the standard system setup above.

The setup was checked by compiling and running a small native program against
the actual GTK and WebKit headers and shared libraries. This verifies that the
local development packages can link to the installed runtimes; the application
must still pass its own build and runtime checks.

Rust was installed at `$HOME/.cargo/bin`, without changing shell profiles. For
this workspace only, activate the development packages before building:

```sh
. "$HOME/.cargo/env"
newest_sysroot="$PWD/.build-tools/sysroot"
export PKG_CONFIG_PATH="$newest_sysroot/usr/lib/x86_64-linux-gnu/pkgconfig:$newest_sysroot/usr/share/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
sh scripts/check-prerequisites.sh --local-sysroot
npm run build
npx tauri build
```

Use a graphical desktop session to run the application. Headless servers can
compile and run core tests, but opening a GTK window requires a display. Build
Windows and macOS installers on their respective platforms; a Linux build
does not verify those targets.
