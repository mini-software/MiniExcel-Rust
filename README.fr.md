<div align="center">

# MiniExcel pour Rust

[English](README.md) | [简体中文](README.zh-CN.md) | [繁體中文](README.zh-TW.md) | [日本語](README.ja.md) | [Español](README.es.md)

[![Crates.io](https://img.shields.io/crates/v/miniexcel.svg)](https://crates.io/crates/miniexcel)
[![Téléchargements](https://img.shields.io/crates/d/miniexcel.svg)](https://crates.io/crates/miniexcel)
[![Documentation](https://docs.rs/miniexcel/badge.svg)](https://docs.rs/miniexcel)
[![CI](https://github.com/mini-software/MiniExcel-Rust/actions/workflows/rust.yml/badge.svg)](https://github.com/mini-software/MiniExcel-Rust/actions/workflows/rust.yml)
[![GitHub Stars](https://img.shields.io/github/stars/mini-software/MiniExcel-Rust?logo=github)](https://github.com/mini-software/MiniExcel-Rust)
[![Licence](https://img.shields.io/crates/l/miniexcel.svg)](LICENSE)

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

### Package .NET

Le dépôt construit également le package NuGet préliminaire `MiniExcel.Rust`. Il dépend exactement de
MiniExcel v1 `1.46.0`, réutilise ses types de configuration et de
mapping, et transmet les appels `MiniExcelRust` à la bibliothèque native Rust.

```bash
dotnet add package MiniExcel.Rust --prerelease
```

```csharp
using MiniExcelLibs;
using MiniExcelLibs.OpenXml;

var rows = MiniExcelRust.Query(
    "book.xlsx",
    useHeaderRow: true,
    configuration: new OpenXmlConfiguration { IgnoreEmptyRows = true });
```

`MiniExcel.Query` utilise l'implémentation managée d'origine ; `MiniExcelRust.Query` utilise le
backend Rust. Construisez et testez un package local avec
`./scripts/dotnet/Test-Package.ps1 -Rid win-x64`.

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

Voir la [matrice de compatibilité](docs/compatibility.md), le [contrat analyse/RAG](docs/analytics-and-rag.md) et le [guide de migration Insert](docs/insert-v1-migration.md).

## Benchmark Rust Et .NET

### Derniers Résultats NuGet

Le dernier test Windows x64 compare `MiniExcel 1.46.0`, le wrapper .NET
`MiniExcel.Rust 0.1.0-preview.1` et `MiniExcel Rust 0.4.0` natif sur 100 000 lignes x 10 colonnes,
avec cinq processus neufs par runtime et scénario. Toutes les valeurs sont vérifiées avant la mesure.

| Scénario | Runtime | Temps médian | Lignes/s | Première ligne | Allocation managée | Working set maximal |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| Cold | MiniExcel | 2 598,50 ms | 38 484 | 1 051,97 ms | 2 541,93 MB | 50,93 MB |
| Cold | MiniExcel.Rust (.NET) | 1 899,42 ms | 52 648 | 450,85 ms | 289,36 MB | 49,64 MB |
| Cold | MiniExcel.Rust | 1 220,82 ms | 81 912 | 537,17 ms | n/a | 4,14 MB |
| Steady | MiniExcel | 5 997,76 ms | 50 019 | 616,40 ms | 7 625,15 MB | 52,80 MB |
| Steady | MiniExcel.Rust (.NET) | 4 202,10 ms | 71 393 | 442,59 ms | 868,05 MB | 48,79 MB |
| Steady | MiniExcel.Rust | 3 344,20 ms | 89 708 | 467,30 ms | n/a | 4,20 MB |

Les résultats dépendent de la machine. Relancez `pwsh ./scripts/compare-nuget-v1-rust.ps1`
sur des workbooks représentatifs ; voir la [méthodologie](docs/dotnet-v1-query-benchmark.md).

Placez ce dépôt à côté de [.NET MiniExcel](https://github.com/mini-software/MiniExcel), puis exécutez :

```powershell
pwsh ./scripts/compare-dotnet-v1-rust.ps1 -DotNetRepository D:\git\MiniExcel
```

Le rapport est écrit dans `target/benchmarks/dotnet-v1-vs-rust.json`. Comparez uniquement des résultats de la même machine ; voir la [méthodologie](docs/dotnet-v1-query-benchmark.md).
