---
name: release-miniexcel
description: 'Prepare and publish a MiniExcel Rust release. Use when asked to release, bump the Rust version, create an N.N.0 tag, generate Git diff release notes with GitHub contributors, publish GUI or CLI downloads, or publish miniexcel to crates.io.'
argument-hint: 'Optional target version, for example 0.5.0'
user-invocable: true
---

# Release MiniExcel Rust

Use this workflow for repository releases. The skill prepares and verifies the release; the tag-triggered GitHub Actions workflow performs the irreversible GitHub and crates.io publication.

## Release Contract

- Derive the next version from the highest remote semantic-version tag by incrementing the minor component and setting the patch component to zero: `N.N.0`.
- Treat remote tags as authoritative. Fetch tags before calculating the version.
- Keep `Cargo.toml`, `Cargo.lock`, `web-demo/package.json`, and `web-demo/package-lock.json` on the same version.
- Generate notes from the complete previous-tag-to-release-tag Git diff and attribute each commit to its GitHub account.
- Publish a static Browser Lab ZIP, a Windows x64 CLI ZIP, and `SHA256SUMS.txt`.
- Publish the `miniexcel` crate through crates.io Trusted Publishing.
- Never move or overwrite an existing release tag and never attempt to republish an immutable crate version.

## One-Time Setup

Before the first automated crates.io release, configure the `miniexcel` crate under **Settings > Trusted Publishing**:

- Platform: GitHub
- Repository owner: `mini-software`
- Repository name: `MiniExcel-Rust`
- Workflow filename: `release.yml`
- Environment: `release`

Create the matching GitHub `release` environment and add appropriate protection rules. No long-lived crates.io token is required.

## Procedure

1. Read `AGENTS.md` and inspect `git status`. Preserve unrelated worktree changes. If the tree is dirty, use an isolated worktree or stop before changing overlapping files.
2. Fetch remote tags and determine the highest stable `vN.N.0` tag. Confirm the corresponding GitHub release and crates.io version.
3. Calculate the requested target. Unless the user explicitly supplies a valid higher version, increment the previous minor version and set patch to zero.
4. Inspect `git diff <previous-tag>..HEAD`, commit subjects, and GitHub commit authors. Summarize behavior from the actual diff rather than commit subjects alone.
5. Update all four version metadata files listed in the release contract. Do not edit unrelated files.
6. Run the focused check first:

   ```powershell
   cargo run -p miniexcel-cli --locked -- --version
   ```

7. Run the applicable repository validation:

   ```powershell
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
   cargo test --workspace --all-targets --all-features --locked
   cargo doc --workspace --no-deps --all-features --locked
   npm --prefix web-demo ci
   npm --prefix web-demo run build
   npm --prefix web-demo run test:e2e
   ```

8. Commit only the release metadata with `Release MiniExcel Rust vN.N.0`, push `main`, and wait for the commit's Rust and Pages workflows to succeed.
9. Create an annotated `vN.N.0` tag at that exact commit and push only that tag. Do not run `cargo publish` or `gh release create` manually; `.github/workflows/release.yml` owns publication.
10. Monitor the `Release` workflow. It validates the tag/version contract, builds both downloads, publishes crates.io, generates diff-based notes through `scripts/new-release-notes.ps1`, and creates the GitHub release.
11. Verify all remote outputs:

   ```powershell
   gh release view vN.N.0 -R mini-software/MiniExcel-Rust
   gh release download vN.N.0 -R mini-software/MiniExcel-Rust
   cargo search miniexcel --limit 1
   ```

12. Confirm the remote tag target, release asset digests, downloaded checksums, crates.io version, publisher account, and clean branch state.

## Failure Handling

- If validation fails, fix the release commit before creating a tag.
- If Trusted Publishing is not configured, configure the exact repository, `release.yml` workflow, and `release` environment on crates.io, then rerun the failed workflow.
- If crates.io publication succeeds but GitHub release creation fails, rerun the same workflow. It detects the existing crate, verifies existing asset digests, and uploads only missing assets.
- If a tag or crate version already exists at a different commit, stop and report the conflict. Do not delete, force-push, yank, or replace it without explicit approval.

## Supporting Files

- Publication workflow: [release.yml](../../workflows/release.yml)
- Release-note generator: [new-release-notes.ps1](../../../scripts/new-release-notes.ps1)
- Chinese instructions: [SKILL.zh-CN.md](./SKILL.zh-CN.md)