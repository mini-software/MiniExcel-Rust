# Directrices del repositorio

[English](../../AGENTS.md) | [简体中文](AGENTS.zh-CN.md) | [繁體中文](AGENTS.zh-TW.md) | [Français](AGENTS.fr.md) | [日本語](AGENTS.ja.md)

## Misión Y Referencia

- Crear una implementación idiomática en Rust de MiniExcel usando el repositorio .NET adyacente `../MiniExcel` como referencia de compatibilidad.
- Antes de implementar paridad, revisar la API pública .NET, su implementación y las pruebas específicas. No modificar ese repositorio salvo petición expresa.
- Tratar [compatibility.md](../compatibility.md) como registro de soporte y `tests/data/contracts/xlsx-parity-v1.json` como contrato de comportamiento compartido.

## Arquitectura

- El workspace contiene `miniexcel`, `miniexcel-cli` y `miniexcel-wasm`; mantener `MiniExcel` como facade pública principal.
- `query` y `query_as` por path deben seguir siendo streams de memoria acotada. Preferir pasadas ZIP/XML secuenciales, poco estado del parser y channels acotados.
- Usar API estructuradas de XML, ZIP y serialización. Conservar el orden del workbook y las coordenadas públicas de Excel basadas en 1.
- La escritura crea workbooks XLSX nuevos; afirmar edición de archivos existentes solo para operaciones implementadas y probadas expresamente.
- Admitir Rust 1.85.0 y Edition 2024. Se prohíbe unsafe Rust.

## Reglas De Cambio

- Reutilizar patrones y dependencias existentes, limitar los cambios y conservar modificaciones ajenas en el working tree.
- Añadir pruebas de regresión específicas a partir de fixtures. Actualizar la compatibilidad cuando cambien API o límites de soporte.
- Mantener `README.md` y `AGENTS.md` en inglés en la raíz del repositorio. Guardar sus cinco variantes localizadas en `docs/i18n/`, para un total exacto de seis idiomas: inglés, `.zh-CN`, `.zh-TW`, `.fr`, `.ja` y `.es`. Enlazar y actualizar todas las versiones juntas; no añadir un séptimo idioma.
- Cualquier otro Markdown inglés requiere una versión `.zh-CN.md` completa. Al crearlo o revisarlo sustancialmente, actualizar también las versiones `.zh-TW.md`, `.fr.md`, `.ja.md` y `.es.md` existentes.

## Browser Lab

- `web-demo` no tiene backend: los datos XLSX permanecen en el navegador mediante `miniexcel-wasm`.
- Requiere Node.js 22, el target `wasm32-unknown-unknown` y `wasm-bindgen-cli` 0.2.127.
- Ejecutar `npm run dev` en `web-demo` y abrir `http://127.0.0.1:4173`. Servir los builds por HTTP, no mediante `file://`.
- Mantener cobertura Playwright de escritorio y móvil.

## Validación

Ejecutar la prueba más específica después del primer cambio y luego las comprobaciones aplicables:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
cargo doc --workspace --no-deps --locked
```

Los cambios de paridad requieren las pruebas de contrato Rust y .NET. Los cambios Browser/WASM requieren además `npm ci`, `npm run build` y `npm run test:e2e` en `web-demo`. Informar de las comprobaciones que no puedan ejecutarse.