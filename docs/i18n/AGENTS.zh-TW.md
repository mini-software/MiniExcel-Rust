# 儲存庫指南

[English](../../AGENTS.md) | [简体中文](AGENTS.zh-CN.md) | [Français](AGENTS.fr.md) | [日本語](AGENTS.ja.md) | [Español](AGENTS.es.md)

## 使命與參考

- 使用相鄰的 .NET 儲存庫 `../MiniExcel` 作為相容性參考，建構符合 Rust 慣例的 MiniExcel 行為實作。
- 實作等同行為前，檢查對應的 .NET 公開 API、實作與聚焦測試。除非明確要求，否則不要修改該儲存庫。
- 將 [compatibility.md](../compatibility.md) 視為支援範圍記錄，將 `tests/data/contracts/xlsx-parity-v1.json` 視為共用行為合約。

## 架構

- workspace 包含 `miniexcel`、`miniexcel-cli` 與 `miniexcel-wasm`；保持 `MiniExcel` 為主要公開 facade。
- 路徑 `query` 與 `query_as` 必須維持有界記憶體串流。優先使用循序 ZIP/XML 掃描、小型 parser 狀態與有界 channel。
- 使用結構化 XML、ZIP 與序列化 API。保持 workbook 順序及 Excel 從 1 開始的公開座標。
- 寫入會建立新的 XLSX workbook；只有明確實作並測試的操作才能宣稱支援編輯既有檔案。
- 支援 Rust 1.85.0 與 Edition 2024。禁止 unsafe Rust。

## 修改規則

- 重用現有模式與相依套件，保持修改聚焦，並保留無關的工作樹變更。
- 使用現有 fixture 新增聚焦迴歸測試。API 或支援邊界變更時更新相容性文件。
- 英文 `README.md` 與 `AGENTS.md` 保留在儲存庫根目錄，其餘五種本地化版本統一放在 `docs/i18n/`。固定維護六種語言：英文、`.zh-CN`、`.zh-TW`、`.fr`、`.ja`、`.es`。所有版本必須互相連結並同步更新；不得增加第七種語言。
- 其他英文 Markdown 必須有完整 `.zh-CN.md` 版本；建立或大幅修改時，也應更新已有的 `.zh-TW.md`、`.fr.md`、`.ja.md` 與 `.es.md` 版本。

## Browser Lab

- `web-demo` 不依賴後端，XLSX 資料透過 `miniexcel-wasm` 留在瀏覽器本機。
- 需要 Node.js 22、target `wasm32-unknown-unknown` 與 `wasm-bindgen-cli` 0.2.127。
- 在 `web-demo` 中執行 `npm run dev`，開啟 `http://127.0.0.1:4173`。建構產物必須透過 HTTP 提供，不能使用 `file://`。
- 保持 Playwright 同時涵蓋桌面與行動 viewport。

## 驗證

第一次修改後執行範圍最窄的測試，完成前執行適用檢查：

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
cargo doc --workspace --no-deps --locked
```

相容行為變更必須同時執行 Rust 與 .NET 合約測試。Browser/WASM 變更還需在 `web-demo` 執行 `npm ci`、`npm run build` 與 `npm run test:e2e`。無法執行的檢查必須說明。