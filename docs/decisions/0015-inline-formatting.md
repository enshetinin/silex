# ADR-0015 — Formato en línea: negrita, cursiva, código, tachado y enlaces web

**Estado:** aceptada (2026-10-22), con las decisiones del propietario del final.

## Contexto
H12.1 (Hito 12, versión 1.1) añade formato dentro del texto de un bloque: **negrita**, *cursiva*, `código`, ~~tachado~~ y enlaces web. Es lo que más se echa en falta frente a otras apps. El ADR 0003 lo dejó fuera, y el ADR 0008 describe el problema: no se puede mostrar texto distinto del que se edita sin que el cursor, la selección, copiar y el IME recorran caracteres invisibles.

Lo que ya existe y condiciona la decisión:
- **Cada bloque es un `TextEdit` de egui 0.37 con texto plano** (`src/app/blocks.rs`). El `TextEdit` acepta un `layouter`: una función que recibe el texto y devuelve el `Galley` que se pinta. El cursor, la selección y el IME trabajan sobre ese `Galley`, así que **su texto tiene que ser exactamente el del campo**, pero cada tramo puede tener su propio `TextFormat` (grosor, cursiva, fuente, color, subrayado, tachado, fondo). Ya se usa así para tachar una tarea hecha.
- **El editor trabaja con índices de carácter** (`src/editor.rs`: dividir con Intro, fusionar con Retroceso/Supr, buscar y reemplazar), igual que los `CCursor` de egui.
- **El documento se guarda sincronizando la lista entera de bloques** (`save_document`, ADR 0003), y deshacer usa instantáneas de esa lista (ADR 0004). Lo que esté en `Block` entra solo en las dos cosas.
- **La búsqueda FTS5 indexa `blocks.content`** con un trigger `AFTER UPDATE OF content`. El historial guarda los bloques como JSON (`page_versions.blocks`, formato 1, ADR 0009).
- **La importación Markdown deja el formato en línea como texto literal** (`**así**`), y la exportación escapa todo el texto.
- **Cmd/Ctrl+K abre la paleta** (H2.5). El roadmap propone Cmd+K para crear un enlace.

Requisitos:
- **Cursor, selección, IME, deshacer y buscar siguen funcionando** como hoy, también con acentos, emoji y escritura compuesta.
- **El texto de la búsqueda sigue siendo el texto que se lee**, sin símbolos de marcado.
- **Integridad:** migración aditiva y transaccional; un formato dañado nunca rompe la página ni pierde texto.
- **Ida y vuelta con Markdown:** lo exportado se vuelve a importar con el mismo formato.
- **Teclado y accesibilidad:** todo se hace sin ratón.

## Alternativas
1. **Markdown dentro del texto, con las marcas visibles** (como Bear o el modo fuente de Obsidian): `**negrita**` se guarda tal cual y el `layouter` pinta las marcas atenuadas y el interior en negrita. Sin cambios de esquema. Pero los asteriscos se ven siempre y el cursor pasa por ellos; un enlace muestra su URL entera en medio de la frase; la búsqueda y la exportación tendrían que quitar o interpretar el marcado; y el texto que ya existe con `*` o `_` literales (las páginas importadas guardan `**así**` como texto) cambiaría de significado de golpe, salvo que una migración lo escape todo.
2. **Markdown dentro del texto, con las marcas ocultas** salvo donde está el cursor (el «live preview» de Obsidian o Typora). Es lo más bonito, pero con egui exige pintar un texto distinto del que se edita: es justo el problema que el ADR 0008 descartó. Habría que escribir un editor de texto propio.
3. **Texto limpio y marcas aparte, en una columna JSON del bloque** (como ProseMirror, Notion o Google Docs): `content` sigue siendo el texto plano y `marks` guarda tramos `[inicio, fin)` en caracteres, con su tipo (y la URL de un enlace). El `layouter` pinta cada tramo con su formato sobre el mismo texto, así que el cursor, la selección y el IME no cambian. El precio es mantener los tramos al día cuando cambia el texto.
4. **Como la 3, pero en una tabla `block_marks`** con una fila por tramo y clave foránea al bloque. Más «relacional», pero cada guardado sincroniza otra tabla, el historial y las plantillas tienen que copiarla, y nunca hace falta consultar los tramos por SQL.

## Decisión
**Alternativa 3.** Es la única que deja intactos el cursor, el IME, la búsqueda y el texto que ya existe, y encaja con el `layouter` que el editor ya usa. Mantener los tramos al día es código puro y comprobable (`src/marks.rs`), no código de interfaz. La alternativa 4 añade una tabla sin ganar nada que se vaya a consultar.

### Modelo (`src/marks.rs`, funciones puras con tests)
- **Tipos:** `Bold`, `Italic`, `Code`, `Strike` y `Link { url }`.
- **Un tramo** es `{ start, end, kind }` en **índices de carácter** (`char`, como `src/editor.rs` y los `CCursor` de egui), con `start < end <= longitud del texto`.
- **Forma normal**, la única que se guarda: ordenados por `start`; dos tramos del mismo tipo nunca se solapan ni se tocan (se fusionan); dos enlaces nunca se solapan; un enlace solo se fusiona con otro de la misma URL. Los tipos distintos sí se solapan (negrita y cursiva a la vez).
- **`Block.marks: Vec<Mark>`**, vacío en casi todos los bloques. Así entra solo en el guardado y en las instantáneas de deshacer.
- **Dónde hay formato:** párrafos, títulos, listas, tareas y citas. Los bloques de código, de archivo y de enlace a página no tienen: convertir un bloque en código quita sus marcas (un paso de deshacer).

### Esquema (migración `0010_inline_marks.sql`, aditiva)
- `ALTER TABLE blocks ADD COLUMN marks TEXT CHECK (marks IS NULL OR json_valid(marks))`. **Sin reconstruir `blocks`**: una columna nueva, `NULL` en todos los bloques existentes, que significa «sin formato».
- **El trigger de FTS no cambia:** solo reacciona a `UPDATE OF content`, así que cambiar el formato no reindexa nada.
- **Al leer**, Rust valida el JSON: tipos conocidos, rangos dentro del texto, URL con esquema permitido. Si algo no encaja, se descartan **las marcas de ese bloque** (nunca su texto) y se avisa en el registro, como los datos dañados del historial.
- **Al guardar**, se escriben ya normalizadas; un bloque sin marcas guarda `NULL`.
- **Tests:** v9→v10, v1→v10, fallo a mitad que deja la v9 intacta, copia v9 restaurada y actualizada, y JSON dañado que conserva el texto.

### Editar
- **Pintar:** el `layouter` parte el texto en los límites de los tramos y aplica a cada trozo el formato base del bloque (fuente del título, gris y tachado de una tarea hecha) más el suyo: negrita con Inter SemiBold (ya incluida para los títulos; en un título, que ya la usa, con Inter Bold), cursiva con la inclinación de egui (`TextFormat::italics`), código con la fuente monoespaciada y el fondo `code_bg`, tachado con una raya de 1 px, enlaces con el color `accent` y subrayado. El texto del `Galley` es siempre el del campo; un test lo comprueba con todas las combinaciones. Si la cursiva inclinada no convence en las capturas, se añade Inter Italic (misma licencia OFL que Inter).
- **Cambios de texto:** tras cada fotograma en que cambia el texto de un bloque, se compara el texto anterior con el nuevo (prefijo y sufijo comunes) y `marks::adjust` mueve, encoge o borra los tramos. Lo que se escribe **dentro** de un tramo toma su formato. Lo que se escribe **justo al final** de negrita, cursiva o tachado lo continúa (como en Notion o Google Docs); al final de código o de un enlace, no. Así funciona igual con escribir, pegar, borrar, cortar y el texto provisional del IME.
- **Operaciones de bloque** (`src/editor.rs`): dividir con Intro reparte los tramos entre los dos bloques; fusionar con Retroceso o Supr los une desplazando los del segundo; duplicar los copia; buscar y reemplazar da al texto nuevo el formato del primer carácter reemplazado.
- **Deshacer:** cada cambio de formato es un paso propio (ADR 0004); escribir sigue agrupándose como hoy.

### Atajos y menús
- **Cmd/Ctrl+B** negrita, **Cmd/Ctrl+I** cursiva, **Cmd/Ctrl+E** código, **Cmd/Ctrl+Mayús+X** tachado. Con texto seleccionado, ponen el formato si falta en algún carácter de la selección y lo quitan si lo tiene toda. Sin selección, activan o desactivan el formato **para lo que se escriba a continuación** en ese punto, hasta mover el cursor.
- **Cmd/Ctrl+K con texto seleccionado en un bloque** abre un campo pequeño bajo la selección para escribir la URL (Intro aplica, Esc cancela; con la URL vacía, quita el enlace). **Sin selección, Cmd/Ctrl+K sigue abriendo la paleta**, como en Notion.
- **Abrir un enlace:** Cmd/Ctrl+clic, o Cmd/Ctrl+Intro con el cursor dentro. Un clic normal coloca el cursor para editar. Al pasar el ratón se ve la URL y cómo abrirla. Se abre con `Context::open_url`, igual que «Buscar actualizaciones» (ADR 0014): la app no hace ninguna petición de red.
- **URL permitidas:** `http`, `https` y `mailto`. Una URL sin esquema que parece una dirección (`ejemplo.com/x`) recibe `https://`; cualquier otro esquema (`file:`, `javascript:`…) se rechaza al escribirla y se descarta al leer o importar.
- **Menú «…» del bloque y paleta:** «Negrita», «Cursiva», «Código», «Tachado» y «Enlace…», con su atajo al lado, para quien no los conoce.
- **Accesibilidad:** el lector de pantalla sigue recibiendo el texto del bloque. Los enlaces se anuncian además como enlaces del bloque, con su texto. AccessKit no transmite negrita ni cursiva desde egui; queda como limitación conocida.

### Encaje con lo que ya existe
- **Búsqueda FTS5 y buscar en la página:** sin cambios, sobre el texto plano. El resaltado de coincidencias (H11.7) sigue usando el `Galley`, que tiene el mismo texto.
- **Historial de versiones** (ADR 0009): formato 2, con `marks` en cada bloque. Se siguen leyendo las versiones de formato 1 (sin marcas). Restaurar devuelve el formato.
  - *Actualización (H12.1a):* una versión sin ningún bloque con formato se sigue guardando como formato 1, idéntica a las de antes; así, actualizar no crea una versión nueva de cada página solo por el cambio de formato.
- **Plantillas, duplicar página o bloques, mover a otra página:** copian las marcas con el bloque.
- **Exportación Markdown:** `**negrita**`, `*cursiva*`, `` `código` ``, `~~tachado~~` y `[texto](url)`, cerrando y abriendo las marcas en el orden correcto cuando se solapan, y escapando el texto como hoy. Los espacios al borde de un tramo se dejan fuera de las marcas, porque CommonMark no reconoce `** así**`.
- **Importación Markdown:** el énfasis, el código en línea, el tachado y los enlaces web pasan a ser marcas en lugar de texto literal; las imágenes y el HTML siguen como texto. Las páginas importadas antes **no se reescriben**: su `**texto**` literal sigue siendo texto (se puede volver a importar).
- **Copiar y pegar:** hacia otras apps se copia el texto plano, como hoy (egui solo pone texto en el portapapeles). Dentro de Silex, la app recuerda el último fragmento copiado con sus marcas: si se pega exactamente ese texto, recupera el formato. Pegar Markdown y «Copiar como Markdown» son H12.7.
- **Bases de datos:** las celdas y los títulos de página y de fila no tienen formato; las filas sí lo tienen en sus bloques.

## Consecuencias
- **Una migración sin reconstrucción**, la primera de `blocks` que solo añade una columna.
- **Cada pulsación compara dos textos y ajusta los tramos:** lineal en la longitud del bloque, del orden de microsegundos. Se medirá en el test de escala con 1000 bloques con formato, junto al clonado de instantáneas (ADR 0004).
- **Los tramos son el punto delicado:** un error en `adjust`, dividir o fusionar desplazaría el formato. Por eso viven en funciones puras con tests de propiedades (texto y ediciones al azar: las marcas siempre normalizadas y dentro del texto) además de los casos concretos con Unicode.
- **Una fuente nueva como mucho** (Inter Bold, y quizá Inter Italic): unos cientos de KB más en el binario.
- **Fuera de alcance**, cada uno con su historia: colores de texto y de fondo, subrayado como formato, menciones de páginas en línea (los enlaces a páginas siguen siendo bloques, ADR 0008), `#etiquetas` (H15.3), una barra flotante de formato al seleccionar con el ratón (se puede añadir después sin cambiar el modelo), y HTML en el portapapeles.

### Historias propuestas
1. **H12.1a — Modelo y migración:** `src/marks.rs` (normalizar, ajustar, dividir, fusionar, poner y quitar, JSON), migración 0010 con sus tests, `Block.marks` en leer y guardar, historial formato 2, plantillas y duplicados. Sin cambios visibles.
2. **H12.1b — Pintar y editar:** fuentes, `layouter` con los tramos, ajuste al escribir (también con IME), Intro, Retroceso, Supr, convertir en código, buscar y reemplazar; capturas en claro y oscuro.
3. **H12.1c — Atajos y menús:** Cmd+B, Cmd+I, Cmd+E, Cmd+Mayús+X, formato pendiente sin selección, entradas en el menú «…» y en la paleta.
4. **H12.1d — Enlaces web:** Cmd+K con selección, campo de URL, abrir con Cmd+clic y Cmd+Intro, esquemas permitidos, anuncio al lector de pantalla.
5. **H12.1e — Markdown y portapapeles:** exportar e importar el formato, ida y vuelta probada, y pegar dentro de Silex conservando el formato.

## Decisiones del propietario (2026-10-22)
1. Texto plano con las marcas en una columna JSON aparte (alternativa 3), no Markdown visible en el texto.
2. Cmd/Ctrl+K crea un enlace cuando hay texto seleccionado en un bloque, y abre la paleta en los demás casos.
3. Escribir justo al final de una negrita, cursiva o tachado lo continúa; al final de código o de un enlace, no.
4. Los enlaces se abren con Cmd/Ctrl+clic (o Cmd/Ctrl+Intro), y un clic normal coloca el cursor para editar.
5. Las páginas importadas antes no se reescriben: su `**texto**` literal sigue siendo texto.
