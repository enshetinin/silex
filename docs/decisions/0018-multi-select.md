# ADR-0018 — Propiedad de selección múltiple

**Estado:** aceptada (2026-10-30), con la decisión del propietario (tabla de unión).

## Contexto
H13.2 añade a las bases de datos (ADR 0006) un tipo de propiedad nuevo: **selección múltiple**, una celda con cero o más opciones de la propiedad. El ADR 0006 la dejó fuera de alcance. Hoy cada celda es una fila de `db_values` con un valor en la columna de su tipo, y una selección simple guarda `option_id`, con una clave foránea `ON DELETE SET NULL` que vacía la celda al borrar la opción.

Hay que decidir dónde van varias opciones por celda, y cómo se amplía el `CHECK` de `db_properties.kind`, que hoy solo admite `text`, `number`, `checkbox`, `date` y `select`.

## Alternativas
1. **Tabla de unión `db_multi_values`**: una fila por celda y opción, con claves foráneas hacia la fila, la propiedad y la opción.
2. **Lista JSON** en una columna nueva de `db_values` (`["id1", "id2"]`), con `CHECK json_valid`.

Con la lista JSON, borrar una opción deja ids sueltos en las celdas que hay que limpiar a mano en todas las filas, y nada impide guardar la opción de otra propiedad. Los filtros tendrían que abrir el JSON. Con la tabla de unión, la base garantiza la integridad como en el resto del esquema.

## Decisión
Opción 1.

### Esquema (migración `0014_property_kinds.sql`)
```sql
-- db_properties se reconstruye con los tipos nuevos en el CHECK.
kind TEXT NOT NULL CHECK (kind IN ('text', 'number', 'checkbox', 'date', 'select',
                                   'multi_select', 'url', 'created', 'edited'))

-- Una opción solo puede ser de su propiedad: clave compuesta para la FK.
CREATE UNIQUE INDEX db_select_options_property_key ON db_select_options(id, property_id);

CREATE TABLE db_multi_values (
    page_id     TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    property_id TEXT NOT NULL REFERENCES db_properties(id) ON DELETE CASCADE,
    option_id   TEXT NOT NULL,
    PRIMARY KEY (page_id, property_id, option_id),
    FOREIGN KEY (option_id, property_id)
        REFERENCES db_select_options(id, property_id) ON DELETE CASCADE
);
CREATE INDEX db_multi_values_property_idx ON db_multi_values(property_id);
CREATE INDEX db_multi_values_option_idx ON db_multi_values(option_id, property_id);
```
- **Una fila por opción elegida.** Una celda vacía no tiene filas. Borrar una opción la quita de todas las celdas (`ON DELETE CASCADE`), con la misma confirmación que una selección simple. La clave compuesta impide, en la base, guardar en una celda la opción de otra propiedad.
- **Sin orden propio dentro de la celda:** las opciones se muestran en el orden de las opciones de la propiedad (el de su menú y del tablero). No hay columna `position` que mantener, y mover una opción en el menú la mueve en todas las celdas.
- **Los tipos de H13.3 van en la misma migración.** `url`, `created` y `edited` necesitan el mismo cambio del `CHECK`. Así `db_properties` se reconstruye una sola vez. Esta versión no los crea ni los lee: un tipo que la app aún no conoce se salta al leer, como ya hacía (ADR 0006). H13.3 los implementa sin otra migración.

### Reconstruir con claves foráneas
`db_select_options` y `db_values` apuntan a `db_properties` con `ON DELETE CASCADE`. Con las claves foráneas activas, el `DROP TABLE` de la reconstrucción borraría en cascada todas las opciones y valores. Por eso el runner de migraciones pasa a seguir el procedimiento de SQLite para cambiar el esquema de una tabla:
1. `PRAGMA foreign_keys = OFF` **antes** de la transacción (dentro no tiene efecto).
2. Aplica las migraciones pendientes en una sola transacción, como siempre.
3. Antes de confirmar, `PRAGMA foreign_key_check`: si hay más violaciones que al empezar, deshace todo y no abre la base. No se exige cero, porque una base antigua con una violación previa no debe dejar de abrirse por actualizarla.
4. `PRAGMA foreign_keys = ON` otra vez, también si algo falló.

Ninguna migración anterior depende de las acciones de las claves foráneas: solo reconstruían `blocks`, y nada apunta a ella. Por eso el cambio no altera su resultado. Un test lo comprueba subiendo una base v1 hasta la última versión con datos en cada tabla.

### Comportamiento
- **Valor:** `CellValue::MultiSelect(Vec<id>)`, en el orden de las opciones; la lista vacía es una celda vacía. Escribirla sustituye las filas de esa celda en una transacción, comprobando que la propiedad está viva, que la fila es de esa base y que cada opción es de la propiedad.
- **Filtros:** «contiene» y «no contiene» una opción, «está vacío» y «no está vacío». Una fila sin opciones «no contiene» ninguna.
- **Orden:** por la primera de sus opciones (en el orden de la propiedad); las vacías, al final, como el resto de tipos.
- **Tablero:** no se agrupa por una selección múltiple: «Agrupar» solo ofrece selecciones simples (ya es así). En las tarjetas se ven todas sus opciones.
- **Markdown y CSV:** los nombres separados por comas y un espacio (`Novela, Cuento`), en el orden de las opciones.
- **Duplicar** una fila o una base copia sus celdas (en una base, con los ids nuevos de las opciones). Las plantillas no tienen propiedades (ADR 0010).
- **Deshacer** en la tabla: un cambio de celda guarda la lista anterior y la nueva, como cualquier celda (H6.4).
- **Cambiar de tipo** entre selección simple y múltiple es H13.4.

### Historias
1. **H13.2a — Modelo y migración:** el runner con claves foráneas desactivadas y comprobadas, la migración 0014 con tests v13→v14, v1→v14, fallo a mitad y copia v13, el tipo y el valor, leer y escribir celdas, borrar opciones, duplicar, filtros y orden (funciones puras), Markdown y CSV. Sin cambios visibles: el tipo todavía no se ofrece.
2. **H13.2b — Interfaz:** «Selección múltiple» en el menú «+», la celda (etiquetas de color, «+N» si no caben) con un menú de casillas, buscador y crear opción, en la tabla, la página de la fila y las tarjetas; «Poner valor…» sobre varias filas; los filtros en el menú «Filtrar».

## Consecuencias
- `db_properties` se reconstruye una vez (con sus filas, `rowid` incluido) y gana tres tipos que esta versión no usa todavía.
- El runner de migraciones desactiva las claves foráneas mientras migra y comprueba que no se rompe ninguna. Las migraciones futuras que reconstruyan tablas referenciadas pueden apoyarse en eso.
- Cargar una base es una consulta más (`db_multi_values`).
- Una celda de selección múltiple no está en `db_values`. Por eso se revisa todo lo que lee, copia o borra celdas: `database_values`, `row_values`, duplicados, exportación y CSV.
