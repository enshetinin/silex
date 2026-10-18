# ADR-0002 — Estrategia de deshacer/rehacer en el Hito 1

**Estado:** aceptada (2026-10-18).

## Contexto
El Hito 1 pide «deshacer/rehacer de texto con estrategia documentada». Hoy el editor tiene un campo de texto egui por título y por bloque; las operaciones estructurales (añadir bloque, crear página, restaurar) se guardan directamente en SQLite. egui ya trae deshacer por campo (`TextEdit` + `Undoer`), pero su historial está ligado al `Id` del widget, que por defecto depende de la **posición** en pantalla. Al añadir un bloque o cambiar de página, el historial de un bloque podía aplicarse a otro.

## Alternativas
1. **Deshacer de egui por campo, con ids estables.** Coste mínimo; atajos estándar (Cmd/Ctrl+Z, Shift+Cmd+Z, Ctrl+Y). Limitado a texto dentro de un campo y a la sesión actual.
2. **Pila de comandos propia en memoria** (cambios de texto y estructura). Permite deshacer entre bloques, pero duplica lo que hace egui para el texto y requiere un modelo de operaciones que aún no existe.
3. **Tabla `document_operations` en SQLite.** Historial persistente y base para versiones; necesita migración, política de retención y diseño del editor de bloques. Prematuro antes del Hito 3.

## Decisión
Opción 1 para el Hito 1:
- Cada campo usa `Id::unique(("silex-field", generación, tipo, id))` con el id de la página o del bloque (`src/app.rs`, `field_id`). El historial va con el bloque aunque cambie su posición.
- La `generación` sube al restaurar una copia, para que el deshacer no reintroduzca texto anterior a la restauración.
- Deshacer cambia el texto como cualquier edición: marca el documento como sin guardar y pasa por el autoguardado normal.

## Consecuencias
- No se deshacen operaciones estructurales: añadir/borrar bloques, crear páginas, papelera, restauración (la restauración tiene su propia copia previa, H1.3).
- El historial no sobrevive a reiniciar la app.
- El agrupamiento de pulsaciones lo decide egui (`Undoer`), no Silex.
- Test sin ventana (`undo_stays_with_its_block_when_blocks_move`): un bloque insertado antes no recibe el deshacer de otro.

## Migración futura
El deshacer semántico entre bloques (Hito 3) introducirá un modelo de operaciones del editor y, si hace falta persistencia, una migración aditiva con `document_operations`, con su propio ADR. Los ids estables de esta decisión se mantienen como base.
