# ADR-0010 — Plantillas de página

**Estado:** aceptada (2026-10-20), con las decisiones del propietario del final.

## Contexto
El PRD pone las plantillas en P2 y el roadmap (Hito 7) las agrupa con el historial y los enlaces, que ya están hechos. Una plantilla es una página que se usa como punto de partida: un acta de reunión, un diario, una lista de tareas. Hoy no se puede ni duplicar una página.

Lo que ya existe y condiciona la decisión:
- **Las páginas son filas de `pages`**, con `kind` ∈ `page`, `database` y un `CHECK`. Cambiar ese `CHECK` exige reconstruir `pages`, con sus índices FTS5 y sus claves foráneas (bloques, favoritos, valores de bases, versiones).
- **Un bloque `file` apunta a su adjunto por id** (ADR 0007). Los adjuntos no cambian nunca, así que varios bloques pueden compartir uno: «Liberar espacio» solo borra los que no nombra nadie.
- **Un bloque `page_link` apunta a una página por id** (ADR 0008) y cuenta para los backlinks.
- **Todo lo del usuario vive en la base** (copias, restauración y migraciones lo cubren).

## Alternativas
1. **Una plantilla es una página marcada** (`pages.template_at`), que no sale en el árbol. Se edita como cualquier página y «Nueva página desde plantilla» copia su título y sus bloques. Reutiliza el editor, los adjuntos, los enlaces, el historial y la papelera.
2. **Un tipo de página nuevo** (`kind = 'template'`). Más explícito, pero exige reconstruir `pages` (la tabla con más dependencias) para cambiar su `CHECK`.
3. **Plantillas fijas en el código** (reunión, diario…). Sin esquema, pero el usuario no puede crear ni cambiar las suyas.
4. **Archivos Markdown en una carpeta.** Salen de la base: las copias no las cubren, y Markdown no guarda adjuntos ni enlaces por id.

## Decisión
**Alternativa 1:** una plantilla es una página con `template_at`.

### Esquema (migración `0009_templates.sql`, aditiva)
```sql
ALTER TABLE pages ADD COLUMN template_at TEXT;
CREATE INDEX pages_template_idx ON pages(template_at) WHERE template_at IS NOT NULL;
```
- Solo añade una columna: no reconstruye nada. Las páginas existentes no son plantillas.
- **Una plantilla es siempre una página de nivel superior** (`parent_id` nulo) y de tipo `page`: una base de datos o una fila no pueden ser plantillas (las plantillas de filas quedan fuera, ADR 0006). Lo comprueba Rust en la transacción, como `move_page` con las bases.
- **Tests:** v8→v9, v1→v9, un fallo a mitad que deja la v8 intacta, y una copia v8 restaurada y actualizada.

### Dónde no aparecen
Cada consulta de páginas filtra `template_at IS NULL` salvo las de plantillas. Hay que revisarlas todas, con un test por cada una:
- el árbol de la barra lateral, los favoritos y la navegación con Alt+↑/↓;
- la búsqueda (Cmd+K) y el buscador de páginas para enlazar;
- los backlinks: un enlace dentro de una plantilla no es un «Enlazada desde»;
- «Mover a…», que no ofrece una plantilla como destino ni mueve una plantilla.

### Comportamiento
- **«Guardar como plantilla»** en el menú «…» de una página (no en bases ni filas) **copia** la página como plantilla nueva, con el mismo título. La página sigue como estaba.
- **«Nueva página desde plantilla…»** (en el «+» de «Páginas», en el menú «…» de una página para crearla como subpágina, y en Cmd+K) abre un buscador con las plantillas (↑ ↓ Intro, Esc), como el de enlazar.
- **Crear desde una plantilla** copia el título y los bloques en una página nueva, en una transacción, y la abre con el título listo para editar:
  - los bloques reciben ids nuevos;
  - un bloque de archivo **comparte el adjunto** de la plantilla (no se duplican datos);
  - un enlace apunta a la misma página.
- **Editar una plantilla** es abrirla y editarla como cualquier página: tiene historial, Cmd+Z y papelera. En su barra superior dice «Plantilla» y ofrece «Usar esta plantilla».
- **Papelera:** una plantilla va a la papelera y vuelve como una página.
  - *Actualización (H7.10):* vuelve siendo plantilla. Las plantillas se ordenan entre plantillas («Subir/Bajar») sin mover las páginas. `{{fecha}}` se escribe «20 oct 2026»; el texto de un enlace (el título de su página) nunca se rellena.

### Encaje con lo que ya existe
- **Adjuntos y «Liberar espacio»:** sin cambios. Un adjunto compartido sigue en uso mientras lo nombre algún bloque o versión.
- **Historial:** las plantillas tienen versiones como las páginas.
- **Exportación del espacio completo:** las plantillas van a una carpeta `Plantillas/` (véase la pregunta 4).
- **Copias y restauración:** sin cambios.

## Consecuencias
- **Una migración aditiva** (una columna y un índice parcial).
- **Riesgo principal:** que alguna consulta de páginas olvide el filtro y una plantilla aparezca en el árbol, la búsqueda o los backlinks. Por eso se revisan todas y cada una tiene su test.
- **Fuera de alcance**, cada uno con su historia o ADR: plantillas con subpáginas, plantillas de bases de datos o de filas, insertar una plantilla dentro de una página existente, y plantillas compartidas entre bases.

### Historias propuestas
1. **H7.10 — Plantillas en la base:** migración 0009 con sus tests; guardar como plantilla y crear desde una; el filtro en todas las consultas, con sus tests.
2. **H7.11 — Plantillas en la interfaz:** sección o lista de plantillas, «Guardar como plantilla», «Nueva página desde plantilla…» con su buscador, la marca «Plantilla» con «Usar esta plantilla», paleta, accesibilidad y capturas.

## Decisiones del propietario (2026-10-20)
1. Una sección «Plantillas», plegada, en la barra lateral, para abrirlas y editarlas. También salen en el buscador de «Nueva página desde plantilla…».
2. Silex trae tres plantillas de ejemplo (Reunión, Diario y Lista de tareas), creadas la primera vez que se abre la lista de plantillas. Se pueden editar y mandar a la papelera, y no vuelven a crearse.
3. `{{fecha}}` en el título o el texto de una plantilla se cambia por la fecha local de hoy al crear la página.
4. Las plantillas no salen en la búsqueda (Cmd+K) ni al enlazar, pero sí en «Exportar todo», en una carpeta `Plantillas/`.
