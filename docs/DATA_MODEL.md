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
`page_search` y `block_search` son tablas virtuales FTS5 external-content que consultan el `rowid` de tablas normales. Triggers mantienen índices tras inserciones, cambios y eliminaciones. Los resultados filtran páginas con `deleted_at IS NULL`. La búsqueda global (H5.7, `Database::search`) usa prefijos, `bm25`, `snippet()` y `highlight()` sobre estas mismas tablas, sin cambiar el esquema (véase `ARCHITECTURE.md`).

## Versión 2 — favoritos (`0002_favorites.sql`)
- `pages.favorite_at TEXT NULL`: `NULL` significa que no es favorita; la fecha ordena la lista de favoritos (de más antigua a más reciente). Índice parcial `pages_favorite_idx`.
- Las favoritas en la papelera se ocultan y vuelven al restaurar la página.

## Versión 3 — tipos de bloque y sangría (`0003_block_kinds.sql`, ADR 0003)
- `blocks.kind` ∈ `paragraph`, `heading_1`, `heading_2`, `heading_3`, `todo`, `bulleted`, `numbered`, `quote`, `code`. Los `heading` de v1/v2 pasan a `heading_1`. En Rust es `BlockKind`; un nombre desconocido al leer es un error, no un pánico.
- `blocks.indent INTEGER NOT NULL DEFAULT 0 CHECK (indent BETWEEN 0 AND 6)`: lista plana con sangría, sin árbol de bloques.
- La tabla se reconstruye (SQLite no permite cambiar un `CHECK`): copia los `rowid` explícitamente, recrea índice y triggers FTS y hace `rebuild` de `block_search`. Hay tests con un hueco de `rowid` y un fallo a mitad que deshace todo.
- La numeración de listas se calcula al pintar (`list_numbers`), no se guarda.
- `save_document` sincroniza la lista completa de la página (inserta, actualiza, borra, `position` = índice) en una transacción y solo toca filas cambiadas, para no reindexar FTS de más.

## Versión 4 — ajustes (`0004_settings.sql`)
- `settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)`: preferencias clave-valor (`theme` = `system`/`light`/`dark`, `zoom_percent` entre 50 y 200). Una preferencia nueva no necesita migración.
- Rust valida cada valor al leer (`settings::Settings::from_pairs`): una clave desconocida o un valor inválido se ignoran y se usa el valor por defecto; nunca impiden arrancar.
- Otras claves, que `Settings::from_pairs` ignora: `open_tabs` (ADR 0011), `recent_pages` (H11.8, `{"pages": [ids]}`, la más reciente primero, como mucho 20) y `last_view:<id de la base>` (H11.6), con el id de la última vista abierta en esa base. Esta se borra con la base al borrarla para siempre (ADR 0013); si apunta a una vista ya borrada, la base se abre en la primera.
- Los ajustes viven en la base, así que entran en las copias de seguridad: restaurar una copia restaura también sus ajustes.

## Versión 5 — bases de datos (`0005_databases.sql`, ADR 0006)
- `pages.kind TEXT NOT NULL DEFAULT 'page' CHECK (kind IN ('page', 'database'))`: se añade sin reconstruir `pages`, y las páginas existentes quedan como `page`. En Rust es `PageKind`; un tipo desconocido al leer es un error, no un pánico.
- Una base de datos es una página de tipo `database`; sus **filas** son sus subpáginas vivas (`Database::database_rows`, en el orden de `sort_order`). Una base de datos no es destino de `move_page` ni puede crearse dentro de otra (las dos cosas se rechazan en la base, no solo en la interfaz).
- `db_properties`, `db_select_options`, `db_values` y `db_views` llevan el esquema completo del ADR: propiedades tipadas con borrado lógico, opciones de selección con color, un valor por celda en la columna de su tipo (fechas `AAAA-MM-DD` comprobadas con `date(x) IS x`) y vistas con configuración JSON. `create_database` crea la página y su vista «Tabla» en una transacción. H6.1 usa `pages.kind` y `db_views`.
- Propiedades y valores (H6.2a, sin migración): `add_property`, `rename_property`, `move_property` (renumera las vivas 0..n en una transacción), `delete_property` (borrado lógico) y `restore_property` (vuelve como última columna). Un valor por celda (`set_value`): vacío (`None`, texto vacío o casilla sin marcar) es borrar la fila de `db_values`; si no, `INSERT … ON CONFLICT DO UPDATE` que deja `NULL` las otras columnas. Antes de escribir se comprueba, en la misma transacción, que la propiedad está viva, que la fila es una página viva de esa base, que el valor es de su tipo, que un número es finito y que una opción es de esa propiedad; si algo falla no se escribe nada. `database_values` lee todas las celdas de las filas y propiedades vivas en una consulta (las de una propiedad borrada o una fila en la papelera no salen, pero siguen guardadas) y `CellValue::from_columns` solo acepta la columna del tipo de la propiedad: lo demás se lee como vacío, nunca como error. Un tipo de propiedad desconocido se salta. Las opciones de selección se crean con `add_select_option` (solo en propiedades de selección vivas) y su color va en turno.
- Opciones y una fila (H6.2b, sin migración): `rename_select_option`, `set_option_color` y `delete_select_option`; borrar una opción vacía sus celdas por el `ON DELETE SET NULL` de `db_values.option_id` (la fila de `db_values` queda con todas las columnas a `NULL` y se lee como vacía). `row_values` lee los valores de una sola fila viva para las propiedades vivas de su base (la página de la fila), sin cargar los de toda la tabla.
- Configuración de una vista (H6.3a, sin migración): `db_views.config` es un objeto JSON `{"sort": [{"property", "direction": "asc"|"desc"}], "filters": [{"property", "op", "value"}], "hidden": [id…]}`, donde `property` es el id de una propiedad o `"name"` (la columna «Nombre»). `view::ViewConfig::from_json` nunca falla: JSON inválido → configuración por defecto; reglas con operador o sentido desconocidos se descartan; las claves de primer nivel desconocidas (p. ej. `widths` o `group_by` de una versión futura) se conservan al volver a escribir. Las reglas sobre propiedades borradas se guardan y no hacen nada hasta que se restauran. `set_view_config` escribe la vista entera. Varias vistas (H6.3b): `add_view` (al final, vacía o con la configuración de otra al duplicar), `rename_view`, `move_view` (renumera 0..n en una transacción) y `delete_view`, que borra solo la configuración y rechaza la última vista de una base.
- Exportación (H6.4): `markdown::database_to_markdown` recibe las propiedades vivas, las filas y sus valores; la casilla se escribe sin escapar (`[x]`/`[ ]`), el resto como se muestra en la app y escapado.
- `src/property.rs` (sin SQL ni egui) define los tipos y cómo el texto escrito en una celda se convierte en valor: números con coma o punto decimal, sin separador de miles ni exponentes, siempre finitos; fechas reales entre 0001 y 9999 en `AAAA-MM-DD` o `D/M/AAAA`, guardadas como `AAAA-MM-DD`.
- Probado: v4→v5 y v1→v5 conservando todo, un fallo a mitad (con `pages.kind` ya añadida) que deja la v4 intacta, y una copia v4 restaurada y actualizada.

## Versión 6 — adjuntos (`0006_attachments.sql`, ADR 0007)
- `attachments (id, name, mime, size, width, height, created_at, data)`: el archivo como BLOB, como mucho 20 MB (`CHECK (size BETWEEN 0 AND 20971520)` y `CHECK (length(data) = size)`). `data` es la última columna y solo se lee por id (`attachment_data`); `attachment` y `page_attachments` leen los metadatos.
- `blocks` se reconstruye como en la 0003, conservando los `rowid` y con `rebuild` de `block_search`. Cambia en dos cosas:
  - el tipo `file` en el `CHECK` de `kind`;
  - `attachment_id TEXT REFERENCES attachments(id) ON DELETE RESTRICT`, con `CHECK ((kind = 'file') = (attachment_id IS NOT NULL))` e índice parcial `blocks_attachment_idx`.
- El `content` de un bloque `file` es el nombre que se ve, así que la búsqueda lo encuentra sin tocar FTS5.
- `save_document` compara y escribe `attachment_id`. Rechaza un bloque `file` sin archivo, uno de texto con archivo y un archivo que no existe (clave foránea), sin cambiar nada de la página. `add_block` y `create_page_with_blocks` no crean bloques `file`.
- **Borrar un bloque no borra su adjunto:** deshacer puede devolverlo, y la papelera conserva los de sus páginas. Un adjunto en uso no se puede borrar (`RESTRICT`).
- `src/attachment.rs` (sin SQL ni egui) decide el tipo por la firma del contenido y lee las dimensiones de PNG, JPEG y GIF de su cabecera, sin decodificar. También limpia los nombres y reconoce programas y scripts (por extensión, también con puntos finales, o por contenido).
- Espacio (H7.4, sin migración): `space_usage` suma tamaños y páginas libres sin leer ningún blob, y `delete_unused_attachments(keep)` borra en una transacción los adjuntos que no usa ningún bloque (de páginas vivas o en la papelera), salvo los de `keep`. Después, `compact` devuelve el espacio al disco.
- Probado: v5→v6 y v1→v6 conservando los `rowid`, un fallo a mitad (con `blocks` ya reconstruida) que deja la v5 intacta, una copia v5 restaurada y actualizada, y las copias de seguridad con adjuntos.

## Versión 7 — enlaces entre páginas (`0007_page_links.sql`, ADR 0008)
- `blocks` se reconstruye por tercera vez (como en la 0003 y la 0006, conservando los `rowid` y con `rebuild` de `block_search`). Cambia en dos cosas:
  - el tipo `page_link` en el `CHECK` de `kind`;
  - `link_page_id TEXT REFERENCES pages(id) ON DELETE SET NULL`, con `CHECK (kind = 'page_link' OR link_page_id IS NULL)` e índice parcial `blocks_link_idx`, que sirve para los backlinks.
- El `content` de un enlace es el título de su destino, para la búsqueda y la exportación. `save_document` lo toma del título actual al guardar, y `rename_page` y `save_document` (al cambiar el título de la página) actualizan los enlaces que apuntan a ella, en la misma transacción.
- `page_link_targets` lee los destinos de los enlaces de una página, también los que están en la papelera.
- `backlinks` (H7.6) lee las páginas vivas que enlazan a una, sin ella misma, con `blocks_link_idx`. No cambia el esquema.
- Probado: v6→v7 y v1→v7 conservando los `rowid` y los adjuntos, un fallo a mitad que deja la v6 intacta, y una copia v6 restaurada y actualizada.

## Versión 8 — historial de versiones (`0008_page_versions.sql`, ADR 0009)
- `page_versions (id, page_id, created_at, title, format, blocks, size)`: una instantánea de una página. `blocks` es una lista JSON (`format` 1, de `src/versions.rs`) con `CHECK (json_valid(blocks))`, y `size` son sus bytes (`CHECK (size = octet_length(blocks))`). `page_id` con `ON DELETE CASCADE` (desde el ADR 0013, la papelera puede borrar páginas para siempre y la cascada se lleva sus versiones) e índice `page_versions_page_idx (page_id, created_at)`. Solo se crea la tabla: no se reconstruye nada.
- `save_document` guarda, en su misma transacción, cómo estaba la página **antes** del guardado si algo cambia (título o bloques), si su última versión tiene más de 10 minutos y si no es igual a ella. Una página sin bloques, o con todos vacíos (sin texto, archivo ni enlace), no se guarda como versión. Después poda las de esa página (`versions::to_prune`): todas las de 24 horas, la última de cada día (UTC) hasta 30 días y la última de cada semana (de lunes a domingo) después.
- `save_version` guarda el estado actual sin esperar los 10 minutos (antes de restaurar, H7.8). `page_versions` las lista (la más reciente primero) y `version_blocks` lee una: un formato o tipo de bloque desconocido, o datos dañados, dan un mensaje para esa versión, nunca un error que oculte las demás.
- Interfaz (H7.8, sin migración): `page_versions` da también la hora local (`strftime(…, 'localtime')`), y `local_today` el día de hoy en hora local. `restorable_blocks` prepara los bloques de una versión para guardarlos: un archivo que ya no está pasa a párrafo («[archivo que ya no está] nombre»), un enlace a una página que no existe pierde su página, y un id que ahora usa otra página recibe uno nuevo.
- **Adjuntos:** un adjunto que nombra alguna versión (`json_each`) cuenta como en uso en `space_usage` y `delete_unused_attachments`, así que «Liberar espacio» no lo borra.
- Probado: v7→v8 y v1→v8 conservando los `rowid`, adjuntos y enlaces; un fallo a mitad que deja la v7 intacta; una copia v7 restaurada y actualizada; y la base pasa la verificación de copias con versiones.

## Versión 9 — plantillas (`0009_templates.sql`, ADR 0010)
- `pages.template_at TEXT NULL`: la fecha en que una página pasó a ser plantilla; `NULL` en las demás. Índice parcial `pages_template_idx`. Solo añade la columna: no reconstruye nada.
- Una plantilla es siempre una página de nivel superior y de tipo `page`. Rust lo garantiza: no se puede mover, ni crear nada dentro de ella (subpágina, base o página importada), ni guardar como plantilla una base, una fila u otra plantilla, ni usarla dentro de una base.
- `list_pages`, `list_favorites`, `search_pages`, `search` y `backlinks` dejan fuera las plantillas; `set_favorite` las rechaza; `reorder_page` ordena las plantillas entre plantillas y las páginas entre páginas. La papelera las lista como cualquier página y vuelven siendo plantillas.
- `save_as_template` copia el título y los bloques de una página en una plantilla nueva; `create_from_template` copia una plantilla viva en una página nueva (al final de su destino) y cambia `{{fecha}}` en el título y el texto por la fecha local en largo («20 oct 2026»; `src/templates.rs`). Los bloques reciben ids nuevos y comparten adjuntos y destinos de enlace; el texto de un enlace no se toca. Todo en una transacción.
- `create_example_templates` crea las tres de ejemplo una sola vez: la clave `example_templates` de `settings` lo recuerda, aunque se manden a la papelera (`Settings::from_pairs` ignora la clave).
- «Exportar todo» escribe las plantillas en `Plantillas/` (con número si una página ya se llama así), con sus adjuntos.
- Probado: v8→v9 y v1→v9 conservando los `rowid`, las versiones y los favoritos; un fallo a mitad que deja la v8 intacta; una copia v8 restaurada y actualizada.

## Evolución
- Las migraciones publicadas (`0001`…`0009`) son inmutables. Un cambio nuevo es un archivo `000N_*.sql` añadido a `MIGRATIONS` en `src/db.rs`, más tests desde una base de la versión anterior (`tests/migrations.rs`).
- Todas las migraciones pendientes se aplican en **una sola transacción**: la base queda actualizada del todo o exactamente como estaba (test con un fallo a mitad de la `0002`).
- Antes de actualizar una base existente en disco se crea una copia verificada con el esquema antiguo. Si esa copia falla, no se migra.
- Las copias de esquemas anteriores son válidas: la verificación acepta `1..=LATEST` y la restauración actualiza la base después de restaurar.
- Evolucionar a `document_operations` para undo/redo e historial si existen requisitos concretos; no convertir el editor en CRDT prematuramente.

## Riesgos conocidos del starter
- Los valores `position` usan enteros consecutivos. Reordenar bloques requerirá transacciones y renumeración o fractional indexing.
- Anidación: `Database::move_page` rechaza ciclos con una consulta recursiva dentro de la transacción (comprobación de la app, no un trigger). Con la papelera por ramas (H2.3) ya no se crean páginas huérfanas; si existieran (datos antiguos o borrados desde otra conexión), se muestran en la raíz. Si los datos contuvieran un ciclo, `build_tree` no entra en bucle y muestra esas páginas en la raíz.
- SQLite no está cifrada.
- Falta backup/restauración con interfaz de usuario.
- Búsqueda (H5.7): prefijos, orden por `bm25`, fragmentos y sin acentos. Límites: no encuentra texto en mitad de una palabra («tilo» no encuentra «estilo»), ni páginas con unas palabras en el título y otras en un bloque. Las búsquedas con muchas coincidencias tardan ~80 ms con 50 000 bloques (en un hilo aparte, sin bloquear la UI); un índice de prefijos (`prefix='2 3'`) podría acelerarlo (sin medir), pero exige reconstruir las tablas FTS con una migración.
- Papelera por ramas sin columna nueva: el grupo se identifica por `deleted_at` (resolución de ms, único por operación). Una subpágina borrada antes que su padre es un elemento aparte en la papelera y no vuelve al restaurar al padre. Si en el futuro se necesitan más metadatos del borrado (quién o por qué), hará falta una tabla `trash_batches` con su migración.
- FTS external-content se enlaza por el `rowid` implícito de `pages`/`blocks` (PK `TEXT`). SQLite documenta que `VACUUM` puede cambiar esos `rowid`. Por eso las copias usan la API de backup (copia páginas y conserva `rowid`), no `VACUUM INTO`, y cada copia pasa el `integrity-check` de FTS5 contra su tabla de contenido (test `verify_detects_search_index_out_of_sync`). No ejecutar `VACUUM` sin un `rebuild` posterior del índice: `Database::compact` (H7.4) lo hace, con una marca en `settings` (`compacting`) que `Database::open` usa para terminar el `rebuild` si se interrumpió.
- Si un guardado falla, los cambios siguen en memoria, el error se muestra y se reintenta con backoff (`src/save_state.rs`). Cerrar la ventana con cambios sin guardar se bloquea hasta reintentar con éxito o descartar explícitamente. Si el proceso termina sin petición de cierre (p. ej. lo mata el sistema), solo queda el último guardado automático.

## Tests mínimos posteriores
1. Migración de datos existentes preserva títulos, bloques y relaciones.
2. FTS encuentra inserts, updates y deja de encontrar deletes.
3. Fallo de transacción deja intacta la página.
4. Guardado al cerrar/no pérdida ante errores de disco.
5. Rutas de backup consistentes en Windows, macOS, Linux.
6. SQLite corrupta o disco lleno muestra error recuperable y no sobrescribe datos.

## Borrar para siempre (H11.3, ADR 0013, sin migración)
- `purge_trash_item(id)`: borra en una transacción la cabeza de un elemento de la papelera y lo que se envió con ella (mismo `deleted_at`). Rechaza páginas vivas y páginas de dentro de una rama. `empty_trash()` borra todas las páginas con `deleted_at`.
- Las cascadas existentes hacen el resto: `blocks`, `page_versions`, `db_values` y, de una base, `db_properties`, `db_select_options` y `db_views`. Los disparadores `AFTER DELETE` mantienen `page_search` y `block_search`, también en las cascadas. `blocks.link_page_id` pasa a `NULL` («Página que ya no existe») y `pages.parent_id` de elementos sueltos, también.
- `save_document` guarda como `NULL` un `link_page_id` que ya no existe, en lugar de fallar la clave foránea.
- Los adjuntos quedan sin uso y los borra «Liberar espacio». Las copias de seguridad anteriores conservan lo borrado.

## Duplicar (H11.4, sin migración)
- `duplicate_page(id, con_hijos)` y `duplicate_pages(ids)` copian en una transacción: página con `kind` y bloques nuevos (mismos `attachment_id` y `link_page_id`), `db_values` de las filas y, de una base, `db_properties` (también las borradas, con su `deleted_at`), `db_select_options` y `db_views`. En `db_views.config` (JSON) se sustituye cada id viejo de propiedad u opción por el nuevo; los UUID no se repiten, así que el reemplazo de texto es seguro.
- La copia va justo debajo del original: se renumera `sort_order` de las hermanas vivas. No se copian `favorite_at`, `template_at` ni `page_versions`.
