# リポジトリガイドライン

[English](AGENTS.md) | [简体中文](AGENTS.zh-CN.md) | [繁體中文](AGENTS.zh-TW.md) | [Français](AGENTS.fr.md) | [Español](AGENTS.es.md)

## ミッションと参照

- 隣接する .NET リポジトリ `../MiniExcel` を互換性の基準として、MiniExcel を慣用的な Rust で実装する。
- Parity 対応前に .NET の公開 API、実装、対象テストを確認する。明示的な依頼なしに同リポジトリを変更しない。
- [docs/compatibility.md](docs/compatibility.md) をサポート記録、`tests/data/contracts/xlsx-parity-v1.json` を共有動作コントラクトとする。

## アーキテクチャ

- workspace は `miniexcel`、`miniexcel-cli`、`miniexcel-wasm` で構成し、`MiniExcel` を主要な公開 facade とする。
- Path の `query` と `query_as` は有界メモリストリームを維持する。逐次 ZIP/XML パス、小さな parser 状態、有界 channel を優先する。
- XML、ZIP、serialization には構造化 API を使う。Workbook の順序と 1 始まりの公開 Excel 座標を維持する。
- 書き込みは新しい XLSX workbook を作成する。既存ファイル編集を説明できるのは、明示的に実装・テストした操作だけとする。
- Rust 1.85.0 と Edition 2024 をサポートする。Unsafe Rust は禁止する。

## 変更ルール

- 既存のパターンと依存関係を再利用し、変更範囲を絞り、無関係な working-tree 変更を保持する。
- 既存 fixture から対象を絞った回帰テストを追加する。API またはサポート境界の変更時は互換性文書を更新する。
- ルートの `AGENTS` とローカライズ `README` は、英語、`.zh-CN`、`.zh-TW`、`.fr`、`.ja`、`.es` の 6 言語だけを維持する。全版を相互リンクし同時更新する。7 番目の言語を追加しない。
- その他の英語 Markdown には完全な `.zh-CN.md` 版が必要。新規作成または大幅改訂時は、既存の `.zh-TW.md`、`.fr.md`、`.ja.md`、`.es.md` 版も更新する。

## Browser Lab

- `web-demo` は backend-free で、XLSX データは `miniexcel-wasm` によりブラウザー内に留まる。
- Node.js 22、target `wasm32-unknown-unknown`、`wasm-bindgen-cli` 0.2.127 が必要。
- `web-demo` で `npm run dev` を実行し、`http://127.0.0.1:4173` を開く。Build は `file://` ではなく HTTP で配信する。
- Desktop と mobile の Playwright coverage を維持する。

## 検証

最初の変更後に最小範囲のテストを実行し、その後、該当するチェックを実行する。

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
cargo doc --workspace --no-deps --locked
```

Parity 変更には Rust と .NET の contract test が必要。Browser/WASM 変更では `web-demo` で `npm ci`、`npm run build`、`npm run test:e2e` も実行する。実行できないチェックは報告する。