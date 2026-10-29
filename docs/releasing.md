# Releasing Pulsar

Pulsar ships through three routes, all from one tag:

- the Inno Setup installer and portable zip, on GitHub Releases
- winget
- crates.io

The maintainer pushes the tag and publishes the GitHub release; workflows do the rest.

## One-time setup

- **crates.io:** on each crate's *Settings → Trusted Publishing* page, add this repository with the workflow `publish.yml` and the environment `crates-io`. No token is stored.
- **winget:** add a repository secret `WINGET_TOKEN`, a classic GitHub token with the `public_repo` scope. `wingetcreate` uses it to fork [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs) and open the pull request.

## 1. Prepare

1. Bump `version` in the root `Cargo.toml` (`[workspace.package]`).
2. Bump the matching `version = "X.Y.Z"` on the `pulsar-core` dependency in `crates/pulsar-monitor/Cargo.toml`.
3. Run `cargo build`, which updates `Cargo.lock`.
4. Commit as `Release vX.Y.Z`.
5. Run `scripts\package.ps1` (needs Inno Setup 7, which ships the Simplified Chinese messages) and go through [testing.md](testing.md), at least the installer section.
6. Run `cargo package --workspace` to check that both crates build from their packaged files alone.

## 2. Tag

Push the commit, then a tag `vX.Y.Z`. The **Release** workflow:

- runs the tests
- checks that the tag matches `Cargo.toml`
- builds the setup, portable zip and `SHA256SUMS.txt`, and attaches them to a **draft** release
- uploads the winget manifests as the workflow artifact `winget-manifests-vX.Y.Z`

## 3. Publish the GitHub release

Download the setup from the draft and install it, since it is this build that ships, not a local one. Review the generated notes and publish the draft. Installed copies announce the new version within a day.

Publishing starts the **Publish** workflow, which has one job for each of the routes below. A release marked as a pre-release starts neither.

## 4. crates.io

The `crates` job publishes `pulsar-monitor-core`, then `pulsar-monitor`, which depends on it. A crate whose version is already on crates.io is skipped, so the job can be run again after a failure.

A published version can never be replaced, only yanked, so do step 1's `cargo package --workspace` check first.

To publish by hand instead:

```
cargo publish -p pulsar-monitor-core
cargo publish -p pulsar-monitor
```

## 5. winget

The `winget` job writes the manifests from the release's `SHA256SUMS.txt` and opens a pull request to [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs) with `wingetcreate submit`. Microsoft's validation and a moderator's approval follow on that pull request.

To submit by hand instead, download the `winget-manifests-vX.Y.Z` artifact from the Release workflow run and add its folder as `manifests/o/oMaN-Rod/Pulsar/X.Y.Z/` in a pull request, or pass it to `wingetcreate submit`.

The installer's `AppId` GUID (`8F3B6C1E-3D8A-4B8E-9C61-5A2E7D4F0B19`) must never change; upgrades and winget rely on it.

## Code signing

Releases are unsigned for now. The SignPath Foundation signs open-source projects for free. Once the project is accepted, add its GitHub Action to `release.yml` between **Package** and **Draft the release**, and remove the SmartScreen note from the README.
