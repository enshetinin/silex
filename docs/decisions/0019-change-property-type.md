# ADR-0019 — Cambiar el tipo de una propiedad

**Estado:** aceptada (2026-10-30), con las decisiones del propietario (valores ocultos y deshacer exacto).

## Contexto
H13.4 permite cambiar el tipo de una propiedad de una base de datos (ADR 0006), por ejemplo de «Texto» a «Número» o de «Selección» a «Selección múltiple». El ADR 0006 lo dejó fuera de la primera versión, con una condición: «con conversión explícita en una transacción y conservando el valor original».

Cada celda guarda su valor en la columna de su tipo (`text_value`, `number_value`, `bool_value`, `date_value`, `option_id`), o en `db_multi_values` si es una selección múltiple (ADR 0018). Al leer solo cuenta la columna del tipo actual; un valor en otra columna se lee como vacío. «Creado» y «Editado» (H13.3) no se guardan: salen de la página de la fila.

Hay que decidir qué pasa con los valores que no encajan en el tipo nuevo, si el cambio se puede deshacer y qué pasa con las vistas que filtran por la propiedad.

## Alternativas para los valores que no encajan
1. **Se guardan ocultos**: la conversión solo escribe en la columna del tipo nuevo y no borra ninguna otra. Un valor que no encaja deja la celda vacía, pero sigue en su columna.
2. **Copia en otra propiedad**: una propiedad de texto «Nombre (original)» con los valores que no encajaron.
3. **Se pierden al confirmar.**

La 3 va contra el ADR 0006. La 2 se ve, pero deja una columna que hay que borrar a mano. La 1 no borra nada, no necesita migración y se apoya en la regla de lectura que ya existe.

## Decisión
Opción 1, y el cambio se puede deshacer con Cmd+Z de forma exacta.

### Conversión
Una función pura, `property::convert`, decide qué es cada valor en el tipo nuevo. La regla general pasa por el texto que se ve en la celda (`CellValue::display`), con estos casos aparte:

| De → a | Resultado |
|---|---|
| Cualquiera → Texto o URL | El texto que se ve (`12,5`, «Sí», `2026-10-30`, «Novela, Cuento»). Texto ↔ URL es la misma columna. |
| → Número | Si el texto se lee como número (`parse_number`). La casilla marcada es 1. |
| → Fecha | Si el texto se lee como fecha (`Date::parse`). Una fecha, «Creado» o «Editado» dan su día. |
| → Casilla | «sí», «x», «true», «1», «✓» o «[x]» → marcada; «no», «false», «0» o «[ ]» → sin marcar; lo demás no encaja. Sin mirar mayúsculas ni acentos. |
| → Selección | Una opción con el texto que se ve: la que ya se llama así (sin mirar mayúsculas ni acentos) o una nueva, con los colores por turno. Una selección múltiple da su primera opción. |
| → Selección múltiple | Igual, pero el texto se separa por comas. Una selección simple da la misma opción. |
| → Creado o Editado | Nada se escribe: las celdas guardadas dejan de verse y la columna muestra las fechas de las filas. |

Un valor vacío no se convierte. Las opciones se crean en la misma transacción.

### Qué se escribe
- La transacción lee el valor actual de cada celda de la propiedad, también las de filas en la papelera, para que una fila restaurada no vuelva con su valor escondido.
- Si la columna del tipo nuevo ya guarda algo en esa celda, **vuelve eso** y no se escribe nada. Solo puede ser lo que la celda tenía antes con ese tipo: `set_value` limpia todas las columnas al editar una celda (y sus filas de `db_multi_values`), así que un resto solo sobrevive en una celda que nadie ha tocado desde entonces. Por eso volver al tipo anterior devuelve el valor original exacto (`12.50` sigue siendo `12.50`, no `12,5`).
- Si no, y el valor actual se convierte, se escribe **solo** la columna del tipo nuevo (en una selección múltiple, las filas de `db_multi_values` de esa celda). Las demás columnas no se tocan.
- Si no se convierte, no se escribe nada: la celda queda vacía y su valor sigue oculto en su columna.
- Texto y URL comparten `text_value`: entre ellos todo se convierte sin escribir nada.
- El tipo cambia en `db_properties` (con `updated_at`). Las filas no se marcan como editadas: «Editado» no se mueve.

### Vista previa
La misma función que planifica la conversión calcula la vista previa, así que coinciden: cuántos valores se convierten, cuántos vuelven (había algo de antes en la columna del tipo nuevo), cuántos quedan vacíos (se guardan ocultos), qué opciones se crearán y cuántos filtros se quitarán.

### Vistas
Los filtros sobre la propiedad se quitan de todas las vistas de la base: su operador o su valor (por ejemplo, el id de una opción) dejan de tener sentido. El orden, las columnas ocultas y la agrupación del tablero se mantienen; si el tablero agrupaba por ella y deja de ser una selección, ya elige otra (`board_property`).

### Deshacer
Antes de convertir se toma una foto: el tipo, las filas de `db_values` y `db_multi_values` de la propiedad tal como estaban, las opciones que se crean y la configuración de las vistas que cambian. `restore_property_kind` la devuelve tal cual en una transacción y borra las opciones creadas. La foto vive en memoria, en el deshacer de la tabla, como las demás operaciones (H6.4).

### Historias
1. **H13.4a — Modelo y base de datos:** `property::convert`, la vista previa, `change_property_kind` y `restore_property_kind`, con tests de cada conversión, ida y vuelta, deshacer exacto, todo o nada y tiempo con 10 000 filas. Sin cambios visibles.
2. **H13.4b — Interfaz:** «Cambiar tipo» en el menú de la propiedad, el diálogo con la vista previa (con una segunda confirmación si se crean más de 100 opciones), Cmd+Z y el aviso.

## Consecuencias
- Sin migración ni columnas nuevas.
- Una celda puede guardar valores en varias columnas a la vez; solo se ve la de su tipo. Tras varias conversiones seguidas sin editar, al volver a un tipo por el que ya pasó reaparece el valor que tenía con él: es intencionado (nada se pierde) y la vista previa lo cuenta como «vuelven».
- La foto para deshacer ocupa memoria proporcional a las celdas de la propiedad (unos cientos de KB con 10 000 filas).
- Convertir una propiedad son unas pocas consultas y una escritura por celda convertida, en una transacción. Se mide con 10 000 filas; si tarda demasiado para el hilo de la interfaz, irá a un hilo aparte.
