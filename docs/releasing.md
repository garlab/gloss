# Releasing Gloss

The GitHub repository is `garlab/gloss`, the crates.io package is `gloss-cli`, and
the executable and Homebrew formula are `gloss`. Application and shared
library versions are independent. The first published application version is `0.1.0`.

The release workflow publishes source to crates.io. The separate
[`garlab/homebrew-tap`](https://github.com/garlab/homebrew-tap) follows the registry
and builds Homebrew bottles automatically.

## Checks before a release

Use a clean checkout with Rust 1.90 or newer, a C compiler, and Python 3 for Unix
terminal tests. Run commands from the repository root so Cargo does not discover
another checkout's local overrides. Remove any `[patch.crates-io]` overrides
before release checks; dependencies must resolve from the registry with the
checksums in `Cargo.lock`.

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo +1.90.0 check --locked --all-targets
cargo build --release --locked
python3 tests/terminal_smoke.py target/release/gloss
python3 tests/pager_smoke.py target/release/gloss
cargo package --list --locked
cargo publish -p gloss-cli --locked --dry-run
```

The Python checks require Unix. Review the package file list: source, tests and
small fixtures, Cargo metadata, README, changelog, and license belong in the
archive. Local configuration, credentials, generated output, and benchmark
corpora do not. Cargo's dry run verifies the packaged source against registry
dependencies without uploading; see the [Cargo publishing guide](https://doc.rust-lang.org/cargo/reference/publishing.html).

For rendering or dependency changes, check appearance in a graphics-capable
terminal and compare relevant latency, memory, binary size, and output costs.
Use the [fixed baseline](performance-baseline.md) and existing benchmark tools;
keep generated inputs and measurements outside the repository.

The [CI workflow](../.github/workflows/ci.yml) defines this platform matrix:

| Runner | Checks |
| --- | --- |
| Ubuntu 24.04, x86-64 | Stable formatting, Clippy, Rust tests, terminal/pager tests, and publishing dry run |
| macOS 15, ARM64 | Stable formatting, Clippy, Rust tests, and terminal/pager tests |
| Windows Server 2022, x86-64 | Stable formatting, Clippy, Rust tests, and direct-output build |
| Ubuntu 24.04, x86-64, Rust 1.90 | Minimum supported Rust version, all targets |

This is the configured matrix, not a claim that a hosted run has already passed.
Require all jobs to pass for the release commit. Protocol tests do not replace
visual checks in real terminal emulators. The pager and watch mode require Unix.

## One-time release configuration

Enable GitHub Actions in `garlab/gloss`. Create an environment named `release`,
allowing deployment from `v*` tags. In the `gloss-cli` crate's settings, configure
[Trusted Publishing](https://crates.io/docs/trusted-publishing):

| Setting | Value |
| --- | --- |
| Repository owner | `garlab` |
| Repository | `gloss` |
| Workflow filename | `release.yml` |
| Environment | `release` |

The initial `0.1.0` crate has been published. No long-lived registry credential
belongs in the repository or workflow configuration. Homebrew uses credentials
configured in the tap; it requires no additional secret in this repository.

## Publishing a version

Update the package version in `Cargo.toml`, refresh `Cargo.lock` with
`cargo check`, and add dated release notes in `CHANGELOG.md`. Run the checks
above, review the diff, commit, and push `main`. Once CI passes, create and push
a signed `vX.Y.Z` tag on that commit.

The [release workflow](../.github/workflows/release.yml) requires the tag to match
the manifest version and its commit to be on `main`. It reruns the complete CI
matrix and package verification before the publishing job can obtain a short-lived
crates.io token. That job skips compilation because the package has already been
verified. It does not build with a publishing credential in the environment.

A version already on crates.io is skipped before authentication, so failed runs
can be rerun without moving a published tag. Registry errors other than a missing
version fail the job. If no run was created, enable Actions and manually run the
workflow against the existing tag, for example with GitHub CLI:

```sh
gh workflow run release.yml --ref vX.Y.Z
```

Manual runs against a branch are rejected. Fix code failures with a new version
once a version has been published; do not replace an existing release tag.

## Homebrew bottles

The tap checks the crates.io index on its existing hourly schedule. After a
stable, non-yanked `gloss-cli` version appears, it downloads the `.crate` archive,
verifies its SHA-256 against the index, and opens a formula-update PR. The source
includes `Cargo.lock`; Homebrew's Cargo helpers fetch dependencies separately
and install with the lockfile and network access disabled during the build.

The tap builds and tests bottles on Apple Silicon macOS 26 and Linux x86-64.
Its publisher checks that the PR contains only the expected formula and release
receipt, validates both bottle artifacts, publishes them, and updates the tap.
Other supported platforms use a source build. Homebrew installs the executable
as `gloss`:

```sh
brew install garlab/tap/gloss
```

The scheduled updater also covers versions published manually with
`cargo publish`. GitHub schedules can be delayed; to start an update immediately,
run **Update formulae** in the tap's Actions tab with formula `gloss`, or use:

```sh
gh workflow run autobump.yml --repo garlab/homebrew-tap --ref main \
  -f formula=gloss -f tag=vX.Y.Z
```

Omit `tag` to select the latest stable version. For the initial setup, push the
tap automation and run it with `tag=v0.1.0`; the bot creates the first formula
from a reviewed template and publishes its bottles. No new Gloss version or
replacement tag is needed. Wait for that tap run to finish before announcing
Homebrew availability. A retry reuses only matching release assets; it does not
overwrite different bottle bytes.

Formula build instructions, supported bottle platforms, credentials, and
recovery steps belong in the [tap's documentation](https://github.com/garlab/homebrew-tap#readme).
Keep formula changes there rather than maintaining another copy in this repo.
