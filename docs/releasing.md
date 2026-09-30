# Releasing Pulsar

Pulsar ships through three routes, all from one tag:

- the Inno Setup installer and portable zip, on GitHub Releases
- winget
- crates.io

Only the maintainer pushes, publishes and submits.

## 1. Prepare

1. Bump `version` in the root `Cargo.toml` (`[workspace.package]`).
2. Bump the matching `version = "X.Y.Z"` on the `pulsar-core` dependency in `crates/pulsar-monitor/Cargo.toml`.
3. Run `cargo build`, which updates `Cargo.lock`.
4. Commit as `Release vX.Y.Z`.
5. Run `scripts\package.ps1` and go through [testing.md](testing.md), at least the installer section.
6. Run `cargo package --workspace` to check that both crates build from their packaged files alone.

## 2. Tag

Push the commit, then a tag `vX.Y.Z`. The **Release** workflow:

- runs the tests
- checks that the tag matches `Cargo.toml`
- builds the setup, portable zip and `SHA256SUMS.txt`, and attaches them to a **draft** release
- uploads the winget manifests as the workflow artifact `winget-manifests-vX.Y.Z`

## 3. Publish the GitHub release

Review the generated notes on the draft and publish it. Installed copies announce the new version within a day.

## 4. crates.io

Publish the library first, because the app depends on it:

```
cargo publish -p pulsar-monitor-core
cargo publish -p pulsar-monitor
```

A published version can never be replaced, only yanked, so do step 1's `cargo package --workspace` check first.

## 5. winget

Download the `winget-manifests-vX.Y.Z` artifact from the workflow run, or run `scripts\winget.ps1 -Version X.Y.Z` after `package.ps1`. It holds `oMaN-Rod.Pulsar.yaml`, `.installer.yaml` and `.locale.en-US.yaml`, with the installer URL and SHA256 filled in.

To submit, use either route:

- `wingetcreate submit <folder>` (needs a GitHub token)
- a pull request to [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs) adding the folder as `manifests/o/oMaN-Rod/Pulsar/X.Y.Z/`

The installer manifest looks like this:

```yaml
PackageIdentifier: oMaN-Rod.Pulsar
PackageVersion: X.Y.Z
Platform:
  - Windows.Desktop
MinimumOSVersion: 10.0.22000.0
InstallerType: inno
Scope: user
InstallModes:
  - interactive
  - silent
  - silentWithProgress
UpgradeBehavior: install
Installers:
  - Architecture: x64
    InstallerUrl: https://github.com/oMaN-Rod/Pulsar/releases/download/vX.Y.Z/pulsar-X.Y.Z-setup.exe
    InstallerSha256: <from SHA256SUMS.txt>
ManifestType: installer
ManifestVersion: 1.6.0
```

The installer's `AppId` GUID (`8F3B6C1E-3D8A-4B8E-9C61-5A2E7D4F0B19`) must never change; upgrades and winget rely on it.

## Code signing

Releases are unsigned for now. The SignPath Foundation signs open-source projects for free. Once the project is accepted, add its GitHub Action to `release.yml` between **Package** and **Draft the release**, and remove the SmartScreen note from the README.
