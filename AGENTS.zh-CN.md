# 仓库指南

[English](AGENTS.md) | [繁體中文](AGENTS.zh-TW.md) | [Français](AGENTS.fr.md) | [日本語](AGENTS.ja.md) | [Español](AGENTS.es.md)

## 使命与参考

- 使用相邻的 .NET 仓库 `../MiniExcel` 作为兼容性参考，构建符合 Rust 惯例的 MiniExcel 行为实现。
- 实现等价行为前，检查对应的 .NET 公共 API、实现和聚焦测试。除非明确要求，否则不要修改该仓库。
- 将 [docs/compatibility.zh-CN.md](docs/compatibility.zh-CN.md) 视为支持范围记录，将 `tests/data/contracts/xlsx-parity-v1.json` 视为共享行为契约。

## 架构

- workspace 包含 `miniexcel`、`miniexcel-cli` 和 `miniexcel-wasm`；保持 `MiniExcel` 为主要公共 facade。
- 路径 `query` 与 `query_as` 必须保持有界内存流。优先使用顺序 ZIP/XML 扫描、小型 parser 状态和有界 channel。
- 使用结构化 XML、ZIP 和序列化 API。保持 workbook 顺序和 Excel 从 1 开始的公共坐标。
- 写入会创建新的 XLSX workbook；只有明确实现并测试的操作才能声称支持编辑现有文件。
- 支持 Rust 1.85.0 和 Edition 2024。禁止 unsafe Rust。

## 修改规则

- 复用现有模式和依赖，保持修改聚焦，并保留无关的工作树改动。
- 使用现有 fixture 添加聚焦回归测试。API 或支持边界变化时更新兼容性文档。
- 根目录 `AGENTS` 和本地化 `README` 固定维护六种语言：英文、`.zh-CN`、`.zh-TW`、`.fr`、`.ja`、`.es`。所有版本必须互链并同步更新；不得增加第七种语言。
- 其他英文 Markdown 必须有完整 `.zh-CN.md` 版本；创建或大幅修改时，也应更新已有的 `.zh-TW.md`、`.fr.md`、`.ja.md` 和 `.es.md` 版本。

## Browser Lab

- `web-demo` 不依赖后端，XLSX 数据通过 `miniexcel-wasm` 留在浏览器本地。
- 需要 Node.js 22、target `wasm32-unknown-unknown` 和 `wasm-bindgen-cli` 0.2.127。
- 在 `web-demo` 中运行 `npm run dev`，打开 `http://127.0.0.1:4173`。构建产物必须通过 HTTP 提供，不能使用 `file://`。
- 保持 Playwright 同时覆盖桌面和移动 viewport。

## 验证

第一次修改后运行范围最窄的测试，完成前运行适用检查：

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
cargo doc --workspace --no-deps --locked
```

等价行为变更必须同时运行 Rust 与 .NET 契约测试。Browser/WASM 变更还需在 `web-demo` 运行 `npm ci`、`npm run build` 和 `npm run test:e2e`。无法运行的检查必须说明。
