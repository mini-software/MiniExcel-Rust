# Repository Guidelines

[简体中文](AGENTS.zh-CN.md) | [繁體中文](AGENTS.zh-TW.md) | [Français](AGENTS.fr.md) | [日本語](AGENTS.ja.md) | [Español](AGENTS.es.md)

## Mission And Reference

- Build an idiomatic Rust implementation of MiniExcel behavior; use the sibling .NET repository `../MiniExcel` as the compatibility reference.
- Before parity work, inspect the corresponding .NET public API, implementation, and focused tests. Do not modify that repository unless requested.
- Treat [docs/compatibility.md](docs/compatibility.md) as the support record and `tests/data/contracts/xlsx-parity-v1.json` as the shared behavior contract.

## Architecture

- The workspace contains `miniexcel`, `miniexcel-cli`, and `miniexcel-wasm`; keep `MiniExcel` as the main public facade.
- Path `query` and `query_as` must remain bounded-memory streams. Prefer sequential ZIP/XML passes, small parser state, and bounded channels.
- Use structured XML, ZIP, and serialization APIs. Preserve workbook order and Excel's 1-based public coordinates.
- Writing creates new XLSX workbooks; claim existing-file editing only for explicitly implemented and tested operations.
- Support Rust 1.85.0 and Edition 2024. Unsafe Rust is forbidden.

## Change Rules

- Reuse existing patterns and dependencies; keep changes focused and preserve unrelated working-tree changes.
- Add focused regression tests from existing fixtures. Update compatibility docs when APIs or support boundaries change.
- Maintain root `AGENTS` and localized root `README` files in exactly six languages: English, `.zh-CN`, `.zh-TW`, `.fr`, `.ja`, and `.es`. Link all variants and update them together; do not add a seventh language.
- Every other English Markdown document requires a complete `.zh-CN.md` version. On creation or substantial revision, also update available `.zh-TW.md`, `.fr.md`, `.ja.md`, and `.es.md` versions.

## Browser Lab

- `web-demo` is backend-free: XLSX data stays in the browser through `miniexcel-wasm`.
- Requires Node.js 22, target `wasm32-unknown-unknown`, and `wasm-bindgen-cli` 0.2.127.
- Run `npm run dev` in `web-demo` and open `http://127.0.0.1:4173`. Serve builds over HTTP, not `file://`.
- Keep desktop and mobile Playwright coverage.

## Validation

Run the narrowest test after the first edit, then applicable checks:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
cargo doc --workspace --no-deps --locked
```

Parity changes require both Rust and .NET contract tests. Browser/WASM changes also require `npm ci`, `npm run build`, and `npm run test:e2e` in `web-demo`. Report checks that cannot run.
