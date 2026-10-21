# ADR-0008 — Enlaces entre páginas y backlinks

**Estado:** aceptada (2026-10-19), con las decisiones del propietario del final.

## Contexto
El PRD pone en P1 los «enlaces internos, backlinks»: desde una página se enlaza a otra, y cada página muestra qué páginas la enlazan. El roadmap lo agrupa en el Hito 7 con el historial y las plantillas, que irán en ADRs propios.

Lo que ya existe y condiciona la decisión:
- **El editor no tiene formato en línea** (ADR 0003). Cada bloque es un `TextEdit` de egui con texto plano: no hay negritas ni enlaces dentro del texto, y la exportación Markdown escapa todo.
- **Las páginas tienen id estable (UUID)** y nunca se borran físicamente: van a la papelera y vuelven. Los títulos cambian y se repiten.
- **Los bloques se guardan sincronizando la lista entera** de la página (`save_document`), y el deshacer usa instantáneas de esa lista (ADR 0004).
- **El bloque `file` (ADR 0007)** es el precedente de un bloque que no es texto: una tarjeta enfocable que guarda una referencia (`attachment_id`) con clave foránea.

Requisitos:
- **Un enlace no se rompe** al renombrar o mover la página destino, y dice claramente si está en la papelera.
- **Backlinks exactos y baratos:** una consulta indexada, no buscar títulos en el texto.
- **Integridad:** migración aditiva y transaccional, con clave foránea al destino.
- **Teclado y accesibilidad:** crear y seguir un enlace sin ratón, con nombres para el lector de pantalla.

## Alternativas
1. **Enlace como bloque propio** (`kind = 'page_link'`, como el «Link to page» de Notion): una tarjeta con el icono y el título **actual** de la página destino, guardada como `link_page_id`. No se rompe al renombrar, y los backlinks son una consulta sobre `blocks`. No permite un enlace en medio de una frase.
2. **Enlace en línea con el título**, `[[Título]]` dentro del texto, resuelto por título. Se escribe como en Obsidian, pero se rompe al renombrar y es ambiguo con títulos repetidos. Arreglarlo exigiría reescribir el texto de todas las páginas que enlazan.
3. **Enlace en línea con el id**, `[[Título|uuid]]` dentro del texto. No se rompe, pero el `TextEdit` mostraría el UUID al editar. Esconderlo exige pintar texto distinto del que se edita: el cursor, la selección, copiar y el IME recorrerían caracteres invisibles. Es el problema del formato en línea, que el ADR 0003 dejó fuera a propósito.
4. **Enlaces en línea con rangos aparte** (tabla `block_links` con posiciones de inicio y fin). El texto queda limpio, pero cada pulsación desplaza los rangos y deshacer, pegar o dividir un bloque los tendría que mantener. Es un editor de texto enriquecido a medias.

## Decisión
**Alternativa 1:** el enlace es un bloque. Es la única que cumple los requisitos con el editor actual, reutiliza el patrón del bloque `file` y deja los enlaces en línea para cuando exista formato en línea (con su propio ADR).

### Esquema (migración `0007_page_links.sql`, aditiva)
- **`blocks` se reconstruye** como en la 0003 y la 0006 (conservando los `rowid` y con `rebuild` de `block_search`):
  - el tipo `page_link` en el `CHECK` de `kind`;
  - `link_page_id TEXT REFERENCES pages(id) ON DELETE SET NULL`, con `CHECK (kind = 'page_link' OR link_page_id IS NULL)`. Las páginas nunca se borran físicamente: el `SET NULL` solo es una red de seguridad, y entonces el bloque se lee como «Página que ya no existe»;
    - *Actualización (H7.5):* la propuesta decía `CHECK ((kind = 'page_link') = (link_page_id IS NOT NULL))`, pero ese `CHECK` haría fallar el `SET NULL` y, con él, el borrado de la página. El `CHECK` solo impide que un bloque que no es un enlace apunte a una página;
  - índice parcial `blocks_link_idx ON blocks(link_page_id) WHERE link_page_id IS NOT NULL`, que hace barata la consulta de backlinks.
- **El `content`** de un bloque de enlace es el título del destino cuando se creó o renombró. Sirve a la búsqueda y a la exportación; lo que se muestra es siempre el título actual, leído de `pages`.
- **Al renombrar una página** se actualiza el `content` de los bloques que la enlazan, en la misma transacción que el renombrado. Así la búsqueda por el título nuevo encuentra también las páginas que la enlazan.
  - *Actualización (H7.5):* también al cambiar el título desde la propia página (`save_document`). Además, al guardar un enlace su `content` se toma del título actual del destino, así que un documento abierto con un título viejo no lo restablece.
- **Tests:** v6→v7, v1→v7, un fallo a mitad que deja la v6 intacta, y una copia v6 restaurada y actualizada.

### Comportamiento
- **Crear un enlace:**
  - escribir `[[` en un bloque de texto, o `/enlace` (también «página», «link») en el menú `/`, abre un buscador de páginas bajo el bloque (↑ ↓ Intro, Esc), que reutiliza la búsqueda de títulos;
    - *Actualización (H7.5):* el buscador es una ventana con su propio campo, la misma para los tres caminos. Filtra los títulos sin acentos ni mayúsculas: primero los que empiezan por lo escrito, luego los que lo contienen.
  - «Enlazar a una página…» en la paleta (Cmd+K);
  - el bloque va tras el bloque en edición. Si `[[` o `/enlace` dejaron el bloque vacío, el enlace ocupa su lugar (como `/archivo`, H7.3).
- **El bloque de enlace** es una tarjeta enfocable con el icono de la página (o de la base de datos), su título actual y su ruta («Proyecto › Diseño») en gris. Un clic o Intro abre la página destino, y Retroceso/Supr la borran (Cmd+Z la devuelve), como el bloque `file`.
- **Destino en la papelera:** la tarjeta lo dice («en la papelera») y abrirla ofrece restaurarla. El enlace vuelve a funcionar al restaurarla, porque nunca se rompió.
- **Backlinks:** al final de cada página, una sección «Enlazada desde» lista las páginas vivas que la enlazan (título y ruta, en orden alfabético), sin las que están en la papelera. Un clic las abre. Si no hay ninguna, la sección no aparece. Se lee al abrir la página (una consulta indexada).
  - *Actualización (H7.6):* también se relee al mandar a la papelera, restaurar, mover o renombrar páginas, así que la lista no se queda vieja. Una página que se enlaza a sí misma no aparece. Intro sobre una fila enfocada también la abre. Una base de datos muestra su tabla en lugar de bloques y no tiene la sección: sus filas sí.
- **Bases de datos:** se puede enlazar a una base o a una fila (son páginas), y una fila muestra sus backlinks bajo sus bloques.
- **Deshacer:** el bloque entra y sale del documento como cualquier otro (ADR 0004).

### Encaje con lo que ya existe
- **Búsqueda:** sin cambios en FTS5. El `content` del enlace (el título del destino) hace que una página se encuentre también por los títulos que enlaza.
- **Exportación Markdown:**
  - **Espacio completo:** `[Título](<ruta relativa/Título.md>)` hacia el archivo del destino. Las rutas se conocen porque se exporta el árbol entero.
  - **Página suelta:** el título como texto, porque el destino no se exporta.
  - **Destino en la papelera o inexistente:** su título como texto.
- **Importación Markdown:** sin cambios. Los `[[…]]` y los enlaces siguen siendo texto (resolverlos sería otra historia).
- **Papelera y mover páginas:** sin cambios. Los enlaces apuntan al id, no a la posición en el árbol.
- **Copias y restauración:** sin cambios (todo vive en la base).
- **Copiar y pegar bloques** (formato de Silex): un enlace se copia como su título, y pegarlo crea un párrafo con ese título.

## Consecuencias
- **Una migración con reconstrucción de `blocks`**, la tercera, con el procedimiento de la 0003 y la 0006.
- **No hay enlaces en medio de una frase.** Son bloques. Si se quieren enlaces en línea, hará falta un ADR de formato en línea (afecta al editor, a la copia, a deshacer y a la exportación).
- **El título guardado en el enlace se puede quedar viejo** si otra conexión renombra la página, pero solo afecta a la búsqueda y a la exportación: la tarjeta siempre lee el título actual. La app solo renombra por `rename_page`, que lo actualiza.
- **Fuera de alcance**, cada uno con su historia o ADR: enlaces en línea, enlaces a un bloque concreto (anclas), grafo de enlaces, resolver `[[…]]` al importar Markdown, y propiedades de tipo «Relación» en bases de datos (ADR 0006).

### Historias propuestas
1. **H7.5 — Enlaces:** migración 0007 con sus tests; crear enlaces (`[[`, `/enlace`, paleta) con el buscador de páginas; tarjeta de enlace (abrir, papelera, accesible); título al día al renombrar; exportación Markdown.
2. **H7.6 — Backlinks:** sección «Enlazada desde» en páginas y filas; prueba de rendimiento con 1000 páginas y 10 000 enlaces.

## Decisiones del propietario (2026-10-19)
1. El enlace es un bloque (alternativa 1), aunque no pueda ir en medio de una frase.
2. Se crean con `[[`, `/enlace` y la paleta, con un buscador de páginas por título.
3. Un enlace a una página en la papelera lo dice, y abrirlo ofrece restaurarla.
4. Los backlinks de páginas en la papelera no se muestran.
5. Al exportar una página suelta, el destino se escribe como texto, sin enlace.

*Actualización (ADR 0013, 2026-10-21):* la papelera ya permite borrar para siempre, con confirmación. Las cascadas y el `SET NULL` dejan de ser solo una red de seguridad: se ejecutan al borrar.
