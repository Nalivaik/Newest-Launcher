# GitHub Actions Workflows

## Available Workflows

### Build Newest Launcher (`build.yml`)

Multi-platform release build workflow for:
- Linux x86_64 (amd64)
- Linux ARM64 (aarch64)
- Windows x86_64

**Trigger:** Manual (`workflow_dispatch`)

**How to run:**
1. GitHub → Actions → "Build Newest Launcher"
2. Click "Run workflow"
3. Select branch
4. Click "Run workflow" button

**Artifacts produced:**
- Linux .deb packages
- Linux AppImages
- Windows .exe installer
- Windows .msi installer (optional)

**Documentation:** See [../../docs/CI_CD_BUILD_GUIDE.md](../../docs/CI_CD_BUILD_GUIDE.md)

---

## Notes

- All builds run in parallel using matrix strategy
- Each build includes typecheck and cargo check before compilation
- Artifacts are automatically verified before upload
- No automatic release creation (manual release process for now)
