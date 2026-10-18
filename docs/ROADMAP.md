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
- [x] Favoritos, renombrado contextual, papelera, restauración. *(H2.3: papelera por ramas con «Restaurar», sin borrado definitivo. H2.4: favoritos con migración aditiva 0002 y copias v1 restaurables. H2.5: menú contextual en cada página y renombrado en línea (F2). Tests verdes y verificación manual en macOS.)*
- [x] Atajos de navegación y lista filtrable. *(H2.5: Cmd/Ctrl+N, Cmd/Ctrl+Mayús+N, Cmd/Ctrl+K, Alt+↑/↓, F2; la búsqueda filtra el árbol por título y lista aparte las coincidencias en el contenido. Tests verdes y verificación manual en macOS.)*

## Hito 3 — editor tipo Notion (primer valor diferencial)

**Cerrado (2026-10-18):** tests verdes y verificación manual en macOS, incluidas las pruebas de `docs/MANUAL_QA.md`. Windows y Linux: cubiertos por la CI de cada PR; las pruebas manuales en esos sistemas siguen pendientes.

- [x] Comandos de edición por bloque: Enter/Backspace/Tab/Shift-Tab. *(H3.2: Intro divide (Mayús+Intro y bloques de código: salto de línea), Retroceso al inicio convierte, quita sangría o fusiona, Supr al final une, Tab/Mayús+Tab con límite de un nivel, ↑/↓ entre bloques; operaciones puras en `src/editor.rs`; sin interceptar durante IME.)*
- [x] Más tipos de bloque: H1/2/3, listas, cita y código. *(H3.1: migración 0003 (reconstrucción de `blocks` conservando `rowid` y FTS), `BlockKind`, sangría guardada, guardado por sincronización completa y selector de tipo; ADR 0003.)*
- [x] Menú `/`, teclado y selección de bloques. *(H3.3: «/palabra» abre el menú de tipos (alias, sin acentos, ↑ ↓ Intro, Esc); Esc selecciona bloques, Mayús+↑/↓ amplía, Retroceso/Supr borra, Cmd+C/Cmd+X copia o corta.)*
- [x] Reordenación y borrado de bloques; undo/redo semánticos. *(H3.4: Alt+Mayús+↑/↓ mueve, Cmd+D duplica, menú «…» por bloque; deshacer/rehacer de documento con instantáneas, agrupando el tecleo; ADR 0004 sustituye a 0002.)*
- [x] Pruebas de cursor/foco, copy-paste y Unicode/IME. *(H3.5: pegar varias líneas crea bloques y reconstruye el formato de copia de Silex; integridad Unicode en todas las posiciones; Intro de confirmación del IME descartado durante la composición; pruebas manuales en `docs/MANUAL_QA.md`.)*

## Hito 4 — importación y exportación

**Cerrado (2026-10-18):** tests verdes y verificación manual en macOS. Windows y Linux: cubiertos por la CI de cada PR; las pruebas manuales en esos sistemas siguen pendientes. Los adjuntos pasan al Hito 7, porque Silex aún no tiene adjuntos.

- [x] Markdown → bloques con pruebas golden. *(H4.2: `markdown::markdown_to_page` con `pulldown-cmark` (CommonMark + tareas GFM); el formato en línea que Silex no tiene se conserva como texto; golden de Markdown estilo Obsidian, GitHub y casos límite; ida y vuelta exacta y exportación estable; importar arrastrando `.md` a la ventana, en un hilo aparte, una transacción por página, máximo 10 MB y solo UTF-8. Tests verdes y verificación manual en macOS.)*
- [x] Bloques → Markdown estable. *(H4.1: `markdown::page_to_markdown` (CommonMark + tareas GFM, todo el texto escapado para volver idéntico), archivo golden y comprobación con `pulldown-cmark`; «Exportar a Markdown» a `Documentos/Silex/Exportaciones` con nombres seguros y escritura atómica. Tests verdes y verificación manual en macOS.)*
- [x] Exportación de espacio de trabajo; adjuntos con rutas seguras. *(H4.3: «Exportar todo a Markdown» crea `Silex <fecha>/` con un `.md` por página y una carpeta por página con subpáginas, sin la papelera, en un hilo aparte, a través de una carpeta temporal que se renombra al final; nombres seguros y sin choques entre archivo y carpeta (`safe_path::unique_stem`). Medido: 100 páginas en ~0,4 s, y al reimportarlas salen idénticas. Adjuntos: Silex no tiene adjuntos todavía; las rutas seguras (`safe_path`) quedan listas y los adjuntos pasan al Hito 7. Tests verdes y verificación manual en macOS.)*

## Hito 5 — UX de producto
- [ ] Diseño coherente oscuro/claro, accesibilidad y navegación completa teclado. *(H5.1: `app.rs` dividido en `src/app/` sin cambios de comportamiento. H5.2: sistema de diseño en `src/app/theme.rs` (tokens claro/oscuro con contraste AA comprobado por test, espacios, radios, tipografía), Inter y Phosphor incluidas (`assets/fonts/`), el tema sigue al sistema, y capturas con `egui_kittest` (`SILEX_SNAPSHOTS=1`). H5.3: barra lateral estilo Asana (fondo oscuro, «+ Nueva página» que abre la página con el título listo para escribir, buscador con icono, secciones Favoritos/Páginas, filas con «+» y «…» al pasar el ratón, pie con Papelera, Copias y Atajos, ancho ajustable). H5.4: la página: barra superior con migas, estado de guardado discreto, deshacer/rehacer, favorito y menú «…» (que sustituye a las filas de botones); columna centrada de 720 px; título grande; bloques sin bordes con un asa al pasar el ratón (convertir, mover, duplicar, eliminar); «Añadir bloque»; avisos como banners. H5.5: accesibilidad y ajustes: todos los controles tienen nombre accesible (también los de solo icono), con un test que recorre varios estados y corre siempre en CI; el foco de teclado se ve en la barra lateral; «Ajustes» con tema (Sistema/Claro/Oscuro) y tamaño de la interfaz (80–150 %, también con Cmd+/−/0), guardados en la tabla `settings` (migración 0004). Pendiente: probar con un lector de pantalla real (VoiceOver/NVDA/Orca).)*
- [ ] Búsqueda global con snippets/ranking y paleta de comandos. *(H5.7: ventana «Buscar» (Cmd+K o el botón de la barra lateral), que sustituye al filtro del árbol de H2.5. Busca cada palabra como prefijo, sin distinguir mayúsculas ni acentos; primero las coincidencias en el título y luego en el contenido, ordenadas por `bm25`; cada resultado muestra la ruta de la página y un fragmento del mejor bloque con la coincidencia resaltada. ↑/↓ e Intro; al abrir, el cursor va a la coincidencia. Sin migración. Las consultas corren en un hilo `silex-search` con su propia conexión (medido en release: ~80 ms con 50 000 bloques), solo cuenta la última. Tests verdes y capturas en macOS; verificación manual pendiente. Pendiente: paleta de comandos (H5.8).)*
- [ ] Empaquetado e iconos multiplataforma.

## Hito 6 — bases de datos tipo Notion
- [ ] Diseño separado de propiedades tipadas y vistas; ADR específica.
- [ ] Vista tabla editable, filtros y ordenación, después Kanban.

## Hito 7 — funcionalidades opcionales
- [ ] Historial/versionado, enlaces bidireccionales, plantillas.
- [ ] Adjuntos locales (P1 del PRD), con un ADR propio: almacenamiento, límites de tamaño, copias de seguridad y exportación, reutilizando `safe_path` (viene del Hito 4).
- [ ] Evaluar cifrado y sincronización únicamente con ADR, threat model y pruebas.
