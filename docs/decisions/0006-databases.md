# ADR-0006 — Bases de datos: propiedades tipadas y vistas

**Estado:** aceptada (2026-10-18).

## Contexto
El Hito 6 pide bases de datos tipo Notion: una colección de entradas con propiedades tipadas (texto, número, fecha…), una vista de tabla editable con filtros y orden, y después una vista Kanban. Es el cambio de modelo más grande desde los bloques, y el PRD lo pone en P2: no puede comprometer la integridad de lo que ya existe.

Hoy hay **páginas** en árbol (`pages`, con papelera por ramas, favoritos, búsqueda FTS5, exportación Markdown y copias de seguridad) y **bloques** en lista plana (ADR 0003), con deshacer por instantáneas de la página abierta (ADR 0004).

Requisitos:
- **Integridad:** migración aditiva y transaccional, claves foráneas, tipos validados, nada se borra sin poder recuperarlo.
- **Encaje:** que funcione con lo que ya existe (papelera, búsqueda, copias, exportación, favoritos, mover páginas) sin reescribirlo.
- **Rendimiento:** abrir una base de 1000 entradas con 10 propiedades sin que se note (objetivo: < 50 ms en cargar y < 16 ms por frame al desplazarse; medir antes de afirmarlo).
- **Accesibilidad:** la tabla se puede usar con teclado y lector de pantalla.

## Alternativas
1. **La base de datos es una página y cada entrada es una subpágina.** Las propiedades y los valores van en tablas normalizadas (EAV tipado). Así lo hace Notion: cada entrada se abre como página con sus bloques, y la papelera, la búsqueda, los favoritos y la exportación funcionan sin cambios porque las entradas son páginas.
2. **Igual, pero con los valores en una columna JSON por página** (`pages.properties`). Menos tablas, pero sin claves foráneas hacia las propiedades ni las opciones; cambiar el nombre de una opción o borrar una propiedad obliga a reescribir el JSON de todas las filas, y validar tipos y fechas queda solo en Rust.
3. **Tablas propias, sin páginas** (`databases`, `rows`, `cells`). Más simple para la tabla, pero una entrada no puede tener contenido, y la papelera, la búsqueda y la exportación habría que duplicarlas.
4. **Una tabla SQLite real por base de datos** (`CREATE TABLE` al crearla). Rápida para filtrar, pero con DDL en tiempo de ejecución, identificadores construidos con texto del usuario y una migración por cada base al cambiar sus columnas. Descartada por seguridad e integridad.

## Decisión
Opción 1, aceptada por el propietario con las respuestas del final.

### Esquema (migración `0005_databases.sql`, aditiva)
```sql
-- Una página es una página normal o una base de datos.
ALTER TABLE pages ADD COLUMN kind TEXT NOT NULL DEFAULT 'page'
    CHECK (kind IN ('page', 'database'));

CREATE TABLE db_properties (
    id          TEXT PRIMARY KEY,
    database_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    kind        TEXT NOT NULL CHECK (kind IN ('text', 'number', 'checkbox', 'date', 'select')),
    position    INTEGER NOT NULL,
    deleted_at  TEXT,            -- borrado lógico: se puede restaurar
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX db_properties_database_idx ON db_properties(database_id, position);

CREATE TABLE db_select_options (
    id          TEXT PRIMARY KEY,
    property_id TEXT NOT NULL REFERENCES db_properties(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    color       TEXT NOT NULL CHECK (color IN ('gray', 'blue', 'green', 'yellow', 'orange', 'red', 'purple', 'pink')),
    position    INTEGER NOT NULL
);

-- Un valor por entrada y propiedad, en la columna de su tipo.
CREATE TABLE db_values (
    page_id      TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    property_id  TEXT NOT NULL REFERENCES db_properties(id) ON DELETE CASCADE,
    text_value   TEXT,
    number_value REAL,
    bool_value   INTEGER CHECK (bool_value IN (0, 1)),
    date_value   TEXT CHECK (date_value IS NULL OR date(date_value) IS date_value),
    option_id    TEXT REFERENCES db_select_options(id) ON DELETE SET NULL,
    updated_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (page_id, property_id)
);
CREATE INDEX db_values_property_idx ON db_values(property_id);

CREATE TABLE db_views (
    id          TEXT PRIMARY KEY,
    database_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    kind        TEXT NOT NULL CHECK (kind IN ('table', 'board')),
    position    INTEGER NOT NULL,
    config      TEXT NOT NULL DEFAULT '{}'   -- JSON validado en Rust
);
```
- **Las filas son páginas** con `parent_id` = la base de datos. La columna «Nombre» es su título: siempre la primera y no se puede borrar. El orden manual de las filas es `sort_order`, el mismo que ya usan las subpáginas.
- **Un valor por celda**, en la columna de su tipo. Que la columna coincida con el tipo de la propiedad no se puede comprobar con un `CHECK` (está en otra tabla): lo valida Rust al escribir, y al leer un valor que no encaja se trata como vacío, nunca como pánico (igual que `BlockKind`). Las fechas son `AAAA-MM-DD` sin hora, y SQLite rechaza las imposibles (`date('2026-02-30')` no es `'2026-02-30'`). Los números son `REAL` (f64).
- **Las opciones de selección** son filas propias: cambiar su nombre o color no toca las celdas. Borrar una opción deja vacías sus celdas (`ON DELETE SET NULL`), con confirmación.
- **Borrar una propiedad es lógico** (`deleted_at`): sus valores se conservan y se puede restaurar desde el menú de la base de datos. Las páginas nunca se borran físicamente, así que los `ON DELETE CASCADE` son solo una red de seguridad.
- **Configuración de las vistas**: un JSON con orden, filtros, propiedades ocultas, anchos de columna y, en Kanban, la propiedad de agrupación. Se valida al leer como los ajustes (`settings`): claves desconocidas o valores inválidos se ignoran y se usa el valor por defecto; una vista dañada nunca impide abrir la base.
- **Cambiar el tipo de una propiedad** queda fuera de la primera versión: se crea una propiedad nueva. Si se añade, será con conversión explícita en una transacción y conservando el valor original.

### Encaje con lo que ya existe
- **Árbol:** la base de datos aparece en la barra lateral con su icono; sus filas no (como en Notion), y se abren desde la tabla, la búsqueda o los favoritos. Las subpáginas de una fila son subpáginas normales de esa fila.
- **Papelera:** sin cambios. Enviar la base a la papelera se lleva sus filas como una rama, y restaurarla las devuelve con sus valores, que nunca se borraron.
- **Mover:** una base de datos no es destino de «Mover a…» en la primera versión. Sacar una fila de la base la convierte en página normal; sus valores se conservan y vuelven a verse si regresa.
- **Búsqueda:** las filas son páginas, así que se encuentran por título y contenido, con la ruta «Base › Fila». Los valores de las propiedades no se indexan en la primera versión (haría falta tocar FTS5 con su migración).
- **Copias de seguridad:** sin cambios, las tablas nuevas van en la misma base. Una copia v4 se restaura y se actualiza a v5 por el mecanismo existente.
- **Exportación Markdown:** la base de datos se exporta como `Base.md` con una tabla GFM (Nombre y una columna por propiedad visible de la primera vista; la selección como su nombre, la casilla como `[x]`/`[ ]`, las fechas en ISO), y sus filas como subpáginas en la carpeta `Base/`, igual que ahora. No hay importación de bases de datos.
- **Deshacer:** en la primera versión cada cambio en la tabla se guarda al momento en su propia transacción, y Cmd+Z deshace el texto mientras se edita una celda. Las operaciones destructivas se recuperan por su propio camino (fila → papelera, propiedad → restaurar). El deshacer de la tabla llegará en una historia propia, con operaciones inversas (pocas y bien definidas: valor anterior de una celda, crear y quitar filas o propiedades), no con instantáneas de toda la base.

### Interfaz
- **Crear:** «Nueva base de datos» desde el «+» de la barra lateral, la paleta de comandos y el menú «…». Nace con la propiedad Nombre y una vista «Tabla». No se puede convertir una página existente en base de datos (por ahora).
- **Página de la base:** la vista sustituye al editor de bloques. Lleva la barra superior de siempre, el título y las pestañas de vistas.
- **Tabla:**
  - Cabecera con el icono del tipo y el nombre de cada propiedad, y un menú para renombrar, ocultar o borrar.
  - Celdas editables según el tipo: texto en línea; número validado; casilla; fecha `AAAA-MM-DD` validada (sin selector de calendario, que exigiría una dependencia nueva); selección con buscador y creación de opciones.
  - «Nueva fila», y abrir una fila como página.
  - Filas virtualizadas (`ScrollArea::show_rows`) para miles de entradas.
- **Página de una fila:** las propiedades se muestran y editan encima de sus bloques.
- **Teclado y accesibilidad:** las flechas, Tab e Intro recorren y editan las celdas, y cada celda se anuncia como «Propiedad, fila: valor».
- **Filtros y orden:** se evalúan en Rust sobre las filas cargadas (funciones puras con tests), no en SQL. Con 1000 a 10 000 entradas es suficiente; se medirá, y si hace falta se llevará a SQL.

### Historias propuestas
1. **H6.1 — Crear bases de datos y filas.**
   - Migración 0005 completa, con tests v4→v5, v1→v5 y de restaurar una copia v4.
   - Crear una base, la tabla solo con la columna Nombre, añadir filas, abrirlas y mandarlas a la papelera.
   - Filas ocultas en el árbol; ruta en la búsqueda; exportación con la tabla de nombres.
2. **H6.2 — Propiedades tipadas.** Añadir, renombrar, ordenar, ocultar y borrar con restauración; editar las celdas de los cinco tipos; opciones de selección con color; propiedades en la página de la fila.
   - *Actualización (2026-10-19):* se entrega en dos partes. **H6.2a**: propiedades en la tabla (añadir, renombrar, mover, borrar y restaurar), las celdas de los cinco tipos y las opciones creadas desde la celda. **H6.2b**: propiedades en la página de la fila, recorrer las celdas con flechas, Tab e Intro, y renombrar, cambiar de color o borrar opciones. «Ocultar» pasa a H6.3, porque vive en la configuración de la vista que H6.3 diseña junto con el orden y los filtros.
3. **H6.3 — Ordenar y filtrar.** Por vista y guardado; varias vistas por base, en pestañas; ocultar propiedades por vista.
4. **H6.4 — Deshacer en la tabla** (operaciones inversas) y exportación con todas las propiedades.
5. **H6.5 — Vista Kanban** agrupada por una propiedad de selección; arrastrar tarjetas entre columnas cambia el valor.

## Consecuencias
- **Una columna y cuatro tablas más.** `Page` gana un `kind`, que hay que propagar a sus 14 construcciones.
- **El esquema queda fijado al publicarse.** La migración 0005 lleva el esquema completo de este ADR desde H6.1, y si una historia posterior necesita otra columna, será una migración 0006 aditiva.
- **Valores en filas separadas (EAV).** Cargar una base son cuatro consultas (filas, propiedades, opciones y valores), no una por celda.
- **Números como `REAL`.** Sumas tipo moneda pueden mostrar errores de redondeo; si hace falta, un tipo «moneda» guardado en céntimos.
- **Fuera de alcance**, cada uno con su ADR si llega: bases de datos dentro de una página (como bloque), relaciones, fórmulas y rollups, selección múltiple, personas, adjuntos, plantillas de filas, importar CSV.

## Decisiones del propietario (2026-10-18)
1. Las filas no aparecen en la barra lateral, como en Notion.
2. La tabla empieza sin deshacer propio; llega en H6.4.
3. Primera versión con cinco tipos: texto, número, casilla, fecha y selección. La selección múltiple sería la siguiente.
