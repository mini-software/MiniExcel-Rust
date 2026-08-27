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

Placez ce dépôt à côté de [.NET MiniExcel](https://github.com/mini-software/MiniExcel), puis exécutez :

```powershell
pwsh ./scripts/compare-dotnet-v1-rust.ps1 -DotNetRepository D:\git\MiniExcel
```

Le rapport est écrit dans `target/benchmarks/dotnet-v1-vs-rust.json`. Comparez uniquement des résultats de la même machine ; voir la [méthodologie](docs/dotnet-v1-query-benchmark.md).
