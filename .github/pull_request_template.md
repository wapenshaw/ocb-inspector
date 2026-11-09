## Summary

<!--
The smallest view that makes the point: a call tree, a diff sketch or a
screenshot, each beside one or two lines of why.
-->

Closes #

## Evidence

<!--
A before and an after: a failing then passing test, the screen before and
after, the CLI output. For anything that writes profiles: which BIOS images
and profiles, and whether the result was loaded on hardware ("not tested on
hardware" is a fine answer).
-->

- **Before:**
  **After:**

## Merge Danger

<!--
A two-way door is cheap to walk back; a one-way door is not: a change to the
bytes a written profile contains, a widened write path or approval, a JSON
contract change, a renamed release asset.
-->

**Door:** <!-- one-way or two-way -->

**Blast Radius:** <!-- one word: what could break, and for whom -->

## Before merging

- [ ] `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`
      and `cargo test --workspace --locked` pass locally.
- [ ] `pnpm typecheck` and `pnpm test` pass in `ui/` (UI changes).
- [ ] `python -I tools/third_party_notices.py --check` passes (dependency changes).
- [ ] One logical change per commit, Conventional Commits, no co-author
      trailer. **PRs are rebased, never squashed or merged**, so the commit
      messages are what lands in the history and in the changelog.
- [ ] Tests land with the behaviour they cover.
- [ ] `CHANGELOG.md`, the tags and the versions in `Cargo.toml`, `Cargo.lock`,
      `ui/package.json` and `tauri.conf.json` are untouched; they belong to release-please.
- [ ] Documentation of current behaviour is updated.
- [ ] This pull request is one thing.
