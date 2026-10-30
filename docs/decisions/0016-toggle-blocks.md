# ADR-0016 — Bloque desplegable

**Estado:** aceptada (2026-10-30), con las tres decisiones del propietario.

## Contexto
H12.4 añade un bloque desplegable: una línea de texto con una flecha que muestra u oculta los bloques que tiene dentro. Silex guarda los bloques como una lista plana con `indent` (ADR 0003): no hay hijos de verdad. Las listas ya usan la sangría para anidar, y un bloque puede estar como mucho un nivel más adentro que el de arriba (`editor::change_indent`).

Hay que decidir qué es «dentro», dónde se guarda si está plegado, qué pasa con su contenido al moverlo o borrarlo, y cómo va a Markdown.

## Decisión
- **Tipo nuevo `toggle`**, de texto y con formato en línea (ADR 0015), como un párrafo. La migración 0012 lo añade al `CHECK` de `blocks.kind`, reconstruyendo `blocks` como la 0011.
- **«Dentro» es la sangría:** el contenido de un desplegable son los bloques que lo siguen con más sangría que él, hasta el primero que no la tenga (`editor::subtree_end`). No cambia el modelo plano ni el límite de un nivel: Tab mete un bloque en el desplegable de arriba y Mayús+Tab lo saca, como en una lista. Un desplegable puede estar dentro de otro.
- **Plegado = estado de vista** (decisión del propietario). El conjunto de desplegables plegados se guarda en `settings` (`folded_blocks`, JSON con sus ids), como las pestañas abiertas (ADR 0011). Plegar o desplegar no cambia el documento: no es un paso de Cmd+Z, no crea versión en el historial, no marca la página como modificada y no se copia en plantillas ni duplicados. Uno nuevo empieza abierto. Al arrancar se olvidan los ids que ya no son desplegables vivos.
- **Lo oculto nunca se edita a ciegas.** Si algo necesita un bloque oculto (el foco tras deshacer, el resultado mostrado en «Buscar en la página», Retroceso al inicio del bloque que sigue a uno plegado), primero se despliegan los desplegables que lo contienen.
- **El contenido va siempre con él** (decisión del propietario): mover, arrastrar, duplicar, borrar y sangrar un desplegable lleva su contenido, esté abierto o plegado, como en Notion. Retroceso al inicio lo convierte en párrafo y el contenido se queda donde está, como debajo de cualquier bloque.
  - *Actualización (H12.4b):* «con su contenido» vale para la selección de bloques (que lo resalta y lo copia), las flechas con Alt+Mayús, el menú «…», Cmd+D, borrar (con o sin selección) y arrastrar. Mover un bloque junto a un desplegable lo pasa entero, sin meterse dentro (para eso está Tab). Tab sobre un desplegable no hace nada si su contenido pasaría del nivel máximo de sangría. Al copiar, un desplegable se escribe con «▸ » delante y su contenido sangrado con dos espacios por nivel. Al pegar dentro de Silex vuelve igual: lo copiado en Silex también lee la sangría de los párrafos, que antes se pegaban sin ella. El texto de otras apps conserva sus espacios como texto, como siempre.
- **Markdown: `<details>`** (decisión del propietario). `<details>` + `<summary>texto</summary>`, el contenido como Markdown normal y `</details>`. GitHub y Obsidian lo muestran plegable y se vuelve a importar como desplegable. El título va sin formato, porque dentro de una etiqueta HTML no se lee Markdown, y con `&`, `<` y `>` escapados.
  - *Actualización (H12.4c):* el contenido se escribe en la columna del propio desplegable (`<details>` abre un contenedor nuevo), también si el desplegable está dentro de una lista, y `</details>` va tras una línea en blanco. Al importar, un bloque HTML hecho solo de `<details>` (o `<details open>`), `<summary>…</summary>` y `</details>` (sin distinguir mayúsculas) crea desplegables. Lo que haya hasta el `</details>` va un nivel más adentro. Cualquier otro HTML sigue como texto, como antes. Un `<details>` con texto pegado debajo, sin línea en blanco, es un único bloque HTML para CommonMark y también sigue como texto. El salto de línea de un título pasa a ser un espacio.
- **Teclado:**
  - Intro al final del título de un desplegable abierto crea un bloque dentro, el primero. Si está plegado, lo crea después de todo su contenido.
  - Intro en uno vacío lo convierte en párrafo, como en una lista.
  - Cmd/Ctrl+Intro pliega y despliega, salvo con el cursor en un enlace, que lo abre (ADR 0015).
  - Las flechas se saltan lo oculto.

## Consecuencias
- Una migración que reconstruye `blocks`, con sus tests de actualización (v11→v12, v1→v12, fallo a mitad, copia v11).
- El estado plegado vive en el equipo: otro equipo, una copia restaurada o una exportación lo ven todo abierto.
- Las operaciones de bloques necesitan saber qué es «el bloque con su contenido». Se hace con funciones puras en `src/editor.rs` (`subtree_end`, `hidden_blocks`) y sus tests.
- Las listas no cambian: mover un elemento de lista sigue moviendo solo ese elemento.

### Historias
1. **H12.4a — Modelo, plegado y teclado:** migración 0012, `BlockKind::Toggle`, la flecha, lo oculto no se pinta y las flechas lo saltan, plegado guardado en `settings`, Intro y Cmd+Intro, «Buscar en la página» y el foco despliegan, y «/desplegable».
2. **H12.4b — Con su contenido:** mover, arrastrar, duplicar, borrar y sangrar con el contenido, y copiar y pegar un desplegable con lo que tiene dentro.
3. **H12.4c — Markdown:** exportar e importar `<details>`, con ida y vuelta probada.
