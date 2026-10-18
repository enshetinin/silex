# ADR-0001 — UI nativa en Rust y base local SQLite

**Estado:** aceptada para el starter (2026-10-18).

## Contexto
El producto requiere escritorio nativo en Rust, datos SQLite en disco, cero servidor obligatorio y desarrollo iterativo mediante Claude Code.

## Alternativas
- **Tauri 2 + frontend web:** madurez del ecosistema de editores HTML/ProseMirror y UI cuidada, pero depende de WebView. No satisface el requisito de UI implementada en Rust.
- **egui/eframe:** Rust puro para widgets y render nativo; iteración rápida, multiplataforma, excelente integración Rust. Editor rich-text requiere ingeniería propia; no reproduce por defecto los widgets del sistema.
- **Iced/Slint:** opciones legítimas con enfoques declarativos, pero requieren evaluación posterior de edición avanzada, accesibilidad y capacidades de la UI.

## Decisión
Utilizar **egui/eframe** y **rusqlite bundled** con esquema versionado. Mantener modelo y persistencia sin dependencia de egui. No incluir sync cloud al principio.

## Consecuencias
- Control sobre render, procesos y persistencia sin Node/WebView.
- Más esfuerzo propio para un editor con selección, bloques y accesibilidad comparable a Notion.
- Para UI extremadamente fiel a Notion, reconsiderar Tauri sería una nueva ADR que necesitaría autorización del propietario.
