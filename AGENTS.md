# Repository Guidelines

[简体中文](AGENTS.zh-CN.md)

## Mission

- Build an idiomatic Rust implementation of MiniExcel behavior, using the .NET MiniExcel project as the compatibility reference.
- Preserve Rust performance, memory safety, and API conventions. Match observable behavior; do not translate .NET internals literally.
- Treat [docs/compatibility.md](docs/compatibility.md) as the current scope and architecture record. Do not claim unsupported surfaces as equivalent.

## Reference Repositories

- The .NET reference checkout is normally the sibling repository `../MiniExcel` (`D:\git\MiniExcel` on the primary Windows development machine).
- Before implementing parity behavior, inspect the corresponding .NET public API, controlling implementation, and focused tests.
- Do not modify the .NET repository unless the task explicitly requires it.
- `tests/data/contracts/xlsx-parity-v1.json` is the shared behavior contract. Update it deliberately and validate both adapters when changing covered behavior.

## Architecture

- The workspace contains `miniexcel` (core library), `miniexcel-cli` (CLI), and `miniexcel-wasm` (browser adapter).
- Keep `MiniExcel` as the main public facade. Keep parser, reader, writer, and concrete iterator types internal unless a public abstraction is required.
- Path-based `query` and `query_as` must remain bounded-memory streams. Do not retain complete worksheet XML or all rows.
- Prefer sequential ZIP/XML passes, small parser state, and bounded channels over workbook-wide materialization. Byte-array APIs may materialize results when their contract requires it.
- Use structured XML, ZIP, and serialization APIs. Avoid ad hoc string parsing when an existing parser or typed helper applies.
- Preserve workbook order and Excel coordinate semantics. Public indices are 1-based; internal cell coordinates may remain 0-based.
- Writing creates new XLSX workbooks. Do not imply support for editing existing files unless that capability is explicitly implemented and tested.

## Implementation Rules

- Support Rust 1.85.0 and Edition 2024. Do not use features newer than the declared MSRV.
- Unsafe Rust is forbidden across the workspace.
- Reuse existing crate patterns and dependencies before adding abstractions or packages.
- Keep changes focused. Do not rewrite unrelated code or overwrite existing working-tree changes.
- Add regression tests from existing fixtures when possible. Put shared .NET/Rust expectations in the parity contract only when both adapters cover the behavior.
- Update the compatibility matrix and public documentation when support boundaries or APIs change.
- Keep a complete `.zh-CN.md` counterpart for every English Markdown document, with reciprocal language links, and update both versions together.

## Pure Frontend Browser Lab

- `web-demo` is a backend-free static application. XLSX parsing and generation run locally in the browser through `miniexcel-wasm`; do not introduce a server API or upload workbook data unless explicitly requested.
- Prerequisites are Node.js 22, the `wasm32-unknown-unknown` Rust target, and `wasm-bindgen-cli` version 0.2.127:

```bash
rustup target add wasm32-unknown-unknown --toolchain 1.85.0
cargo +stable install wasm-bindgen-cli --version 0.2.127 --locked
```

- To build and run locally, use `npm run dev` from `web-demo`, then open `http://127.0.0.1:4173`:

```bash
cd web-demo
npm ci
npm run dev
```

- `npm run build` produces a deployable static site in `web-demo/dist`, including the HTML, CSS, JavaScript, and WebAssembly assets. It can be deployed directly to GitHub Pages or another static host; no .NET or Rust server process is required at runtime.
- Serve `dist` over HTTP with `npm run serve` or another static server. Do not rely on opening `dist/index.html` through `file://`, because browser module and WebAssembly loading requires correct HTTP behavior and MIME types.
- Keep browser tests covering both desktop and mobile viewports. The current Playwright configuration starts the local static server automatically.

## Validation

Run the narrowest relevant test immediately after the first substantive edit. Before completion, run the applicable CI checks from the repository root:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
cargo doc --workspace --no-deps --locked
```

For shared .NET parity changes, run both adapters:

```bash
cargo +1.85.0 test -p miniexcel --test parity_contract --locked
dotnet test ../MiniExcel/tests/MiniExcel.OpenXml.Tests/MiniExcel.OpenXml.Tests.csproj --framework net10.0 --filter "FullyQualifiedName~RustParityContractTests"
```

For browser or WASM changes, also run from `web-demo`:

```bash
npm ci
npm run build
npm run test:e2e
```

If an environment cannot run an applicable check, report that limitation explicitly.