# ADR-0013 — Borrar para siempre desde la papelera

**Estado:** aceptada (2026-10-21) por el propietario, sin cambios sobre la propuesta.

## Contexto
La papelera (H2.3) solo restaura. «No hay borrado definitivo» fue una decisión de producto del 2026-10-18 (`docs/ARCHITECTURE.md`), y varios ADR se apoyan en ella:
- **0007 (adjuntos):** «nada de lo que cuelga de una página se pierde al mandarla a la papelera».
- **0008 (enlaces):** el `ON DELETE SET NULL` de `blocks.link_page_id` es «solo una red de seguridad».
- **0009 (historial):** el `ON DELETE CASCADE` de `page_versions` es «solo una red de seguridad».
- **0006 (bases):** los `ON DELETE CASCADE` de propiedades, valores y vistas, igual.

H11.3 pide «Borrar para siempre» por elemento y «Vaciar la papelera». Además, este ADR tiene que decidir si la papelera se vacía sola pasados 30 días.

Este ADR revoca la decisión del 2026-10-18 **solo para el borrado que pide el usuario desde la papelera**. Fuera de la papelera, nada se borra físicamente.

## Decisión propuesta
1. **Borrado manual con confirmación.** En la papelera habrá «Borrar para siempre» en cada elemento y «Vaciar la papelera». Una ventana de confirmación dice cuántas páginas se borran (con sus subpáginas y filas), que no se puede deshacer y que las copias de seguridad anteriores aún las contienen. No hay Cmd+Z.
2. **Sin borrado automático a los 30 días.** Silex es local-first y no debe hacer desaparecer datos sin una acción del usuario. Una página olvidada en la papelera no estorba: no sale en la búsqueda, los enlaces ni la barra lateral. Lo único que ocupa es espacio, y para eso están «Vaciar la papelera» y «Liberar espacio». Si más adelante se quiere, sería una opción en Ajustes, desactivada por defecto y con su propio ADR.
3. **Qué se borra.** Un elemento de la papelera es una rama: la página y lo que se envió con ella (el mismo `deleted_at`, como al restaurar). Se borra en **una transacción** con `DELETE FROM pages`, y las claves foráneas existentes hacen el resto:
   - `blocks`, `page_versions`, `db_values` de sus filas y, si es una base, `db_properties`, `db_select_options` y `db_views`, todo con `ON DELETE CASCADE`;
   - los índices FTS (`page_search`, `block_search`), con sus disparadores `AFTER DELETE`, que también se ejecutan en las cascadas;
   - los enlaces de otras páginas a ella (`blocks.link_page_id`) pasan a `NULL` y se leen como «Página que ya no existe»;
   - los adjuntos no se borran en ese momento: quedan sin uso y los recoge «Liberar espacio» (ADR 0007, H7.4), que también compacta el archivo.
4. **Elementos sueltos.** Una subpágina o fila enviada a la papelera **antes** que su padre es otro elemento. Si se borra el padre, ese elemento pierde el padre (`parent_id` pasa a `NULL` por la clave existente) y, si se restaura, vuelve a la raíz como hoy. Una fila suelta cuya base se borra pierde sus valores (la cascada de `db_properties`) y vuelve como página normal. La confirmación cuenta solo la rama, porque esas filas siguen en la papelera.
5. **Referencias que quedan colgando:**
   - **Historial de versiones (0009):** una versión de otra página puede enlazar a una página borrada (el JSON guarda el id). Al restaurarla, el enlace se guarda vacío («Página que ya no existe») en lugar de fallar la clave foránea. Es un cambio en el guardado de bloques: un `link_page_id` que ya no existe se escribe como `NULL`.
   - **Deshacer en la tabla y aviso de «Deshacer»:** una operación sobre una fila borrada falla con el aviso de siempre («No se pudo deshacer el cambio»), sin pánico.
   - **Pestañas, favoritos y vistas recordadas:** las pestañas ya se cierran al enviar a la papelera, y los favoritos y las plantillas son columnas de la propia página. Las claves de `settings` con ids (`open_tabs`) ya ignoran páginas que no existen.
6. **Copias de seguridad.** Las copias anteriores conservan lo borrado. Así se puede recuperar con «Restaurar copia», y también hay que decirlo en la confirmación, por privacidad. No se crea una copia previa automática. El borrado no compacta: el espacio se reutiliza y el archivo solo encoge con «Liberar espacio».

## Alternativas descartadas
- **Mantener la papelera sin borrado:** la base crece sin control, y quien quiere quitar algo de verdad (privacidad) no puede.
- **Borrado automático a los 30 días (como Notion):** pierde datos sin una acción del usuario. En una app local no hay un servidor que los guarde «por si acaso».
- **Marcar como «purgada» sin borrar:** no libera espacio ni quita el contenido. No cumple lo que pide el usuario.
- **Borrar también los adjuntos sin uso en la misma operación:** mezcla dos operaciones con reglas distintas (H7.4 respeta el deshacer de la página abierta) y alargaría una transacción en la interfaz.

## Consecuencias
- Hay que actualizar `docs/ARCHITECTURE.md` (papelera), `docs/DATA_MODEL.md` y los ADR 0006–0009 con una nota «Actualización (ADR 0013)»: las cascadas dejan de ser solo una red de seguridad.
- **Tests de integridad tras borrar:**
  - `PRAGMA foreign_key_check` vacío;
  - FTS sincronizado: la búsqueda no devuelve lo borrado y el `integrity-check` de FTS5 pasa;
  - `verify_backup` de una copia hecha después;
  - enlaces entrantes a `NULL`;
  - versiones de otras páginas restaurables;
  - elementos sueltos restaurables a la raíz.
- **Sin migración:** las claves y los disparadores ya existen.

## Decisiones del propietario (2026-10-21)
1. Se revoca «sin borrado definitivo» para la papelera: borrado manual, con confirmación y sin deshacer.
2. Sin borrado automático a los 30 días.
3. Sin copia de seguridad automática antes de vaciar; la confirmación recuerda que las copias anteriores conservan lo borrado.
