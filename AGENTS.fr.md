# Directives du dépôt

[English](AGENTS.md) | [简体中文](AGENTS.zh-CN.md) | [繁體中文](AGENTS.zh-TW.md) | [日本語](AGENTS.ja.md) | [Español](AGENTS.es.md)

## Mission Et Référence

- Construire une implémentation Rust idiomatique de MiniExcel en utilisant le dépôt .NET voisin `../MiniExcel` comme référence de compatibilité.
- Avant un travail de parité, examiner l'API publique .NET, son implémentation et les tests ciblés. Ne pas modifier ce dépôt sauf demande explicite.
- Considérer [docs/compatibility.md](docs/compatibility.md) comme le registre de prise en charge et `tests/data/contracts/xlsx-parity-v1.json` comme le contrat de comportement partagé.

## Architecture

- Le workspace contient `miniexcel`, `miniexcel-cli` et `miniexcel-wasm` ; conserver `MiniExcel` comme façade publique principale.
- Les `query` et `query_as` par chemin doivent rester des flux à mémoire bornée. Préférer des passes ZIP/XML séquentielles, un petit état de parser et des channels bornés.
- Utiliser des API structurées pour XML, ZIP et la sérialisation. Préserver l'ordre du workbook et les coordonnées publiques Excel commençant à 1.
- L'écriture crée de nouveaux workbooks XLSX ; ne revendiquer l'édition de fichiers existants que pour les opérations explicitement implémentées et testées.
- Prendre en charge Rust 1.85.0 et l'édition 2024. Le Rust unsafe est interdit.

## Règles De Modification

- Réutiliser les modèles et dépendances existants, cibler les changements et préserver les modifications sans rapport dans l'arbre de travail.
- Ajouter des tests de régression ciblés à partir des fixtures. Mettre à jour la compatibilité lorsque les API ou limites changent.
- Maintenir `AGENTS` et le `README` localisé à la racine dans exactement six langues : anglais, `.zh-CN`, `.zh-TW`, `.fr`, `.ja` et `.es`. Relier et actualiser toutes les versions ensemble ; ne pas ajouter de septième langue.
- Tout autre Markdown anglais exige une version `.zh-CN.md` complète. Lors d'une création ou révision majeure, actualiser aussi les versions `.zh-TW.md`, `.fr.md`, `.ja.md` et `.es.md` existantes.

## Browser Lab

- `web-demo` est sans backend : les données XLSX restent dans le navigateur via `miniexcel-wasm`.
- Requiert Node.js 22, la target `wasm32-unknown-unknown` et `wasm-bindgen-cli` 0.2.127.
- Exécuter `npm run dev` dans `web-demo` et ouvrir `http://127.0.0.1:4173`. Servir les builds en HTTP, pas avec `file://`.
- Conserver la couverture Playwright desktop et mobile.

## Validation

Exécuter le test le plus ciblé après la première modification, puis les contrôles applicables :

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
cargo doc --workspace --no-deps --locked
```

Les changements de parité exigent les tests de contrat Rust et .NET. Les changements Browser/WASM exigent aussi `npm ci`, `npm run build` et `npm run test:e2e` dans `web-demo`. Signaler les contrôles impossibles à exécuter.