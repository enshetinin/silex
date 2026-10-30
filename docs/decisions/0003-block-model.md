# ADR-0003 — Modelo de bloques: tipos ampliados y lista plana con sangría

**Estado:** aceptada (2026-10-18).

## Contexto
El Hito 3 añade títulos de tres niveles, listas con viñetas y numeradas, citas y código, y anidamiento con Tab/Mayús+Tab. El esquema v2 solo admite `paragraph`, `heading` y `todo` (restricción `CHECK`) y no guarda ningún anidamiento. Además, `save_document` solo actualizaba el texto de bloques existentes; el editor de bloques necesita insertar, borrar y reordenar.

## Alternativas
1. **Lista plana con sangría** (`indent` 0–6 por bloque, orden por `position`). Cada operación de teclado afecta a un bloque o a una vecina; exportar a Markdown es directo; no permite plegar un bloque con sus hijos.
2. **Árbol de bloques** (`parent_block_id`), como Notion. Permite plegar ramas y mover un bloque con sus descendientes, pero cada operación de teclado, el guardado, la numeración y la migración se complican; necesita prevención de ciclos también en bloques.

## Decisión
Opción 1, elegida por el propietario.
- Tipos: `paragraph`, `heading_1`, `heading_2`, `heading_3`, `todo`, `bulleted`, `numbered`, `quote`, `code`, con `BlockKind` tipado en `src/model.rs`. Un tipo desconocido en la base es un error explícito, nunca un pánico.
  - *Actualización (H12.2, H12.3):* la migración 0011 añade `divider` (una línea entre bloques, sin texto: una tarjeta como la de un archivo) y `callout` (un aviso: texto con fondo e icono, que se puede elegir en «Convertir en»). Ver `docs/DATA_MODEL.md`, versión 11.
- `indent INTEGER NOT NULL DEFAULT 0 CHECK (indent BETWEEN 0 AND 6)`.
- Los `heading` existentes pasan a `heading_1`, porque ya se pintaban con el tamaño mayor.
- Numeración calculada, no guardada: un contador por nivel; un bloque no numerado en el mismo nivel corta la lista, y volver a un nivel menor reinicia los más profundos.
- `save_document` sincroniza la lista completa de la página en una transacción: inserta nuevos, actualiza cambiados, borra ausentes y fija `position` = índice. Ids duplicados, ids de otra página o datos inválidos abortan sin cambios.

## Consecuencias
- SQLite no permite cambiar un `CHECK`: la migración `0003` reconstruye `blocks`. Copia los `rowid` explícitamente (el índice FTS external-content depende de ellos) y después reconstruye `block_search`. Todo en la transacción única de migración, con la copia previa automática.
- Sin toggles: si algún día hacen falta, será un ADR nuevo con migración a árbol (la sangría permite derivar padres).
- Un bloque huérfano en datos antiguos (imposible con claves foráneas activas) haría fallar la migración en vez de perderse en silencio.

## Migración futura
Pasar a árbol: derivar `parent_block_id` recorriendo la lista con una pila por sangría, en una migración aditiva.
