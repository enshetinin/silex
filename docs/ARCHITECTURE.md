# Arquitectura de Silex

## Decisión técnica
Aplicación **nativa renderizada en Rust** usando egui/eframe, persistencia local SQLite por rusqlite, sin servidor ni WebView. UI immediate-mode, lógica de dominio aislada de egui, I/O encapsulada. Véase `decisions/0001-native-rust.md`.

## Módulos actuales
```
src/main.rs                 # Bootstrap del proceso, DB y ventana
src/app.rs                  # Estado y composición de interfaz egui
src/lib.rs                  # Biblioteca reutilizable
src/model.rs                # Page, Block sin dependencia GUI
src/db.rs                   # Persistencia y consultas parametrizadas
src/db/migrations/          # SQL versionado embebido en binario
tests/database.rs           # Persistencia, búsqueda y migraciones
```

## Arquitectura destino (crear solo cuando crezca)
```
crates/
  silex-domain/             # Entidades, comandos, invariantes
  silex-storage/            # SQLite, migraciones, backups, repositorios
  silex-editor/             # Modelo de documento y operaciones editables
  silex-app/                # Aplicación egui, atajos, flujos y estado UI
```
No crear el workspace de cuatro crates hasta que la complejidad lo justifique. Mantener primero un monolito modular.

## Flujo de datos
Interacción de usuario → estado UI → comando de dominio → transacción SQLite → modelo leído/actualizado → render. Un bloque no es HTML: es un registro tipado y ordenado. La futura exportación Markdown es una representación, no el formato maestro.

## Reglas de concurrencia
Actualmente las consultas son cortas y se ejecutan en el hilo UI (medido: < 1 ms con 100 páginas/1000 bloques). Los cambios pendientes se guardan en `App::on_exit`; `Drop` queda como red de seguridad. **Deuda técnica identificada:** mover operaciones que puedan tardar (adjuntos, backups, migraciones grandes, indexación masiva) a worker dedicado, intercambiando mensajes. Nunca compartir una conexión SQLite mutable sin modelo de ownership definido.

## Persistencia
- `foreign_keys=ON` por conexión.
- WAL cuando esté soportado por el archivo; `synchronous=FULL` en el starter.
- FTS5 mediante triggers para títulos y contenido de bloques.
- Esquema con `PRAGMA user_version` (v2). Migraciones encadenadas en una sola transacción, con copia verificada previa al actualizar una base existente. Se rechazan esquemas posteriores. Véase `DATA_MODEL.md`.
- Copias (`src/db/backup.rs`): API de backup de SQLite en un hilo `silex-backup` con su propia conexión. El resultado vuelve por un canal, y la UI solo lo consulta en cada frame. Se escribe en `*.partial`, se pasa a modo de diario `DELETE` (un solo archivo), se verifica (`integrity_check`, `user_version`, `integrity-check` de FTS5) y solo entonces se renombra a `backups/silex-AAAAMMDD-HHMMSS.sqlite3` (UTC). La rotación (10) solo borra archivos con ese patrón y solo después de una copia verificada. Si la app se cierra a mitad de una copia, el `.partial` se limpia en la siguiente. **Nunca copiar solo el .sqlite3 mientras la app está abierta con WAL.**
- Restauración (`Database::restore_from_backup`): verifica la copia elegida, crea una copia verificada del estado actual y solo entonces sobrescribe la base con la API de backup, que deshace la escritura en la base si no termina. Después reaplica los pragmas (WAL, FK). La copia previa aparece la primera en la lista, para poder deshacer la restauración.
- **Deuda:** la restauración corre en el hilo de la UI, porque usa la conexión principal (medido: unos 25 ms con 100 páginas/1000 bloques, incluida la copia previa). Con bases grandes habrá que moverla a un worker. Los nombres de copia tienen resolución de 1 s: dos copias en el mismo segundo devuelven un error de «inténtalo de nuevo» y no sobrescriben nada.
- Papelera (H2.3): borrado lógico por rama. La página y sus subpáginas vivas comparten un `deleted_at` único por operación (siempre posterior al último existente), que identifica el grupo al restaurar. Restaurar devuelve exactamente ese grupo; si el padre sigue en la papelera, la página vuelve a la raíz. No hay borrado definitivo (decisión del producto, 2026-10-18).

## Rendimiento
Evitar recrear conexiones cada frame, filtrar SQL para búsqueda y virtualizar páginas/bloques grandes cuando existan benchmarks. Añadir debounce sin sacrificar guardado al cerrar y recuperación ante errores.

## Privacidad y seguridad
Archivos en rutas estándar del OS; sin red, cookies, login o telemetría. No hay cifrado at-rest; documentarlo claramente. Atajos SQL y rutas de adjuntos requieren pruebas de escaping y traversal. Revisión de dependencias mediante Dependabot y auditoría futura.

## UI/UX
Diseño editorial, navegación lateral, área central legible, estados de carga/guardado/error, accesibilidad por teclado. Atajos (H2.5) se leen al inicio de cada frame con `consume_shortcut`, antes de los widgets, comprobando primero los más específicos (Cmd+Mayús+N antes que Cmd+N). Las acciones sobre páginas (clic, menú contextual, teclado) pasan por un único `RowAction` → `SilexApp::apply`.

Teclado del editor (H3.2): las operaciones son funciones puras sobre `Vec<Block>` en `src/editor.rs`, con posiciones en caracteres (como egui). En cada frame:
1. Antes de los widgets se decide si la tecla es un comando del bloque con foco. Se usa el cursor del frame anterior y la coincidencia de modificadores es exacta, para que Mayús+Intro no cuente como Intro. Si aplica, se consume.
2. No se intercepta nada durante una composición IME (`ImeEvent::Preedit` no vacía), con texto seleccionado ni en bloques de código (Intro y Tab).
3. Después del layout se ejecuta la operación con el cursor ya actualizado, por si en el mismo frame llegó texto antes de la tecla.
4. Se fija el foco y el cursor del bloque destino para el frame siguiente.

Menú `/` y selección (H3.3): el menú se calcula después del layout a partir del cursor (`editor::slash_query`) y, mientras está abierto, se queda con ↑ ↓ Intro y Esc. Esc en un bloque lo selecciona: egui borra el foco con Esc al empezar el frame, así que para esa tecla basta el cursor del frame anterior. Las teclas de selección se leen antes del layout, sin ningún campo de texto con foco; Cmd+C y Cmd+X llegan como `Event::Copy`/`Event::Cut`. Borrar una selección se deshace con Cmd+Z.

Pegar e IME (H3.5): un `Event::Paste` con varias líneas en un bloque que no sea de código se toma antes del campo de texto y se ejecuta después del layout con `editor::paste_lines`. Solo reconoce el formato de copia de Silex; el Markdown es el Hito 4. Durante una composición IME en un bloque, el Intro de confirmación se descarta, porque el IME entrega el texto como `Commit`. La marca de composición se reinicia cuando ningún bloque tiene el foco, para que no se quede atascada. La lista de pruebas manuales está en `docs/MANUAL_QA.md`.

Markdown (H4.1): `src/markdown/` convierte sin E/S. Silex no tiene formato en línea, así que la exportación escapa todo lo que Markdown podría interpretar (`\ ` * _ [ ] < > # & ! ~ |` y los marcadores de inicio de línea) y escribe como `&#32;` los espacios al principio y al final de una línea. La sangría se traduce en anidamiento de listas; un bloque sangrado sin elemento de lista encima pierde la sangría, no el texto. Los párrafos vacíos no se exportan. Importación (H4.2): `markdown::markdown_to_page` recorre los eventos de `pulldown-cmark` (CommonMark + tareas GFM). El texto llega sin escapes ni entidades. El énfasis, los enlaces, el código en línea, las imágenes y el HTML se reconstruyen como texto literal, porque Silex no tiene formato en línea. Un primer `# Título` es el título de la página. Los saltos de línea de un párrafo pasan a `\n`, y no se conserva el número de inicio de las listas ni el lenguaje del código. `src/import.rs` lee los archivos soltados en un hilo `silex-import` (máximo 10 MB, solo UTF-8) y la UI crea cada página con `Database::create_page_with_blocks` en una transacción. `src/safe_path.rs` genera nombres de archivo válidos en los tres sistemas y `join_inside` impide salir de la carpeta. `src/export.rs` escribe de forma atómica (temporal + `sync_all` + renombrado) en `Documentos/Silex/Exportaciones`, o en `SILEX_EXPORT_DIR` para pruebas.

Deshacer (H3.4, ADR 0004): todos los cambios del documento pasan por `SilexApp::changed(kind)`, que guarda en `History` la instantánea anterior (`shadow`) y programa el autoguardado. Cmd+Z, Cmd+Mayús+Z y Cmd+Y se interceptan antes que los campos de texto del documento. Mover (Alt+Mayús+↑/↓), duplicar (Cmd+D) y el menú «…» de cada bloque son operaciones puras de `src/editor.rs`. egui no replica automáticamente controles estéticos del OS: planificar esfuerzo visual explícito. egui no hace shaping ni texto bidireccional (árabe/hebreo se dibujan de izquierda a derecha y sin ligaduras); los glifos que faltan se toman de las fuentes del sistema (`system_fonts`). UX de editor rich-text de varios bloques es el componente más complejo y requiere prototipos.

## ADRs
Cada decisión irreversible (formato de documento, sincronización, cifrado, editor, arquitectura de concurrencia) tiene su propia `docs/decisions/NNNN-*.md`: problema, alternativas, decisión, consecuencias, migración.
- `0001-native-rust.md`: egui/eframe + SQLite.
- `0002-undo-strategy.md`: deshacer de texto por campo con ids estables (Hito 1); deshacer semántico en el Hito 3.
- `0003-block-model.md`: tipos de bloque ampliados y lista plana con sangría.
- `0004-document-undo.md`: deshacer/rehacer del documento con instantáneas (sustituye a 0002).
