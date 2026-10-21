# ADR-0007 — Adjuntos locales

**Estado:** aceptada (2026-10-19), con las decisiones del propietario del final.

## Contexto
El PRD pone en P1 los «adjuntos locales»: poder guardar en una página un PDF, una imagen o cualquier archivo, y abrirlo después, sin red. El roadmap (Hito 7) pide un ADR propio sobre almacenamiento, límites de tamaño, copias de seguridad y exportación, reutilizando `safe_path`.

Hoy todo lo del usuario vive en **un solo archivo SQLite** (v5): páginas, bloques, bases de datos y ajustes. Varias garantías dependen de eso:
- **Copias** (`src/db/backup.rs`): la API de backup de SQLite copia la base entera en un hilo aparte. La copia se verifica (`integrity_check`, versión y FTS5) y solo entonces se renombra. Se guardan 10.
- **Restauración:** antes de sobrescribir, verifica la copia y guarda el estado actual; si falla, no deja nada a medias.
- **Migraciones:** son transaccionales y van precedidas de una copia verificada.
- **Papelera:** sin borrado definitivo (decisión de producto), así que nada de lo que cuelga de una página se pierde al mandarla a la papelera.
- **Deshacer del documento** (ADR 0004): instantáneas de la lista de bloques. Un bloque borrado puede volver con Cmd+Z durante la sesión.

Requisitos:
- **Integridad:** un adjunto nunca queda a medias, ni huérfano de su bloque, ni apuntando a un archivo que no existe. Copias y restauraciones lo incluyen siempre.
- **Seguridad:** el nombre de un archivo viene de fuera, así que nunca se usa como ruta sin `safe_path`. Una imagen maliciosa no puede agotar memoria al decodificarse, y abrir un adjunto no ejecuta nada sin que el usuario lo vea.
- **Rendimiento:** añadir un archivo no bloquea la interfaz, y listar páginas, bloques o resultados de búsqueda no lee el contenido de los adjuntos.
- **Accesibilidad:** añadir y abrir adjuntos no depende solo del ratón.
- **Sin red ni telemetría**, como el resto.

### Medidas (macOS, SQLite 3.50, WAL, `synchronous=FULL`)
- Insertar un blob de 20 MB: 0,12 s; cuarenta de 5 MB: 1,35 s. Es demasiado para el hilo de la interfaz (16 ms por frame), así que las escrituras van a un hilo aparte.
- Leer el nombre y el tamaño de una fila con un blob de 20 MB: < 1 ms. SQLite no lee los blobs si la consulta no los pide.
- Con 220 MB de adjuntos, copiar la base con la API de backup tarda 0,43 s y su `integrity_check` 0,03 s.

## Alternativas
1. **Dentro de SQLite, como BLOB**, en una tabla `attachments`.
   - *A favor:* un solo archivo, así que copias, restauración, migraciones y transacciones siguen funcionando como hoy, sin código nuevo. Un bloque y su archivo se crean o fallan juntos, y no hay rutas que validar ni huérfanos en disco.
   - *En contra:* la base crece con los adjuntos, y cada una de las 10 copias también: con 500 MB de adjuntos, unos 5 GB en copias. SQLite rinde peor que el sistema de archivos con archivos muy grandes, y un blob no puede pasar de 1 GB (`SQLITE_MAX_LENGTH`).
2. **En una carpeta `attachments/` junto a la base**, con nombre por id (nunca el nombre original), y los metadatos en SQLite.
   - *A favor:* la base sigue pequeña, los archivos grandes no cuestan nada, y abrir uno es pasar su ruta.
   - *En contra:* rompe «un solo archivo». Las copias tendrían que ser carpetas o archivos comprimidos, con su verificación, y la restauración debería sustituir base y carpeta de forma atómica, que el sistema de archivos no ofrece. Escribir en dos sitios abre ventanas de inconsistencia (archivo sin fila, fila sin archivo) que hay que limpiar al arrancar. Además, cualquier programa puede tocar esa carpeta.
3. **Híbrido:** en SQLite hasta cierto tamaño y en carpeta a partir de él. Suma la complejidad de las dos.
4. **Solo enlaces a archivos del usuario** (guardar una ruta, sin copiar). Es trivial, pero no es «local-first»: el enlace se rompe al mover el archivo, no entra en las copias ni en la exportación, y deja rutas absolutas del equipo en la base.

## Decisión
**Alternativa 1 (BLOB en SQLite) con un límite por archivo**, porque conserva todas las garantías de integridad existentes sin código nuevo de copias. Si el uso real pide archivos grandes, se reevaluará la alternativa 2 en un ADR nuevo. Pasar de 1 a 2 es una migración de datos acotada (sacar blobs a archivos); al revés también.

### Esquema (migración `0006_attachments.sql`, aditiva)
```sql
CREATE TABLE attachments (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL,              -- nombre original, solo para mostrar y exportar
    mime       TEXT NOT NULL,              -- por firma del contenido, si no por extensión
    size       INTEGER NOT NULL CHECK (size BETWEEN 0 AND 20971520),
    width      INTEGER,                    -- imágenes: dimensiones leídas al añadir
    height     INTEGER,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    data       BLOB NOT NULL CHECK (length(data) = size)   -- la última columna
);
```
- **Un bloque nuevo, `file`.** Su `content` es el nombre que se ve, así que la búsqueda FTS5 encuentra los adjuntos por nombre sin tocar los índices. Su archivo va en `blocks.attachment_id TEXT REFERENCES attachments(id) ON DELETE RESTRICT`, con `CHECK ((kind = 'file') = (attachment_id IS NOT NULL))`.
- **Hay que reconstruir `blocks`**, porque SQLite no deja cambiar el `CHECK` de `kind`. Se hace con el procedimiento de la migración 0003, que ya está probado: copiar los `rowid`, recrear índices y triggers, y `rebuild` de `block_search`. Tests: v5→v6, v1→v6, un fallo a mitad que deja la v5 intacta, y una copia v5 restaurada y actualizada.
- **`ON DELETE RESTRICT`:** un adjunto usado por un bloque no se puede borrar, ni por error.
- **Nunca `SELECT *`** sobre `attachments`: el blob se lee solo para abrir, exportar o dibujar una vista previa, y por id.

### Ciclo de vida
- **Añadir:**
  1. Un hilo `silex-attach` lee el archivo y comprueba el límite antes de leerlo entero.
  2. Detecta el tipo por la firma del contenido y, si es imagen, lee sus dimensiones.
  3. Guarda **solo el adjunto**, en su transacción y con su propia conexión, como la búsqueda.
  4. La interfaz recibe el resultado por un canal y añade el bloque al documento en memoria, como cualquier edición: el autoguardado lo escribe y Cmd+Z lo quita. Mientras tanto, la página muestra «Añadiendo N archivos…».
  5. Si falla, no se escribe nada y se explica por qué.
  - *Actualización (H7.1):* el bloque no se escribe en el hilo, al contrario de lo que decía la propuesta. El autoguardado sincroniza la lista entera de bloques de la página, así que un bloque escrito desde otra conexión se borraría en el siguiente guardado, o pisaría lo que se escribe mientras tanto. Si la app se cierra entre los dos pasos, solo queda un adjunto sin usar (H7.4). Si se cambia de página mientras se guarda, los bloques se añaden al final de la página de origen, en la base.
- **Borrar el bloque:** el adjunto **no** se borra. Así Cmd+Z puede devolver el bloque durante la sesión, y la papelera conserva los adjuntos de sus páginas.
- **Adjuntos sin usar:** los que ningún bloque (vivo o en la papelera) usa. No se borran solos (pregunta 4). «Copias y exportación» muestra cuánto ocupan y ofrece «Liberar espacio» con confirmación, solo con el historial de deshacer vacío o tras guardarlo todo.
- **Abrir:**
  1. Se escribe una copia en una carpeta temporal propia (`cache/abiertos/<id>/<nombre seguro>`), vaciada al arrancar, y se abre con la aplicación del sistema (`open`, `xdg-open`, `explorer`, como `open_in_file_manager`).
  2. Los cambios hechos en esa copia **no** vuelven a Silex, y la interfaz lo dice.
  3. Los ejecutables y scripts (`.app`, `.exe`, `.bat`, `.cmd`, `.sh`, `.command`, `.js`, `.jar`, `.msi`, `.ps1`…) no se abren: se ofrece «Mostrar en la carpeta» (pregunta 5).
- **Guardar una copia:** se escribe en la carpeta de exportaciones (sin diálogo, como la exportación Markdown) y se ofrece abrir la carpeta.

### Encaje con lo que ya existe
- **Copias y restauración:** sin cambios de código. Los adjuntos van dentro de cada copia, y la consecuencia es el espacio (ver Consecuencias). La pantalla de copias mostrará el tamaño de cada una.
- **Papelera y bases de datos:** sin cambios. Una página en la papelera conserva sus bloques y, por tanto, sus adjuntos. Las filas de una base son páginas, así que también pueden tener adjuntos en sus bloques. Un tipo de propiedad «Archivos» queda fuera (ADR 0006).
- **Búsqueda:** por el nombre del adjunto, a través del `content` del bloque. El contenido de los archivos (texto de un PDF) no se indexa.
- **Exportación Markdown:**
  - Los adjuntos de `Título.md` van a la carpeta `Título (adjuntos)/`, con nombres de `safe_path::unique_stem`, reservada junto al `.md` y la carpeta de subpáginas para que no choquen.
  - El enlace es relativo y entre `<…>`, que CommonMark admite con espacios: `[informe.pdf](<Título (adjuntos)/informe.pdf>)`, o `![foto.png](<…>)` si es imagen.
  - La exportación completa hace lo mismo en cada carpeta, y la de una sola página pasa a escribir una carpeta si la página tiene adjuntos.
- **Importación Markdown:** sin cambios. Los enlaces a imágenes siguen siendo texto; importar los archivos enlazados sería otra historia.
- **Deshacer (ADR 0004):** las instantáneas guardan el bloque con su `attachment_id`, no el archivo, así que deshacer y rehacer no copian datos. Añadir es una operación de la base fuera del documento: el bloque aparece por recarga, y Cmd+Z lo quita como cualquier bloque (el adjunto queda sin usar).
- **Copiar y pegar bloques** (formato de Silex, H3.5): un bloque `file` se copia como su nombre, y pegarlo crea un párrafo con ese nombre, no otro adjunto. Pegar adjuntos entre páginas sería otra historia.

### Interfaz
- **Bloque de archivo:** icono por tipo, nombre, tamaño («2,4 MB») y botones «Abrir» y «Guardar una copia». Es accesible como «Archivo «informe.pdf», 2,4 MB». Intro abre y Supr borra el bloque, como los demás.
- **Añadir:**
  - arrastrar archivos a la página: los `.md` se siguen importando como páginas, y los demás se adjuntan en la página abierta, tras el bloque con foco o al final;
  - pegar una imagen del portapapeles: egui solo entrega texto pegado, así que se lee con `arboard` (ya en `Cargo.lock` por eframe; usarlo directamente lo añade a `Cargo.toml` sin crates nuevos), que la da como RGBA, y se guarda como PNG;
  - `/archivo` en el menú `/`, que abre el diálogo del sistema (pregunta 3).
- **Vista previa de imágenes** (pregunta 2): PNG y JPEG dentro del bloque, con un ancho máximo y su proporción, decodificadas en un hilo y guardadas como textura solo mientras se ven.
  - Decodificar con `image::Limits` (máx. 8192 × 8192 y 256 MB de memoria) evita las «bombas» de descompresión; una imagen que no cumple se muestra como archivo.
  - `image` 0.25 ya es dependencia de eframe con PNG; JPEG solo activa su formato `jpeg`, sin crates nuevos en `Cargo.lock`. Se comprobará al implementarlo.
  - *Actualización (H7.2):* comprobado: el `Cargo.lock` solo gana la línea de la dependencia directa. La vista previa se reduce a 1600 px de lado como mucho y se muestra a un punto por píxel, hasta el ancho de la columna y 480 puntos de alto. Su espacio sale de las dimensiones guardadas, así que la página no salta cuando llega. «Solo mientras se ven» se concreta así: las texturas de la página abierta se guardan hasta 96 MB, y por encima se liberan las que no están a la vista. Al cambiar de página se liberan todas. Pegar imágenes no necesita `arboard`: egui 0.37 entrega `Event::PasteImage` cuando el portapapeles tiene una imagen y no texto.

### Seguridad (modelo de amenazas)
- **Nombres:** el nombre original solo se muestra o pasa por `safe_path` (`safe_stem`, `unique_stem`, `join_inside`) al escribir en disco. Tests con `../`, rutas absolutas, `CON.txt`, nombres de 300 bytes, caracteres de control y Unicode.
  - Al guardarlo se quitan también las marcas de dirección (U+202A–U+202E, U+2066–U+2069, U+200E/F), que harían que «factura\u{202E}fdp.exe» se leyera como «facturaexe.pdf».
- **Contenido:** Silex nunca interpreta un adjunto, salvo decodificar PNG/JPEG con límites. El tipo se decide por la firma, no por la extensión, así que una «imagen» que no lo es se queda como archivo.
- **Abrir:** solo con un clic o Intro del usuario sobre ese bloque, nunca al arrancar ni al pasar el ratón, y los ejecutables no se abren (ver Ciclo de vida).
- **Tamaño:** el límite está en la base (`CHECK`) y en el hilo que lee, antes de cargar el archivo en memoria.
- **Privacidad:** los adjuntos no salen del equipo. Los metadatos que puedan llevar dentro (EXIF con ubicación de una foto) se guardan tal cual y no se muestran; quitarlos sería otra historia.

## Consecuencias
- **Espacio de las copias.** Cada copia lleva todos los adjuntos: la base y sus 10 copias ocupan unas 11 veces lo adjuntado. La pantalla de copias mostrará el tamaño, y si hace falta se reducirán las copias guardadas o se harán incrementales (otro ADR).
- **Límite de 20 MB por archivo** (pregunta 1). Cambiarlo es una migración nueva, porque el `CHECK` está en la tabla.
- **Una migración con reconstrucción de `blocks`**, la segunda tras la 0003, con el mismo procedimiento y sus tests.
- **Dos hilos nuevos** (añadir y decodificar imágenes), con su conexión y sus canales, como búsqueda y copias.
- **Una dependencia nueva**, si se acepta el diálogo de archivos: `rfd` (MIT, mantenida, diálogos nativos). En Linux usa el portal de escritorio por D-Bus; se comprobará que no añade bibliotecas de sistema a los paquetes (ADR 0005).
- **Fuera de alcance**, cada uno con su historia o ADR: propiedad «Archivos» en bases de datos, indexar el contenido de los archivos, editar un adjunto y que el cambio vuelva, quitar metadatos EXIF, vídeo o audio en línea, archivos de más de 20 MB, deduplicar archivos iguales y copias incrementales.

### Historias propuestas
1. **H7.1 — Adjuntar y abrir.**
   - Migración 0006 completa con sus tests.
   - Arrastrar archivos a una página, en el hilo `silex-attach`.
   - Bloque de archivo con «Abrir» y «Guardar una copia».
   - Exportación Markdown con la carpeta de adjuntos.
   - Tests de `safe_path` con nombres hostiles y de los límites.
2. **H7.2 — Imágenes:** vista previa de PNG y JPEG con límites, y pegar imágenes del portapapeles.
3. **H7.3 — Diálogo de archivos:** `/archivo` (si se acepta `rfd`).
4. **H7.4 — Espacio:** tamaño de copias y adjuntos, y «Liberar espacio» de adjuntos sin usar.

## Decisiones del propietario (2026-10-19)
1. Los adjuntos se guardan dentro de SQLite (alternativa 1), con un límite de 20 MB por archivo.
2. Vista previa de imágenes PNG y JPEG, en H7.2, sin crates nuevos.
3. Se acepta `rfd` para el diálogo de archivos (H7.3), con la comprobación de los paquetes de Linux.
   - *Comprobado (H7.3):* `rfd` 0.17.2 solo añade `rfd` y `pollster` al `Cargo.lock`. En Linux usa el portal de escritorio por D-Bus con `libdbus` abierta en tiempo de ejecución (`dlopen`): si falta, el diálogo falla con un error, pero la app arranca. Wayland ya va por `dlopen` en eframe, así que los paquetes no necesitan ninguna biblioteca nueva del sistema.
4. Los adjuntos sin usar nunca se borran solos: solo con «Liberar espacio» y confirmación (H7.4).
   - *Actualización (H7.4):* borrar filas no reduce un archivo SQLite, y las copias tampoco, porque copian las páginas libres. Por eso «Liberar espacio» también compacta (`VACUUM`) en un hilo con su propia conexión. Como `VACUUM` puede renumerar los `rowid` implícitos, después se reconstruyen los dos índices de búsqueda. Antes de empezar se guarda una marca en `settings`, y si la app se detiene a mitad, la siguiente apertura termina la reconstrucción. No se borran los adjuntos que el historial de deshacer de la página abierta aún puede devolver.
5. Silex no abre ejecutables ni scripts: ofrece «Mostrar en la carpeta».

*Actualización (ADR 0013, 2026-10-21):* la papelera ya permite borrar para siempre, con confirmación. Las cascadas y el `SET NULL` dejan de ser solo una red de seguridad: se ejecutan al borrar.
