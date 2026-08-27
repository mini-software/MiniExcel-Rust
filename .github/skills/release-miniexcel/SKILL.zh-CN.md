# 发布 MiniExcel Rust

本流程用于仓库发布。Skill 负责准备和验证版本；由标签触发的 GitHub Actions 工作流负责执行不可逆的 GitHub 与 crates.io 发布。

## 发布约定

- 以远端最高语义版本标签为准，将次版本号加一，并把修订号固定为零：`N.N.0`。
- 计算版本前必须抓取远端标签。
- `Cargo.toml`、`Cargo.lock`、`web-demo/package.json` 与 `web-demo/package-lock.json` 必须使用相同版本。
- 发布说明必须来自上一个标签到发布标签之间的完整 Git diff，并将每个提交归属到对应 GitHub 账号。
- 发布静态 Browser Lab ZIP、Windows x64 CLI ZIP 与 `SHA256SUMS.txt`。
- 通过 crates.io Trusted Publishing 发布 `miniexcel` crate。
- 不得移动或覆盖已有发布标签，也不得尝试重新发布 crates.io 上不可变的同版本 crate。

## 一次性配置

首次自动发布 crates.io 前，在 `miniexcel` crate 的 **Settings > Trusted Publishing** 中配置：

- 平台：GitHub
- 仓库所有者：`mini-software`
- 仓库名称：`MiniExcel-Rust`
- 工作流文件名：`release.yml`
- Environment：`release`

同时创建对应的 GitHub `release` environment，并设置适当的保护规则。不需要保存长期 crates.io token。

## 执行步骤

1. 阅读 `AGENTS.md` 并检查 `git status`。保留无关的工作区改动；若工作区不干净，应使用隔离 worktree，或在修改重叠文件前停止。
2. 抓取远端标签并确定最高稳定 `vN.N.0` 标签，同时确认对应 GitHub Release 与 crates.io 版本。
3. 计算目标版本。除非用户明确指定更高且有效的版本，否则将上一个次版本号加一，并把修订号设为零。
4. 检查 `git diff <previous-tag>..HEAD`、提交主题与 GitHub 提交作者。必须根据实际 diff 总结行为，不能只依赖提交主题。
5. 只更新发布约定中列出的四个版本元数据文件，不修改无关文件。
6. 首先执行聚焦检查：

   ```powershell
   cargo run -p miniexcel-cli --locked -- --version
   ```

7. 执行适用的仓库验证：

   ```powershell
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
   cargo test --workspace --all-targets --all-features --locked
   cargo doc --workspace --no-deps --all-features --locked
   npm --prefix web-demo ci
   npm --prefix web-demo run build
   npm --prefix web-demo run test:e2e
   ```

8. 只提交发布元数据，提交信息使用 `Release MiniExcel Rust vN.N.0`；推送 `main` 后，等待该提交的 Rust 与 Pages 工作流成功。
9. 在该精确提交上创建带说明的 `vN.N.0` 标签，并只推送此标签。不要手动执行 `cargo publish` 或 `gh release create`；发布由 `.github/workflows/release.yml` 统一负责。
10. 监视 `Release` 工作流。它会验证标签和版本约定、构建两个下载包、发布 crates.io、通过 `scripts/new-release-notes.ps1` 生成基于 diff 的说明，并创建 GitHub Release。
11. 验证所有远端产物：

   ```powershell
   gh release view vN.N.0 -R mini-software/MiniExcel-Rust
   gh release download vN.N.0 -R mini-software/MiniExcel-Rust
   cargo search miniexcel --limit 1
   ```

12. 确认远端标签目标、发布资产摘要、下载校验和、crates.io 版本、发布者账号以及干净的分支状态。

## 失败处理

- 验证失败时，必须在创建标签前修复发布提交。
- Trusted Publishing 尚未配置时，应在 crates.io 上配置精确的仓库、`release.yml` 工作流和 `release` environment，然后重新运行失败的工作流。
- crates.io 已发布但 GitHub Release 创建失败时，重新运行同一工作流；它会检测已有 crate、验证现有资产摘要，并只上传缺失资产。
- 标签或 crate 版本已存在但指向不同提交时，立即停止并报告冲突。未经明确批准，不得删除、强推、yank 或替换。

## 支持文件

- 发布工作流：[release.yml](../../workflows/release.yml)
- 发布说明生成器：[new-release-notes.ps1](../../../scripts/new-release-notes.ps1)
- 英文 Skill：[SKILL.md](./SKILL.md)