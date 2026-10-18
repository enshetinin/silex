# Roadmap — historias verticales

> Marcadores: `[x]` significa *incluido en el starter*, no verificado por cargo en el paquete; `[ ]` pendiente. La compilación y los tests reales son el **hito 0**.

## Hito 0 — entorno reproducible
- [x] Repositorio Rust, UI inicial, esquema SQLite, CI y documentación.
- [x] Generar y versionar `Cargo.lock` con `cargo generate-lockfile`.
- [x] Ejecutar `cargo fmt --all` y después `cargo fmt --all -- --check`.
- [ ] Ejecutar `cargo clippy --workspace --all-targets -- -D warnings`. *(Verde en macOS local 2026-10-18; CI pendiente.)*
- [ ] Ejecutar `cargo test --workspace`. *(12 tests verdes en macOS local 2026-10-18; CI pendiente.)*
- [ ] Ejecutar `cargo check --workspace` en Windows, Linux y macOS. *(macOS verde; Windows/Linux pendientes de CI.)*
- [ ] Arrancar GUI y comprobar creación/edición/guardado/cierre/reapertura. *(Arranca sin errores en macOS; checklist manual pendiente.)*
- [ ] Inspección de UX y respuesta de la interfaz con 100 páginas/1000 bloques. *(Capa SQLite medida: todas las consultas < 1 ms; inspección visual pendiente.)*

**Aceptación:** todos los comandos verdes; app visible, sin pérdida de un documento tras reiniciar. **No** marcar antes de verificar.

## Hito 1 — documentos seguros
- [x] Crear backups automáticos verificables y restauración. *(H1.2: copia con la API de backup en un hilo aparte, verificada antes de darla por buena, al arrancar si la última tiene > 24 h y con botón manual; se conservan 10. H1.3: restauración con confirmación y copia previa del estado actual. Tests verdes y verificación manual en macOS.)*
- [x] Recuperación de errores de escritura, señal clara de documento sin guardar. *(H1.1: estado visible, reintento con backoff 1–30 s, cierre bloqueado con «Reintentar / Salir sin guardar». Tests verdes y verificación manual en macOS.)*
- [x] Tests de edición/cierre/apertura y manejo de fallos. *(Reapertura con Unicode, rollback de guardado fallido, `SaveState` (no cerrar con cambios pendientes, backoff), página en la papelera desde otra conexión, copias corruptas, restauración fallida sin efectos. El cierre real de la ventana se verificó a mano.)*
- [x] Acciones deshacer/rehacer de texto con estrategia documentada. *(H1.4: deshacer de egui por campo con ids estables por bloque; ADR 0002. Tests verdes y verificación manual en macOS.)*

## Hito 2 — páginas navegables
- [x] Anidación visual y reordenación de páginas; evitar ciclos. *(H2.1: árbol con subpáginas, plegar/desplegar, «+ Subpágina» y «Mover a…» con ciclos rechazados en la BD y destinos inválidos ocultos en la UI. H2.2: «Subir/Bajar» entre hermanas con renumeración transaccional. Tests verdes y verificación manual en macOS.)*
- [ ] Favoritos, renombrado contextual, papelera, restauración. *(H2.3 implementada: papelera por ramas con lista y «Restaurar»; sin borrado definitivo. Tests verdes en macOS; verificación manual pendiente. Favoritos: H2.4. Renombrado contextual: H2.5.)*
- [ ] Atajos de navegación y lista filtrable.

## Hito 3 — editor tipo Notion (primer valor diferencial)
- [ ] Comandos de edición por bloque: Enter/Backspace/Tab/Shift-Tab.
- [ ] Más tipos de bloque: H1/2/3, listas, cita y código.
- [ ] Menú `/`, teclado y selección de bloques.
- [ ] Reordenación y borrado de bloques; undo/redo semánticos.
- [ ] Pruebas de cursor/foco, copy-paste y Unicode/IME.

## Hito 4 — importación y exportación
- [ ] Markdown → bloques con pruebas golden.
- [ ] Bloques → Markdown estable.
- [ ] Exportación de espacio de trabajo; adjuntos con rutas seguras.

## Hito 5 — UX de producto
- [ ] Diseño coherente oscuro/claro, accesibilidad y navegación completa teclado.
- [ ] Búsqueda global con snippets/ranking y paleta de comandos.
- [ ] Empaquetado e iconos multiplataforma.

## Hito 6 — bases de datos tipo Notion
- [ ] Diseño separado de propiedades tipadas y vistas; ADR específica.
- [ ] Vista tabla editable, filtros y ordenación, después Kanban.

## Hito 7 — funcionalidades opcionales
- [ ] Historial/versionado, enlaces bidireccionales, plantillas.
- [ ] Evaluar cifrado y sincronización únicamente con ADR, threat model y pruebas.
