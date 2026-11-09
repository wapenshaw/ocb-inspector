# Contributing to OCB Inspector

Thanks for helping. OCB Inspector reads, and experimentally writes, the files a BIOS loads its
overclocking settings from, so the project is careful about evidence: what was observed in a
real BIOS image or profile, what was inferred, and what is only proposed. Most of these
guidelines follow from that.

## Ways to help

- **Share a profile from another board.** Profiles open today only for Intel 600/700-series
  boards. [Open a profile report](https://github.com/wapenshaw/ocb-inspector/issues/new?template=profile_report.yml)
  with two `.ocb` files that differ in one setting you changed on purpose, plus the board and
  BIOS version. That is how support for a new board starts.
- **Report what you see.** If a setting's name, value or optimized default looks wrong next to
  what your BIOS screens show, [open a bug report](https://github.com/wapenshaw/ocb-inspector/issues/new?template=bug_report.yml)
  with the BIOS ID and the CLI output (`ocbi profile inspect --format json`).
- **Fix bugs and improve the app.** Open an issue first if you plan a larger change.

## Development setup

You need Rust (rustup installs the pinned toolchain from `rust-toolchain.toml`), Node.js 26 and
pnpm, and on Linux the [Tauri system packages](https://tauri.app/start/prerequisites/#linux).

```sh
cd ui && pnpm install && pnpm build && cd ..   # the desktop app embeds the built UI
cargo run -p ocbi-cli -- --help
cargo run -p ocbi-desktop
cd ui && pnpm dev                              # the UI alone in a browser
```

## Before you open a pull request

Run the offline checks from the repository root:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
(cd ui && pnpm typecheck && pnpm test)
python -I tools/third_party_notices.py --check   # after dependency changes; regenerate without --check
git diff --check
```

- Tests that need real BIOS images and profiles look for them in `fixtures/private/` and skip
  themselves when it is absent; CI runs the synthetic tests only. If your change touches
  decoding or writing, say which images and profiles you tested with.
- For UI changes, look at the screens you changed at the default window size in light and dark
  mode, and include a screenshot in the pull request.
- Update the documentation that describes current behaviour in the same pull request.

## Commits and pull requests

- Branch from `master`, named `<type>/<kebab-summary>`, such as `fix/compare-stale-pair`.
  Nobody commits to `master` directly; it only moves through pull requests.
- Commits follow [Conventional Commits](https://www.conventionalcommits.org):
  `<type>(<scope>): <subject>`, imperative and lowercase, no trailing period, 72 characters at
  most. Types in use are `feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `ci` and `chore`. The
  scope is an area (`core`, `firmware`, `format`, `engine`, `cli`, `desktop`, `ui`), or `deps`
  for a dependency update. The body explains **why**; the diff already says what. One logical
  change per commit, and never a co-author trailer.
- `feat`, `fix`, `perf` and `refactor` commits become the changelog, so write their subjects for
  someone who uses OCB Inspector. A dependency update is `chore(deps)` whatever it updates, so it
  cuts no release on its own.
- **Pull requests are merged by rebase.** Merge commits and squashing are disabled.
- A pull request needs the approval of a code owner ([.github/CODEOWNERS](.github/CODEOWNERS))
  and passing CI to merge. CI runs on its own only for the maintainer's pull requests; anyone
  else's waits for approval (a fork's from the pull request, a branch in this repository at the
  `Approval` job).
- Fill in every section of the [pull request template](.github/pull_request_template.md).
- Keep a pull request to one thing.

## Versions and releases

[release-please](https://github.com/googleapis/release-please) owns versions, tags and
`CHANGELOG.md`; do not edit them by hand. From the Conventional Commits on `master` it keeps a
release pull request open that bumps the version in `Cargo.toml`, `Cargo.lock`,
`ui/package.json` and `tauri.conf.json`. Merging it tags `vX.Y.Z` and opens a draft release; the
[release workflow](.github/workflows/release.yml) then builds the portable Windows and Linux
zips, attaches them with `SHA256SUMS` and signed build provenance, and publishes the release.

## Rules that keep written profiles safe

- **No bypasses.** No `--force`, raw-offset write or "ignore mismatch" option, in the CLI or the
  app. A profile is only ever paired with the BIOS image whose ID it records.
- **Writing stays narrow.** Only reviewed settings (`crates/core/src/catalog.rs`) can be written,
  only on approved images (`config/approved-images.toml`), and only with the experimental switch
  on. Displaying more settings never widens what can be written.
- **Preserve bytes.** Writing an unchanged profile must give identical bytes; an edit touches
  only the bytes of the settings it changes.
- **Say where facts come from.** A new board's layout or a setting's meaning needs a real BIOS
  image and real profiles, not a guess from another generation.

## What must never be committed

BIOS images, other people's `.ocb` profiles, anything extracted from vendor firmware beyond
short observations, build output and logs. The `.gitignore` covers the usual locations
(`fixtures/`, `analysis/`, `target/`). Code under licences incompatible with MIT (for example
GPL tools used during research) must not be copied into `crates/` or `ui/`.

## Licence

By contributing you agree that your contribution is licensed under the [MIT License](LICENSE),
the same as the project.
