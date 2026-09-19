# Build Instructions for Newest Launcher

## Linux Builds (Local)

### Prerequisites
- Node.js 20+
- Rust stable toolchain
- Linux development dependencies (already satisfied if build works)

### Build Commands
```bash
# Install dependencies
npm ci

# Build Linux release binaries
npm run desktop:build
```

### Output Files
- **DEB Package**: `target/release/bundle/deb/Newest Launcher_0.1.0_amd64.deb`
- **AppImage**: `target/release/bundle/appimage/Newest Launcher_0.1.0_amd64.AppImage`

## Multi-Platform Build (GitHub Actions)

For building on all supported platforms (Linux x86_64, Linux ARM64, Windows x86_64), 
use the GitHub Actions workflow.

### Steps
1. Push your changes to GitHub repository
2. Go to **Actions** tab in your repository
3. Select **Build Newest Launcher** workflow
4. Click **Run workflow** button
5. Wait for the build to complete (~10-20 minutes)
6. Download artifacts from the completed workflow run

### Output Artifacts

**Linux x86_64:**
- `Newest-Launcher-linux-x86_64-deb` - .deb package
- `Newest-Launcher-linux-x86_64-AppImage` - AppImage

**Linux ARM64:**
- `Newest-Launcher-linux-aarch64-deb` - .deb package
- `Newest-Launcher-linux-aarch64-AppImage` - AppImage

**Windows x86_64:**
- `Newest-Launcher-windows-x86_64-exe` - .exe installer
- `Newest-Launcher-windows-x86_64-msi` - .msi installer (optional)

📖 **Detailed CI/CD documentation**: See [docs/CI_CD_BUILD_GUIDE.md](docs/CI_CD_BUILD_GUIDE.md)

## Verification

### Linux DEB
```bash
dpkg-deb --info "target/release/bundle/deb/Newest Launcher_0.1.0_amd64.deb"
```

### Linux AppImage
```bash
file "target/release/bundle/appimage/Newest Launcher_0.1.0_amd64.AppImage"
# Should show: ELF 64-bit LSB pie executable
```

## Package Details
- **Product Name**: Newest Launcher
- **Version**: 0.1.0
- **Bundle Identifier**: io.newest.launcher
- **Architecture**: x86-64 (amd64)
