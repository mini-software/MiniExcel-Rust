<div align="center">

# MiniExcel pour Rust

[English](../../README.md) | [简体中文](README.zh-CN.md) | [繁體中文](README.zh-TW.md) | [日本語](README.ja.md) | [Español](README.es.md)

[![Crates.io](https://img.shields.io/crates/v/miniexcel.svg)](https://crates.io/crates/miniexcel)
[![Téléchargements](https://img.shields.io/crates/d/miniexcel.svg)](https://crates.io/crates/miniexcel)
[![Documentation](https://docs.rs/miniexcel/badge.svg)](https://docs.rs/miniexcel)
[![CI](https://github.com/mini-software/MiniExcel-Rust/actions/workflows/rust.yml/badge.svg)](https://github.com/mini-software/MiniExcel-Rust/actions/workflows/rust.yml)
[![GitHub Stars](https://img.shields.io/github/stars/mini-software/MiniExcel-Rust?logo=github)](https://github.com/mini-software/MiniExcel-Rust)
[![Licence](https://img.shields.io/crates/l/miniexcel.svg)](../../LICENSE)

**Traitement XLSX et CSV rapide, avec mémoire bornée.**

</div>

---

<div align="center">

Ce projet fait partie de l'écosystème [MiniExcel](https://github.com/mini-software/MiniExcel) et utilise la bibliothèque .NET comme référence de compatibilité.

</div>

---

<div align="center">

**[Ouvrir Browser Lab](https://mini-software.github.io/MiniExcel-Rust/)** pour inspecter ou générer des XLSX localement. Les données restent dans le navigateur.

</div>

---

## Présentation

MiniExcel pour Rust est une bibliothèque de lecture/écriture XLSX et CSV avec flux à mémoire bornée, Serde, analyse et exports RAG.

## Installation

```bash
cargo add miniexcel
```

Nécessite Rust 1.85.0 ou ultérieur.

## Démarrage Rapide

```rust
use miniexcel::MiniExcel;

for row in MiniExcel::query("book.xlsx")? {
    println!("{:?}", row?["A"]);
}
```

```rust
use miniexcel::{CellValue, DynamicRow, MiniExcel};

let mut row = DynamicRow::new();
row.insert("Name".into(), CellValue::String("MiniExcel".into()));
MiniExcel::save_as("book.xlsx", &[row])?;
```

Exécutez les exemples inclus avec vos propres fichiers :

```bash
cargo run -p miniexcel --example read -- book.xlsx
cargo run -p miniexcel --example write -- output.xlsx
cargo run -p miniexcel --example rag_export -- book.xlsx
```

## Flux De Travail Courants

Toutes les API renvoient `miniexcel::Result`. Les extraits ci-dessous peuvent
être placés dans `fn main() -> miniexcel::Result<()>`. Les exemples typés
nécessitent `cargo add serde --features derive` ; l'exemple de template requiert
aussi `cargo add serde_json`.

### Sélectionner Une Feuille Et Une Plage

`query()` n'utilise pas d'en-tête par défaut et emploie les lettres de colonnes
Excel comme clés. Utilisez `HeaderMode::FirstRow` si la première ligne
sélectionnée contient les noms de colonnes. Les cellules de début et de fin sont
incluses.

```rust
use miniexcel::{HeaderMode, MiniExcel, ReadOptions};

let options = ReadOptions::new()
    .with_sheet_name("Data")
    .with_header_mode(HeaderMode::FirstRow)
    .with_start_cell("A1".parse()?)
    .with_end_cell("F100".parse()?)
    .with_ignore_empty_rows(true);

for row in MiniExcel::query_with_options("book.xlsx", &options)? {
    let row = row?;
    println!("{:?}", row["Name"]);
}
```

L'itérateur possède un worker borné. Arrêter l'itération avec `.take(10)`, par
exemple, interrompt le reste de la requête path lorsque l'itérateur est détruit.

### Désérialiser Des Lignes Typées Avec Serde

Les requêtes typées utilisent par défaut la première ligne sélectionnée comme
en-tête et effectuent le mapping ligne par ligne.

```rust
use miniexcel::MiniExcel;

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Release {
    name: String,
    version: u32,
}

for release in MiniExcel::query_as::<Release>("book.xlsx")? {
    let release = release?;
    println!("{} {}", release.name, release.version);
}
```

Utilisez `miniexcel::serde_helpers` pour convertir strictement les dates et
heures série Excel vers les types `chrono`.

### Créer Un Workbook Depuis Des Lignes Serde

```rust
use miniexcel::{MiniExcel, WriteOptions};

#[derive(serde::Serialize)]
#[serde(rename_all = "PascalCase")]
struct Release<'a> {
    name: &'a str,
    version: u32,
}

let rows = [
    Release { name: "MiniExcel", version: 1 },
    Release { name: "MiniExcel Rust", version: 4 },
];
let options = WriteOptions::new()
    .with_sheet_name("Releases")
    .with_auto_width(true);
MiniExcel::save_as_serialized_with_options("releases.xlsx", &rows, &options)?;
```

Les API Save créent un workbook et refusent un chemin de sortie existant sauf
si `with_overwrite_file(true)` est explicitement activé.

### Modifier Atomiquement Un Workbook Existant

```rust
use miniexcel::{InsertOptions, MiniExcel};

let inserted = MiniExcel::insert_serialized(
    "book.xlsx",
    &rows,
    &InsertOptions::new().with_sheet_name("Archive"),
)?;
MiniExcel::rename_sheet("book.xlsx", "Archive", "History")?;
MiniExcel::reorder_sheet("book.xlsx", "History", 0)?;
println!("inserted {inserted} rows");
```

Les modifications par path verrouillent, réécrivent et valident le workbook
avant son remplacement atomique. Un nom de feuille existant est refusé par
défaut ; choisissez explicitement `ExistingSheetPolicy::Replace` pour le
remplacer.

### Remplir Un Template XLSX

Placez des marqueurs comme `{{title}}`, `{{items.name}}` et `{{items.score}}`
dans le workbook template. Une liste développe sa ligne modèle.

```rust
use miniexcel::{MiniExcel, TemplateOptions};
use serde_json::json;

MiniExcel::save_as_template(
    "report.xlsx",
    "template.xlsx",
    &json!({
        "title": "Quarterly report",
        "items": [
            { "name": "Ada", "score": 10 },
            { "name": "Linus", "score": 20 }
        ]
    }),
    &TemplateOptions::new(),
)?;
```

### Exporter Des Chunks RAG Traçables

```rust
use miniexcel::{HeaderMode, MiniExcel, RagExportOptions, ReadOptions};

let read = ReadOptions::new().with_header_mode(HeaderMode::FirstRow);
let rag = RagExportOptions::new().with_chunk_rows(25).with_max_rows(500);
let mut export = MiniExcel::export_rag("book.xlsx", &read, &rag)?;

for chunk in export.by_ref() {
    let chunk = chunk?;
    println!("{} {}", chunk.chunk_id(), chunk.data_range());
}
println!("source SHA-256: {}", export.manifest().source_sha256());
```

Chaque chunk conserve l'identité feuille/plage, les adresses A1, les valeurs
typées en cache, le texte des formules, les style IDs et les number formats.
Les feuilles masquées exigent un consentement explicite. Consultez le
[contrat RAG](../analytics-and-rag.md) pour les sorties JSONL et Markdown en
flux.

### Utiliser Le CLI Du Dépôt

Le CLI est un outil local du workspace et n'est pas publié comme crate séparé.

```bash
cargo run -p miniexcel-cli -- sheets book.xlsx
cargo run -p miniexcel-cli -- query book.xlsx --sheet Data --header --start-cell A1 --end-cell F100 --format jsonl
cargo run -p miniexcel-cli -- rag-export book.xlsx --header --chunk-rows 25 --format both --output-prefix ./out/book
```

### Choisir Une Forme D'I/O

| Entrée ou sortie | API principales | Comportement mémoire |
| --- | --- | --- |
| Chemin de fichier | `query*`, `query_as*`, `save_as*`, `insert*` | Pipeline de lignes borné ; les grandes shared strings peuvent utiliser un index disque |
| Octets XLSX | `query_bytes`, `save_as_bytes`, `visit_rag_chunks_from_bytes` | Les octets du workbook restent en mémoire |
| Streams empruntés | `visit_rows_from_reader`, `save_as_to_writer` | L'appelant conserve la propriété du stream |
| Navigateur | `miniexcel-wasm` et [Browser Lab](https://mini-software.github.io/MiniExcel-Rust/) | WebAssembly local ; les octets chargés et les téléchargements terminés utilisent la mémoire du navigateur |

D'autres programmes exécutables se trouvent dans
[`miniexcel/examples`](../../miniexcel/examples). Consultez la
[matrice de compatibilité](../compatibility.md) avant de dépendre précisément
de l'édition de workbooks, des templates, des formules ou du formatage.

## Capacités

- Requêtes dynamiques, typées, structurées, Table et CSV à mémoire bornée.
- API par chemin, octets et reader/writer emprunté.
- Lecture/écriture Serde, helpers date/heure et mapping de cells précis.
- Création multi-feuilles, options de format et visibilité.
- Ajout/remplacement, renommage, ordre, copie et visibilité atomiques des feuilles.
- Templates, blocs conditionnels/groupés et fusion pilotée par marqueurs.
- Analyses groupées en flux avec limites explicites.
- Exports JSONL et Markdown adressés à la source pour LLM/RAG.
- Streams async optionnels et indépendants du runtime ; ZIP/XML/filesystem restent bloquants.

## Sémantique Clé

- Les requêtes par chemin lisent le XML en flux sans conserver toutes les lignes.
- La feuille par défaut est la première, pas l'onglet actif.
- La lecture renvoie les valeurs de formule en cache ; la lecture structurée expose aussi texte et formats. MiniExcel ne calcule pas les formules.
- Save crée un workbook et refuse les chemins existants par défaut ; Insert modifie les `.xlsx` atomiquement après validation.
- Les grandes shared strings peuvent utiliser des fichiers temporaires indexés ; les requêtes bytes/WASM les gardent en mémoire.
- Non pris en charge : `.xls`, `.xlsb`, `.ods`, macros, création d'images, calcul de formules ou système général de styles.

Voir la [matrice de compatibilité](../compatibility.md), le [contrat analyse/RAG](../analytics-and-rag.md) et le [guide de migration Insert](../insert-v1-migration.md).

## Benchmark Rust Et .NET

Placez ce dépôt à côté de [.NET MiniExcel](https://github.com/mini-software/MiniExcel), puis exécutez :

```powershell
pwsh ./scripts/compare-dotnet-v1-rust.ps1 -DotNetRepository D:\git\MiniExcel
```

Le rapport est écrit dans `target/benchmarks/dotnet-v1-vs-rust.json`. Comparez uniquement des résultats de la même machine ; voir la [méthodologie](../dotnet-v1-query-benchmark.md).
