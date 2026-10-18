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

## Versión 2 — favoritos (`0002_favorites.sql`)
- `pages.favorite_at TEXT NULL`: `NULL` significa que no es favorita; la fecha ordena la lista de favoritos (de más antigua a más reciente). Índice parcial `pages_favorite_idx`.
- Las favoritas en la papelera se ocultan y vuelven al restaurar la página.

## Versión 3 — tipos de bloque y sangría (`0003_block_kinds.sql`, ADR 0003)
- `blocks.kind` ∈ `paragraph`, `heading_1`, `heading_2`, `heading_3`, `todo`, `bulleted`, `numbered`, `quote`, `code`. Los `heading` de v1/v2 pasan a `heading_1`. En Rust es `BlockKind`; un nombre desconocido al leer es un error, no un pánico.
- `blocks.indent INTEGER NOT NULL DEFAULT 0 CHECK (indent BETWEEN 0 AND 6)`: lista plana con sangría, sin árbol de bloques.
- La tabla se reconstruye (SQLite no permite cambiar un `CHECK`): copia los `rowid` explícitamente, recrea índice y triggers FTS y hace `rebuild` de `block_search`. Hay tests con un hueco de `rowid` y un fallo a mitad que deshace todo.
- La numeración de listas se calcula al pintar (`list_numbers`), no se guarda.
- `save_document` sincroniza la lista completa de la página (inserta, actualiza, borra, `position` = índice) en una transacción y solo toca filas cambiadas, para no reindexar FTS de más.

## Evolución
- Las migraciones publicadas (`0001`, `0002`) son inmutables. Un cambio nuevo es un archivo `000N_*.sql` añadido a `MIGRATIONS` en `src/db.rs`, más tests desde una base de la versión anterior (`tests/migrations.rs`).
- Todas las migraciones pendientes se aplican en **una sola transacción**: la base queda actualizada del todo o exactamente como estaba (test con un fallo a mitad de la `0002`).
- Antes de actualizar una base existente en disco se crea una copia verificada con el esquema antiguo. Si esa copia falla, no se migra.
- Las copias de esquemas anteriores son válidas: la verificación acepta `1..=LATEST` y la restauración actualiza la base después de restaurar.
- Evolucionar a `document_operations` para undo/redo e historial si existen requisitos concretos; no convertir el editor en CRDT prematuramente.

## Riesgos conocidos del starter
- Los valores `position` usan enteros consecutivos. Reordenar bloques requerirá transacciones y renumeración o fractional indexing.
- Anidación: `Database::move_page` rechaza ciclos con una consulta recursiva dentro de la transacción (comprobación de la app, no un trigger). Con la papelera por ramas (H2.3) ya no se crean páginas huérfanas; si existieran (datos antiguos o borrados desde otra conexión), se muestran en la raíz. Si los datos contuvieran un ciclo, `build_tree` no entra en bucle y muestra esas páginas en la raíz.
- SQLite no está cifrada.
- Falta backup/restauración con interfaz de usuario.
- FTS frase exacta; el UX de búsqueda necesita ranking, snippets, prefijos y normalización.
- Papelera por ramas sin columna nueva: el grupo se identifica por `deleted_at` (resolución de ms, único por operación). Una subpágina borrada antes que su padre es un elemento aparte en la papelera y no vuelve al restaurar al padre. Si en el futuro se necesitan más metadatos del borrado (quién o por qué), hará falta una tabla `trash_batches` con su migración.
- FTS external-content se enlaza por el `rowid` implícito de `pages`/`blocks` (PK `TEXT`). SQLite documenta que `VACUUM` puede cambiar esos `rowid`. Por eso las copias usan la API de backup (copia páginas y conserva `rowid`), no `VACUUM INTO`, y cada copia pasa el `integrity-check` de FTS5 contra su tabla de contenido (test `verify_detects_search_index_out_of_sync`). No ejecutar `VACUUM` sin un `rebuild` posterior del índice.
- Si un guardado falla, los cambios siguen en memoria, el error se muestra y se reintenta con backoff (`src/save_state.rs`). Cerrar la ventana con cambios sin guardar se bloquea hasta reintentar con éxito o descartar explícitamente. Si el proceso termina sin petición de cierre (p. ej. lo mata el sistema), solo queda el último guardado automático.

## Tests mínimos posteriores
1. Migración de datos existentes preserva títulos, bloques y relaciones.
2. FTS encuentra inserts, updates y deja de encontrar deletes.
3. Fallo de transacción deja intacta la página.
4. Guardado al cerrar/no pérdida ante errores de disco.
5. Rutas de backup consistentes en Windows, macOS, Linux.
6. SQLite corrupta o disco lleno muestra error recuperable y no sobrescribe datos.
