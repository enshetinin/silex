# ADR-0011 — Pestañas y vista dividida

**Estado:** aceptada (2026-10-20), con las decisiones del propietario del final.

## Contexto
El PRD pone en P1 la «vista dividida, pestañas». Los temas claro y oscuro ya están hechos (Hito 5); lo demás no figura en el roadmap. Hoy Silex muestra **una página a la vez**: abrir otra sustituye a la actual, y su deshacer (ADR 0004) se pierde.

Lo que ya existe y condiciona la decisión:
- **Todo el estado de «la página abierta» vive suelto en `SilexApp`**: `selected`, `title`, `blocks`, el deshacer (`history`, `shadow`), el autoguardado (`save`), el cursor y la selección, el menú `/`, las teclas pendientes, la tabla de una base (`database`), las propiedades de una fila (`row_page`), los adjuntos y sus vistas previas (`files`), los enlaces (`links`)… Son unas 350 referencias en `src/app/`.
- **Los ids de los campos** (`field_id`) dependen del bloque y de `doc_generation`, no de dónde se dibujan: la misma página dibujada dos veces daría ids repetidos, y egui mezclaría su foco y su cursor.
- **El teclado del editor** (Intro, Retroceso, Tab, flechas, `[[`, `/`, Cmd+Z) se resuelve sobre la página abierta: `pending_key`, `caret` y `pending_focus` suponen que solo hay una.
- **Ya hay un dibujo de solo lectura** de una página: la vista previa del historial (`version_preview`, H7.8), con todos los tipos de bloque, casillas, archivos y enlaces.

## Alternativas
1. **Pestañas como lista de páginas abiertas.** Una barra de pestañas encima de la página. Cambiar de pestaña guarda la actual y abre la otra con el mismo código de siempre (`open_page`). Para no perder el deshacer, cada pestaña guarda el suyo (y el desplazamiento) mientras no se ve. Una sola página editable a la vez: no cambia el editor.
2. **Vista dividida de consulta.** Un segundo panel, a la derecha, muestra otra página **solo para leer** (con el dibujo del historial), con sus enlaces y adjuntos abribles. «Editar aquí» la cambia al panel principal. Sirve para escribir mirando otra página, sin tocar el editor.
3. **Vista dividida editable.** Sacar el estado de la página abierta a una estructura `Document` (una por panel) y dibujar dos editores a la vez. Es lo que hacen Notion u Obsidian, pero toca casi todo `src/app/` (las ~350 referencias), los ids de los campos, el teclado y el autoguardado. Además hay que decidir qué pasa con la misma página en los dos paneles (dos copias que se pisan al guardar).
4. **Varias ventanas.** egui permite ventanas nativas extra (viewports), pero cada una necesitaría su propio editor (el mismo problema que la 3), y en Linux con Wayland su posición no es controlable.

## Decisión
**Por etapas: 1 y 2 ahora, 3 solo si hace falta después**, con su propia historia y tras ver cómo se usan las dos primeras.
- Las pestañas (1) dan casi todo el valor de «varias páginas abiertas» sin riesgo para el editor.
- La vista de consulta (2) cubre el uso más común de la vista dividida (escribir mirando otra cosa), reutilizando el dibujo del historial.
- Si después se quiere editar en los dos paneles, el trabajo de la 3 se hace sobre esta base: la barra de pestañas y el segundo panel ya existirán, y solo cambiará lo que hay dentro.

### Pestañas
- **Una barra de pestañas** encima de la barra superior de la página: icono, título actual (cortado si no cabe) y «×» al pasar el ratón; la activa resaltada. Si solo hay una pestaña, la barra no aparece (como hoy).
- **Abrir:**
  - un clic en la barra lateral, un enlace o un resultado de búsqueda abre la página **en la pestaña actual**, como hoy;
  - Cmd/Ctrl+clic, o «Abrir en una pestaña nueva» en el menú de una página, la abre en una pestaña nueva;
  - Cmd/Ctrl+T abre una pestaña nueva con la búsqueda (Cmd+K) para elegir página.
- **Cerrar:** «×», clic central o Cmd/Ctrl+W. Cerrar la última no deja la ventana vacía: queda esa página.
- **Cambiar:** clic; Ctrl+Tab y Ctrl+Mayús+Tab; Cmd/Ctrl+1…9.
- **Una página está en una sola pestaña:** abrir en pestaña nueva una página que ya tiene una la activa.
- **Deshacer por pestaña:** al salir de una pestaña se guarda su página (como hoy al cambiar de página) y su deshacer se aparta con ella; al volver, Cmd+Z sigue funcionando. Se pierde al cerrar la pestaña o la app (como hoy).
- **Límite:** 12 pestañas. Abrir otra cierra la usada hace más tiempo, nunca la activa.
- **Papelera:** una página que va a la papelera cierra su pestaña; restaurar una página no la reabre.
  - *Actualización (H8.1):* restaurar desde la papelera la abre en la pestaña actual, como cualquier clic. Clic central en una página de la barra lateral también la abre en una pestaña nueva.
- **Plantillas y bases de datos:** se abren en pestañas como cualquier página.
- **Accesibilidad:** cada pestaña es un botón con el título y su estado (seleccionada), y «Cerrar «Título»» para su «×».

### Vista dividida de consulta
- **«Abrir al lado»** en el menú de una página (y Alt+clic en la barra lateral o un enlace) la muestra en un panel a la derecha, **solo para leer**: título, bloques (como en la vista del historial), los adjuntos con «Abrir» y los enlaces, que un clic abre en ese mismo panel.
- **Una base de datos** se muestra como su tabla, también de solo lectura (sin editar celdas).
- **«Editar aquí»** pasa esa página a la pestaña activa (el panel se cierra). **«×»** cierra el panel. El divisor entre los dos se puede arrastrar.
- **Se mantiene al día:** el panel relee su página al cambiar de página o de pestaña, y tras cada guardado. Si la página va a la papelera, el panel lo dice.
- **Ancho:** el panel solo aparece si la ventana es lo bastante ancha (al menos 900 puntos para la página principal más 320 para el panel); si no, lo dice y propone abrirla en una pestaña.

### Encaje con lo que ya existe
- **Sin cambios en el esquema** ni en `src/db.rs`, salvo guardar las pestañas abiertas si se decide así (pregunta 2), como un ajuste más en `settings`.
- **El editor no cambia:** sigue habiendo una página editable a la vez.
- **Rendimiento:** apartar el deshacer de una pestaña es mover memoria, no copiarla. El panel de consulta relee una página por cambio (una consulta, < 1 ms con 1000 bloques).

## Consecuencias
- **No se puede editar en los dos paneles a la vez**, y así lo dice el panel. Si se pide, es la alternativa 3, con su ADR o historia, sobre esta base.
- **Más estado en memoria:** hasta 12 deshaceres (en el peor caso, decenas de MB con páginas de 1000 bloques; ADR 0004).
- **Fuera de alcance:** varias ventanas, arrastrar pestañas para ordenarlas o para dividir, y dividir en más de dos paneles.

### Historias propuestas
1. **H8.1 — Pestañas:** barra, abrir/cerrar/cambiar con ratón y teclado, deshacer por pestaña, límite, papelera, accesibilidad y capturas (más guardarlas al cerrar, si se decide).
2. **H8.2 — Vista de consulta:** «Abrir al lado», panel de solo lectura (páginas y tablas), «Editar aquí», divisor, ancho mínimo, accesibilidad y capturas.

## Decisiones del propietario (2026-10-20)
1. Vista dividida de consulta (solo leer) ahora; editable en los dos paneles solo si hace falta después, con su propia historia.
2. Las pestañas abiertas se recuerdan al cerrar la app y vuelven al abrirla (sin su deshacer).
3. Un clic normal abre en la pestaña actual; Cmd/Ctrl+clic o «Abrir en una pestaña nueva», en una nueva.
4. Como mucho 12 pestañas: abrir otra cierra la usada hace más tiempo, nunca la activa.
