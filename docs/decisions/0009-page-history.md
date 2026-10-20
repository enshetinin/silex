# ADR-0009 — Historial de versiones de página

**Estado:** aceptada (2026-10-20), con las decisiones del propietario del final.

## Contexto
El PRD pone en P1 un «historial básico», y el roadmap (Hito 7) lo deja para un ADR propio. Hoy se puede volver atrás de tres formas, y ninguna basta:
- **Deshacer** (ADR 0004): 100 pasos en memoria, solo de la página abierta. Se pierde al cambiar de página o cerrar la app.
- **Copias de seguridad** (H1.2–H1.3): de toda la base, una al día y 10 como mucho. Restaurar una devuelve **todas** las páginas a ese momento, también las que estaban bien.
- **Papelera:** devuelve páginas borradas, no contenido cambiado.

Falta poder decir «esta página, como estaba ayer por la tarde» sin tocar el resto.

Lo que ya existe y condiciona la decisión:
- **El autoguardado escribe la página entera** (`save_document`) unos 650 ms después de cada pausa al escribir. Guardar una versión en cada autoguardado serían cientos por sesión.
- **Las páginas nunca se borran físicamente**, y los bloques `file` y `page_link` guardan referencias (`attachment_id`, `link_page_id`) con clave foránea.
- **«Liberar espacio»** (H7.4) borra los adjuntos que no usa ningún bloque, salvo los que deshacer aún puede devolver.
- **Una fila de una base** es una página con bloques más sus valores (`db_values`), que se guardan aparte y tienen su propio deshacer (H6.4).

### Medidas (macOS, release, `synchronous=FULL`)
Bloques de unas 12 palabras, serializados como JSON (id, tipo, texto, casilla, sangría):
- 100 bloques: 17,8 KB; codificar 0,08 ms, insertar 0,11 ms.
- 1000 bloques: 178 KB; codificar 0,4 ms, insertar 0,3 ms.

Guardar una versión cabe en el mismo hilo que el autoguardado, sin worker.

## Alternativas
1. **Instantáneas completas en SQLite**, una fila por versión con el título y los bloques en JSON. Simple: cada versión se lee sola, sin reconstruir nada. Ocupa más que las diferencias, pero con la poda (abajo) y los tamaños medidos es poco.
2. **Diferencias** entre versiones (por bloque). Ocupa menos, pero leer una versión exige aplicar la cadena entera, y una diferencia dañada estropea las siguientes. Más código y más riesgo para el mismo resultado.
3. **Registro de operaciones** (`document_operations`, como dejó abierto el ADR 0004). Permitiría deshacer entre sesiones, pero cada operación tendría que poder reproducirse. Es otro proyecto.
4. **Archivos aparte** (una carpeta por página). Saca datos de la base: las copias, la restauración y las migraciones dejarían de cubrirlos (el mismo motivo que en el ADR 0007).

## Decisión
**Alternativa 1:** instantáneas completas, en la misma base.

### Esquema (migración `0008_page_versions.sql`, aditiva)
Solo crea una tabla y un índice: no reconstruye nada.

```sql
CREATE TABLE page_versions (
    id         INTEGER PRIMARY KEY,
    page_id    TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    title      TEXT NOT NULL,
    format     INTEGER NOT NULL DEFAULT 1,
    blocks     TEXT NOT NULL CHECK (json_valid(blocks)),
    size       INTEGER NOT NULL CHECK (size = length(blocks))
);
CREATE INDEX page_versions_page_idx ON page_versions(page_id, created_at);
```

- **`blocks`** es una lista JSON con, por bloque, `id`, `kind`, `content`, `checked`, `indent` y, si los tiene, `attachment` y `link`. `format` permite cambiarlo en el futuro sin otra migración.
- **Leer es tolerante:** un tipo de bloque desconocido (de un Silex más nuevo) o un JSON que no se entiende hace que esa versión se muestre como «no se puede leer», nunca un pánico ni un error que oculte las demás.
- **`ON DELETE CASCADE`:** las páginas no se borran físicamente, así que solo es una red de seguridad.
- **Sin búsqueda:** las versiones no entran en FTS5. Buscar encuentra lo que hay ahora.
- **Tests:** v7→v8, v1→v8, un fallo a mitad que deja la v7 intacta, y una copia v7 restaurada y actualizada.

### Cuándo se guarda una versión
Una sola regla: **al guardar una página, si su última versión tiene más de 10 minutos (o no tiene ninguna), primero se guarda como estaba** (el estado de la base antes de este guardado), en la misma transacción que el guardado.
- Garantiza que siempre se puede volver a como estaba la página **antes** de empezar a editarla, aunque luego se edite durante horas.
- Escribiendo sin parar sale una versión cada 10 minutos; una página que no se toca no genera ninguna.
- No se guarda una versión igual a la anterior.
- Antes de restaurar una versión se guarda el estado actual, sin esperar los 10 minutos: restaurar nunca pierde nada.

Solo se versiona el **título y los bloques**. Mover, renombrar desde la barra lateral, favoritos y papelera no crean versiones (tienen sus propias formas de volver atrás). Las propiedades de las filas y la tabla de una base de datos quedan fuera (véase la pregunta 4).

### Poda
Al guardar una versión, en la misma transacción, se aclaran las de esa página:
- últimas 24 horas: todas (como mucho una cada 10 minutos);
- hasta 30 días: la última de cada día;
- más antiguas: la última de cada semana.

Así una página editada todos los días acumula unas 30 versiones al mes y 52 al año. La poda es una consulta indexada por página (`page_versions_page_idx`).

### Encaje con lo que ya existe
- **Adjuntos:** una versión puede nombrar un adjunto que el documento ya no usa. «Liberar espacio» lo considera en uso mientras alguna versión lo nombre (con `json_each`), así que restaurar nunca encuentra un archivo borrado. Si aun así faltara (una base editada a mano), el bloque vuelve como párrafo con su nombre: «[archivo que ya no está] informe.pdf».
- **Enlaces:** el destino es una página, que nunca se borra físicamente. Si faltara, el enlace vuelve como «Página que ya no existe», como hoy.
- **Copias de seguridad y restauración:** sin cambios. Las versiones viven en la base y entran en las copias, que crecen con ellas. La sección «Espacio» (H7.4) muestra lo que ocupan.
- **Papelera:** las versiones se quedan con su página, y vuelven con ella.
- **Exportación e importación Markdown:** sin cambios. No se exporta el historial.
- **Deshacer:** restaurar una versión es un cambio más del documento en memoria. Cmd+Z lo deshace, y se guarda por el autoguardado normal.

### Interfaz
- **«Historial de versiones…»** en el menú «…» de la página y en la paleta (Cmd+K → «historial»).
- Una ventana con la lista a la izquierda (fecha y hora, «hace 2 horas»; la más reciente arriba) y, a la derecha, la versión elegida **solo para leer**, con el mismo aspecto que la página (tipos de bloque, casillas, tarjetas de archivo y enlace, sin poder editarlos).
- **«Restaurar esta versión»**: sustituye el título y los bloques de la página abierta, cierra la ventana y lo dice («Se restauró la versión del 18 oct, 17:40. Cmd+Z para deshacer.»).
- Teclado: ↑ ↓ eligen versión, Intro restaura (con confirmación), Esc cierra. Lista y botones con nombre para el lector de pantalla.
- Sin versiones, la ventana lo explica: «Todavía no hay versiones. Silex guarda una al editar, como mucho cada 10 minutos.»
- **Fechas en hora local.** Hoy la app muestra UTC en las copias. Para el historial, la hora local es la que el usuario reconoce; mientras no haya una forma fiable de saber la zona horaria, véase la pregunta 5.

## Consecuencias
- **Una migración aditiva**, sin reconstruir tablas.
- **La base crece con el historial.** Con la poda, una página de 100 bloques editada a diario durante un año ocupa unos 2 MB de versiones (unas 110 de 18 KB). Las copias de seguridad crecen igual.
- **Un guardado puede escribir una versión más** (< 1 ms medido con 1000 bloques).
- **No hay comparación** entre versiones (resaltar qué cambió). Se puede añadir después sin cambiar el esquema.
- **Fuera de alcance**, cada uno con su historia o ADR: historial de las bases de datos (propiedades, valores, vistas), historial del espacio entero, comparar versiones, nombrar o fijar versiones, y deshacer persistente entre sesiones.

### Historias propuestas
1. **H7.7 — Guardar versiones:** migración 0008 con sus tests; versión al guardar con la regla de 10 minutos y la poda; «Liberar espacio» respeta los adjuntos de las versiones; prueba de escala (una página editada un año simulado).
2. **H7.8 — Ver y restaurar:** ventana «Historial de versiones» con lista, vista de solo lectura y «Restaurar esta versión» (con Cmd+Z); menú «…» y paleta; accesibilidad y capturas.

## Decisiones del propietario (2026-10-20)
1. Como mucho una versión cada 10 minutos de edición, con el estado de antes del cambio.
2. Poda: todas las de 24 horas, una al día hasta 30 días y una a la semana después, sin límite de antigüedad.
3. «Liberar espacio» conserva los adjuntos que nombra alguna versión: restaurar nunca pierde un archivo.
4. Las fechas de las versiones se muestran en hora local. Si leer la zona horaria necesita un crate, se justifica en H7.8.
5. Filas de bases de datos: no se preguntó por separado; se aplica la propuesta (solo título y bloques, como las páginas). El historial de propiedades queda fuera de alcance.
