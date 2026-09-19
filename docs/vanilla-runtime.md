# Vanilla installation and launch

The desktop backend now has a real runtime for **Vanilla**, **Fabric**,
**Quilt**, **Forge**, and **NeoForge**:

- **Vanilla**, **Fabric**, and **Quilt** instances are installable and launchable;
- version metadata comes from Mojang's `version_manifest_v2.json`;
- the client, libraries, asset index, assets, logging configuration and native
  archives are downloaded by Rust, with Mojang SHA-1 verification before use;
- shared immutable files live under `cache/minecraft`; worlds, screenshots,
  resource packs, mods and game logs remain inside the selected instance;
- native archives are extracted into that instance's `.newest/natives` folder;
- Fabric/Quilt loader profiles are fetched from their official metadata APIs,
  merged with the selected Vanilla profile, and their Maven libraries are
  verified with a profile SHA-1 or an official Maven `.sha1` sidecar;
- Forge and NeoForge are installed only by their respective official installer
  JARs in a dedicated `instances/<id>/runtime` Minecraft root. The launcher
  uses the supported client flags (`--installClient` for Forge and
  `--install-client` for NeoForge), checks the installer exit code, then
  validates its generated version JSON and every named loader library before
  recording the actual loader version in the instance;
- Modrinth mods, resource packs and shaders are resolved for the selected
  Minecraft version (and loader for mods), downloaded only from Modrinth's
  official CDN, verified against the published SHA-512, and saved into that
  instance's `mods`, `resourcepacks`, or `shaderpacks` directory;
- a failed or corrupt download is kept out of the final cache path;
- the Java executable is chosen from the instance setting, `JAVA_HOME`, then
  `PATH`, and is checked against the Java major version in Mojang metadata;
- if a compatible JVM is absent, the launcher obtains the matching Temurin JRE
  from Adoptium's official API, verifies the published SHA-256 before use, and
  keeps it under `cache/java` for subsequent launches.

To run a Vanilla, Fabric, Quilt, Forge, or NeoForge instance:

1. Create an instance with a supported loader and a version present in the
   official manifest.
2. Create or select a **Local / Offline** profile.
3. Click **Install** in the instance card, or **Install and play** on Home.
4. If Java is missing or too old, the launcher downloads a compatible Temurin
   JRE automatically. You may still set an absolute Java executable path in
   instance settings to use your own runtime instead.

The launcher invokes Java directly with the official main class and version
arguments. An Offline profile is passed as a local `legacy` session and stays
visibly marked `offline`; it is not a Microsoft token and must not be presented
as access to authenticated online servers.

For Forge and NeoForge, leave the loader version blank to select the latest
compatible release from the official Maven metadata, or specify a compatible
release explicitly. Installer stdout/stderr and its source URL are written to
`game/logs/newest-loader-install.log`; no account tokens are passed to or logged
by that process. A failed installer or verification leaves the loader marked as
not installed. Microsoft profiles remain blocked from launch until
the OAuth/Xbox/Minecraft entitlement flow described in
[`microsoft-auth.md`](microsoft-auth.md) is implemented.

To install content from Modrinth, select the target instance, open **Mods**,
**Resource packs**, or **Shaders**, and click **Install to instance**. The
launcher selects the latest compatible file; a mod requires a non-Vanilla
loader. Modpacks intentionally remain catalogue-only because installing an
`.mrpack` means creating and configuring a new instance rather than copying a
single game file.

Mojang's published metadata is the source of truth for the available version,
downloads, hashes, Java requirement, libraries and assets. The installer
accepts only the expected Mojang download hosts and rejects redirects or paths
outside its cache.
