# Silex — reglas del proyecto para Claude Code

## Objetivo
Construir una aplicación de escritorio local-first, tipo Notion, con GUI **nativa en Rust** (egui/eframe), SQLite (rusqlite) y edición por bloques. Windows, macOS y Linux. Sin WebView, sin backend cloud, sin telemetría por defecto.

## Contexto que debes leer cuando corresponda
- Alcance y criterios: `docs/PRD.md`
- Decisiones y límites: `docs/ARCHITECTURE.md` y `docs/decisions/0001-native-rust.md`
- Esquema y riesgos: `docs/DATA_MODEL.md`
- Orden de entrega: `docs/ROADMAP.md`

## Comandos de verificación obligatorios
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo check --workspace`

## Reglas no negociables
1. Antes de cambios de más de un módulo: inspecciona el código, resume el plan y define criterios de aceptación verificables.
2. Implementa **solo una historia o hito pequeño a la vez**. No añadas sincronización, IA o plugins sin una decisión arquitectónica escrita.
3. Prioriza integridad de datos: migraciones **aditivas y transaccionales**, claves foráneas, parámetros SQL y pruebas de persistencia.
4. No uses `unwrap`/`expect` en rutas de producción que puedan fallar por datos o E/S. Propaga o muestra errores comprensibles.
5. La UI no ejecuta SQL; `src/db.rs` es la frontera de persistencia en el starter. Extrae repositorios solo cuando la complejidad lo justifique.
6. No bloquees deliberadamente la interfaz con operaciones largas. Al añadir importaciones, búsqueda compleja o adjuntos, introduce workers y mensajes.
7. No captures ni transmitas datos del usuario. No incluyas secretos, `.env`, bases SQLite ni copias de seguridad en Git.
8. Los cambios a esquemas existentes requieren nueva migración y pruebas de actualización de versiones anteriores. Nunca modifiques una migración ya publicada.
9. En PRs: documenta qué cambió, pruebas ejecutadas, riesgos y pasos manuales pendientes.
10. Nunca declares una prueba pasada si no se ejecutó. Si falta toolchain o SDK de OS, indícalo.

## Flujo habitual
- Explorar → planificar → escribir test → implementar → verificar → revisar diff → commit pequeño.
- Para trabajo repetible: `/next-milestone` o `/sqlite-review`.
- **No** actives bypass de permisos, ni publiques, borres datos o ejecutes acciones destructivas sin confirmación explícita.

## Mínimo de producto
El starter ofrece páginas planas, títulos, 3 tipos de bloque, SQLite y búsqueda FTS5. Anidación visual, arrastrar bloques, Markdown y bases de datos son hitos futuros; no se deben presentar como implementados.
