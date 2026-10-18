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
- Esquema con `PRAGMA user_version`, transacciones de migración y rechazo a esquemas posteriores.
- Copias (`src/db/backup.rs`): API de backup de SQLite en un hilo `silex-backup` con su propia conexión. El resultado vuelve por un canal, y la UI solo lo consulta en cada frame. Se escribe en `*.partial`, se pasa a modo de diario `DELETE` (un solo archivo), se verifica (`integrity_check`, `user_version`, `integrity-check` de FTS5) y solo entonces se renombra a `backups/silex-AAAAMMDD-HHMMSS.sqlite3` (UTC). La rotación (10) solo borra archivos con ese patrón y solo después de una copia verificada. Si la app se cierra a mitad de una copia, el `.partial` se limpia en la siguiente. **Nunca copiar solo el .sqlite3 mientras la app está abierta con WAL.**
- Restauración (`Database::restore_from_backup`): verifica la copia elegida, crea una copia verificada del estado actual y solo entonces sobrescribe la base con la API de backup, que deshace la escritura en la base si no termina. Después reaplica los pragmas (WAL, FK). La copia previa aparece la primera en la lista, para poder deshacer la restauración.
- **Deuda:** la restauración corre en el hilo de la UI, porque usa la conexión principal (medido: unos 25 ms con 100 páginas/1000 bloques, incluida la copia previa). Con bases grandes habrá que moverla a un worker. Los nombres de copia tienen resolución de 1 s: dos copias en el mismo segundo devuelven un error de «inténtalo de nuevo» y no sobrescriben nada.
- La papelera inicial solo oculta páginas; restauración y reglas recursivas se desarrollan después.

## Rendimiento
Evitar recrear conexiones cada frame, filtrar SQL para búsqueda y virtualizar páginas/bloques grandes cuando existan benchmarks. Añadir debounce sin sacrificar guardado al cerrar y recuperación ante errores.

## Privacidad y seguridad
Archivos en rutas estándar del OS; sin red, cookies, login o telemetría. No hay cifrado at-rest; documentarlo claramente. Atajos SQL y rutas de adjuntos requieren pruebas de escaping y traversal. Revisión de dependencias mediante Dependabot y auditoría futura.

## UI/UX
Diseño editorial, navegación lateral, área central legible, estados de carga/guardado/error, accesibilidad por teclado. egui no replica automáticamente controles estéticos del OS: planificar esfuerzo visual explícito. UX de editor rich-text de varios bloques es el componente más complejo y requiere prototipos.

## ADRs
Cada decisión irreversible (formato de documento, sincronización, cifrado, editor, arquitectura de concurrencia) tiene su propia `docs/decisions/NNNN-*.md`: problema, alternativas, decisión, consecuencias, migración.
