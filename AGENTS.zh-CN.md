# 仓库指南

[English](AGENTS.md)

## 使命

- 使用 .NET MiniExcel 项目作为兼容性参考，构建符合 Rust 习惯的 MiniExcel 行为实现。
- 在保持 Rust 性能、内存安全和 API 惯例的同时匹配可观察行为；不要逐字翻译 .NET 内部实现。
- 将 [docs/compatibility.zh-CN.md](docs/compatibility.zh-CN.md) 视为当前范围与架构记录。不要声称尚未支持的接口已实现等价行为。

## 参考仓库

- .NET 参考代码通常位于相邻仓库 `../MiniExcel`（主要 Windows 开发机上为 `D:\git\MiniExcel`）。
- 实现等价行为前，先检查对应的 .NET 公共 API、控制该行为的实现以及聚焦测试。
- 除非任务明确要求，否则不要修改 .NET 仓库。
- `tests/data/contracts/xlsx-parity-v1.json` 是共享行为契约。修改所覆盖的行为时，应有意识地更新该文件并验证两个适配器。

## 架构

- workspace 包含 `miniexcel`（核心库）、`miniexcel-cli`（CLI）和 `miniexcel-wasm`（浏览器适配器）。
- 保持 `MiniExcel` 为主要公共 facade。除非确实需要公共抽象，否则 parser、reader、writer 和具体迭代器类型都应保持内部可见。
- 基于路径的 `query` 和 `query_as` 必须保持有界内存流。不要保留完整 worksheet XML 或全部行。
- 优先使用顺序 ZIP/XML 扫描、小型 parser 状态和有界 channel，而不是物化整个 workbook。字节数组 API 可在契约要求时物化结果。
- 使用结构化 XML、ZIP 和序列化 API。当已有 parser 或类型化 helper 可用时，不要临时拼接或解析字符串。
- 保持 workbook 顺序和 Excel 坐标语义。公共索引从 1 开始；内部 cell 坐标可以从 0 开始。
- 写入操作创建新的 XLSX workbook。除非已明确实现并测试，否则不要暗示支持编辑现有文件。

## 实现规则

- 支持 Rust 1.85.0 和 Edition 2024。不要使用高于声明 MSRV 的功能。
- 整个 workspace 禁止使用 unsafe Rust。
- 添加抽象或 package 前，优先复用现有 crate 模式和依赖。
- 保持修改聚焦。不要重写无关代码，也不要覆盖工作树中已有的修改。
- 尽可能使用现有 fixture 添加回归测试。只有当两个适配器都覆盖某项行为时，才将共享的 .NET/Rust 预期写入等价契约。
- 支持边界或 API 改变时，更新兼容性矩阵和公共文档。
- 每份英文 Markdown 文档都必须维护完整的 `.zh-CN.md` 对应版本并提供双向语言链接；修改时同步更新两个版本。

## 纯前端 Browser Lab

- `web-demo` 是不依赖后端的静态应用。XLSX 解析和生成通过 `miniexcel-wasm` 在浏览器本地运行；除非明确要求，否则不要引入服务器 API 或上传 workbook 数据。
- 前置条件为 Node.js 22、`wasm32-unknown-unknown` Rust target，以及 0.2.127 版 `wasm-bindgen-cli`：

```bash
rustup target add wasm32-unknown-unknown --toolchain 1.85.0
cargo +stable install wasm-bindgen-cli --version 0.2.127 --locked
```

- 本地构建和运行时，在 `web-demo` 中执行 `npm run dev`，然后打开 `http://127.0.0.1:4173`：

```bash
cd web-demo
npm ci
npm run dev
```

- `npm run build` 会在 `web-demo/dist` 中生成可部署的静态站点，包括 HTML、CSS、JavaScript 和 WebAssembly 资源。它可以直接部署到 GitHub Pages 或其他静态托管服务；运行时不需要 .NET 或 Rust 服务器进程。
- 使用 `npm run serve` 或其他静态服务器通过 HTTP 提供 `dist`。不要依赖通过 `file://` 打开 `dist/index.html`，因为浏览器 module 和 WebAssembly 加载需要正确的 HTTP 行为与 MIME type。
- 保持浏览器测试同时覆盖桌面和移动 viewport。当前 Playwright 配置会自动启动本地静态服务器。

## 验证

第一次实质性修改后立即运行范围最窄的相关测试。完成前，从仓库根目录运行适用的 CI 检查：

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
cargo doc --workspace --no-deps --locked
```

涉及共享 .NET 等价行为的修改，还需运行两个适配器：

```bash
cargo +1.85.0 test -p miniexcel --test parity_contract --locked
dotnet test ../MiniExcel/tests/MiniExcel.OpenXml.Tests/MiniExcel.OpenXml.Tests.csproj --framework net10.0 --filter "FullyQualifiedName~RustParityContractTests"
```

涉及浏览器或 WASM 的修改，还需在 `web-demo` 中运行：

```bash
npm ci
npm run build
npm run test:e2e
```

如果当前环境无法运行某项适用检查，应明确报告该限制。