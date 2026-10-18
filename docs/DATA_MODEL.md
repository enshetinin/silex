# Modelo de datos y estrategia SQLite

## Versión 1 (starter)

### `pages`
- `id TEXT PRIMARY KEY` — UUID estable.
- `parent_id TEXT NULL` — referencia de página; interfaz jerárquica pendiente.
- `title TEXT`, `sort_order INTEGER`.
- `created_at`, `updated_at` — UTC ISO-8601.
- `deleted_at NULL` — borrado lógico; restauración pendiente.

### `blocks`
- `id TEXT PRIMARY KEY`, `page_id TEXT NOT NULL REFERENCES pages`.
- `kind` (`paragraph`, `heading`, `todo`).
- `content TEXT`, `checked INTEGER`, `position INTEGER`.
- `created_at`, `updated_at`.

### Búsqueda
`page_search` y `block_search` son tablas virtuales FTS5 external-content que consultan el `rowid` de tablas normales. Triggers mantienen índices tras inserciones, cambios y eliminaciones. Los resultados filtran páginas con `deleted_at IS NULL`.

## Evolución
- La migración `0001_init.sql` es inmutable una vez liberada.
- Nuevos campos/tipos/tablas en migración `0002_*.sql`; incrementar `LATEST_SCHEMA_VERSION` e incorporar pruebas desde una base antigua.
- Migrar dentro de transacciones; respaldar datos antes de operaciones irreversibles.
- Desacoplar el `BlockKind` de los nombres SQL con enum tipado cuando crezca el editor.
- Evolucionar a `document_operations` para undo/redo e historial si existen requisitos concretos; no convertir el editor en CRDT prematuramente.

## Riesgos conocidos del starter
- Los valores `position` usan enteros consecutivos. Reordenar bloques requerirá transacciones y renumeración o fractional indexing.
- La anidación almacenada no está expuesta en UI, ni tiene controles para ciclos ni orden jerárquico.
- SQLite no está cifrada.
- Falta backup/restauración con interfaz de usuario.
- FTS frase exacta; el UX de búsqueda necesita ranking, snippets, prefijos y normalización.
- El schema permite pages hijas de páginas enviadas a la papelera; hay que definir semántica recursiva.
- FTS external-content se enlaza por el `rowid` implícito de `pages`/`blocks` (PK `TEXT`). SQLite documenta que `VACUUM` puede cambiar esos `rowid`; no se ha reproducido, pero los backups con `VACUUM INTO` (hito 1) deben verificar o reconstruir el índice (`INSERT INTO page_search(page_search) VALUES('rebuild')`) y tener un test.
- Si un guardado falla, los cambios siguen en memoria, el error se muestra y se reintenta con backoff (`src/save_state.rs`). Cerrar la ventana con cambios sin guardar se bloquea hasta reintentar con éxito o descartar explícitamente. Si el proceso termina sin petición de cierre (p. ej. lo mata el sistema), solo queda el último guardado automático.

## Tests mínimos posteriores
1. Migración de datos existentes preserva títulos, bloques y relaciones.
2. FTS encuentra inserts, updates y deja de encontrar deletes.
3. Fallo de transacción deja intacta la página.
4. Guardado al cerrar/no pérdida ante errores de disco.
5. Rutas de backup consistentes en Windows, macOS, Linux.
6. SQLite corrupta o disco lleno muestra error recuperable y no sobrescribe datos.
