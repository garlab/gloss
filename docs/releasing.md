# Releasing Gloss

The GitHub repository is `garlab/gloss`, the crates.io package is `gloss-cli`, and
the executable and planned Homebrew formula are `gloss`. Application and shared
library versions are independent. The initial application version is `0.1.0`.

This setup publishes source to crates.io. Prebuilt executables and Homebrew
bottles are separate follow-up work.

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

## First publication

1. Create the public `garlab/gloss` GitHub repository with `main` as its default
   branch. If this checkout has no remote, add it:

   ```sh
   git remote add origin git@github.com:garlab/gloss.git
   ```

2. Review the `0.1.0` changelog, replace its `unreleased` marker with the release
   date, and replace the README's first-release notice with the registry install
   command below. Commit the reviewed tree, push `main`, and wait for CI to pass.
   Enable Actions if the repository's settings disable workflows.

   ```sh
   cargo install gloss-cli --locked
   ```

3. Publish the first version manually from that exact clean commit. Use your
   local crates.io authentication, then run:

   ```sh
   cargo publish -p gloss-cli --locked --dry-run
   cargo publish -p gloss-cli --locked
   ```

   Check `git status --short` before publishing and confirm the manifest version
   is `0.1.0`. The first manual release creates the crate so its owner can
   configure [Trusted Publishing](https://crates.io/docs/trusted-publishing).
   No registry credential belongs in the repository or workflow configuration.

4. Create a GitHub environment named `release`, allowing deployment from `v*`
   tags. On the crate's settings page, add a GitHub Trusted Publisher with:

   | Setting | Value |
   | --- | --- |
   | Repository owner | `garlab` |
   | Repository | `gloss` |
   | Workflow filename | `release.yml` |
   | Environment | `release` |

5. Tag the published commit and push that tag:

   ```sh
   git tag -s v0.1.0 -m 'Release 0.1.0'
   git push origin v0.1.0
   ```

   The release workflow verifies the tag and reruns CI. Because `0.1.0` already
   exists, it skips authentication and upload. Later versions use Trusted
   Publishing. Confirm installation from the registry with
   `cargo install gloss-cli --version 0.1.0 --locked`, then `gloss --version`.

## Subsequent releases

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

## Homebrew handoff

Keep the formula in the separate tap. Once the source tag exists, use its archive
URL, `https://github.com/garlab/gloss/archive/refs/tags/vX.Y.Z.tar.gz`, and compute
its actual SHA-256. The formula class is `Gloss`. Use Homebrew's
[Cargo helpers](https://docs.brew.sh/Language-Specific-Formulae#rust), which retain
the lockfile and separate dependency fetching from installation:

```ruby
depends_on "rust" => :build

def fetch
  system "cargo", "fetch", *std_cargo_fetch_args
end

def install
  system "cargo", "install", *std_cargo_args
end

test do
  assert_match version.to_s, shell_output("#{bin}/gloss --version")
  (testpath/"sample.md").write("# Hello\n\nA **small** document.\n")
  assert_match "Hello", shell_output("#{bin}/gloss --output text sample.md")
end
```

This is an excerpt, not a complete formula: supply the description, homepage,
archive URL, checksum, and MIT license in the tap. Test the formula there before
advertising its installation command. This release setup does not change the
tap or create bottles.
