# Releasing Gloss

The GitHub repository is `garlab/gloss`, the crates.io package is `gloss-cli`, and
the executable and Homebrew formula are `gloss`. Application and shared
library versions are independent. The first published application version is `0.1.0`.

The release workflow publishes source to crates.io and an attested source archive
to GitHub Releases. The separate
[`garlab/homebrew-tap`](https://github.com/garlab/homebrew-tap) follows those GitHub
releases and builds Homebrew bottles automatically.

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
python3 -m unittest discover -s ci -p 'test_*.py'
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
| Ubuntu 24.04, x86-64 | Stable formatting, Clippy, Rust tests, terminal/pager tests, release helper tests, and publishing dry run |
| macOS 15, ARM64 | Stable formatting, Clippy, Rust tests, and terminal/pager tests |
| Windows Server 2022, x86-64 | Stable formatting, Clippy, Rust tests, and direct-output build |
| Ubuntu 24.04, x86-64, Rust 1.90 | Minimum supported Rust version, all targets |

This is the configured matrix, not a claim that a hosted run has already passed.
Require all jobs to pass for the release commit. Protocol tests do not replace
visual checks in real terminal emulators. The pager and watch mode require Unix.

## One-time release configuration

Enable GitHub Actions and immutable releases in `garlab/gloss`. Create an
environment named `release`, allowing deployment from `v*` tags. In the `gloss-cli`
crate's settings, configure
[Trusted Publishing](https://crates.io/docs/trusted-publishing):

| Setting | Value |
| --- | --- |
| Repository owner | `garlab` |
| Repository | `gloss` |
| Workflow filename | `release.yml` |
| Environment | `release` |

The initial `0.1.0` crate has been published. No long-lived registry credential
belongs in the repository or workflow configuration.

To notify the tap of a release, the `release` environment also needs the tap's
GitHub App: its ID in the `HOMEBREW_APP_ID` variable and its private key in the
`HOMEBREW_APP_PRIVATE_KEY` secret. Without them the release is unaffected and only
that job fails; the tap finds the release on its next scheduled check.

Enable immutable releases before the first release that uses them. Immutability
applies to releases published after it is enabled, the tap accepts no other kind,
and a published version cannot be replaced.

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

Each version also becomes an immutable GitHub release with these assets:

| Asset | Contents |
| --- | --- |
| `gloss-X.Y.Z-source.tar.gz` | Source files from the release commit, including `Cargo.lock` |
| `release.json` | Version tag, source commit, archive filename, and SHA-256 |
| `SHA256SUMS` | SHA-256 checksums of the archive and manifest |

These assets carry build-provenance attestations identifying the repository,
workflow, tag, and commit that produced them. Archives are built from committed
Git objects, so a rerun produces identical bytes.

A version already on crates.io is skipped before authentication, so failed runs
can be rerun without moving a published tag. Registry errors other than a missing
version fail the job. Publication across crates.io and GitHub is not atomic: a
retry resumes a draft release and refuses to replace an asset with different
bytes. If no run was created, enable Actions and manually run the
workflow against the existing tag, for example with GitHub CLI:

```sh
gh workflow run release.yml --ref vX.Y.Z
```

Manual runs against a branch are rejected. Fix code failures with a new version
once a version has been published; do not replace an existing release tag.

## Homebrew bottles

After publication, the release workflow notifies the tap, which also checks for
releases on an hourly schedule. The tap verifies the source archive's checksum and
build provenance against this repository, the release workflow, the tag, and the
commit, then opens a formula-update PR. It accepts only immutable stable releases.
The source includes `Cargo.lock`; Homebrew's Cargo helpers fetch dependencies
separately and install with the lockfile and network access disabled during the
build.

The tap builds and tests bottles on Apple Silicon macOS 26 and Linux x86-64.
Its publisher checks that the PR contains only the expected formula and release
receipt, validates both bottle artifacts, publishes them, and updates the tap.
Other supported platforms use a source build. Homebrew installs the executable
as `gloss`:

```sh
brew install garlab/tap/gloss
```

A version published manually with `cargo publish` has no attested release, so the
tap does not follow it. GitHub schedules can be delayed by several hours; if the
notification was missed, run **Update formulae** in the tap's Actions tab with
formula `gloss`, or use:

```sh
gh workflow run autobump.yml --repo garlab/homebrew-tap --ref main \
  -f formula=gloss -f tag=vX.Y.Z
```

Omit `tag` to select the latest stable version. Bottles can appear some time after
the source release while their builds finish, so wait for the tap to publish them
before announcing a Homebrew update. A retry reuses only matching release assets;
it does not overwrite different bottle bytes.

Version `0.1.0` predates attested releases, so it has a source-only formula.
Bottles start with `0.1.1`.

Formula build instructions, supported bottle platforms, credentials, and
recovery steps belong in the [tap's documentation](https://github.com/garlab/homebrew-tap#readme).
Keep formula changes there rather than maintaining another copy in this repo.
