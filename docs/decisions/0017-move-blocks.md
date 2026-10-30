# ADR-0017 — Mover bloques a otra página y convertirlos en subpágina

**Estado:** aceptada (2026-10-30), con las tres decisiones del propietario.

## Contexto
H12.6 añade «Mover a…» (bloques a otra página) y «Convertir en subpágina» (los bloques pasan a una página nueva, y en su sitio queda un enlace). Es la primera operación que cambia **dos páginas a la vez**.

El deshacer del documento (ADR 0004) guarda instantáneas de la página abierta. Si la página de origen volviera sin más a su instantánea anterior, intentaría guardar unos bloques que ya pertenecen a otra página: el guardado fallaría por la clave primaria o duplicaría el contenido. Las pestañas (ADR 0011) guardan además el deshacer de cada página que no se ve, y ese deshacer podría borrar lo que ha llegado.

## Decisión
- **Mover cambia `blocks.page_id`** en una transacción (`Database::move_blocks`), conservando los ids. Con ellos se conservan los `rowid` (la búsqueda no se reindexa), los adjuntos, los enlaces y los backlinks. Antes se guarda la página de origen, así que lo que se mueve es lo último escrito.
- **Destino:** al final de la página elegida (decisión del propietario), en su orden y con su sangría. Se elige con el mismo buscador de páginas que los enlaces (ADR 0008). No se puede mover a la misma página ni a una base de datos. Un desplegable va con su contenido (ADR 0016).
- **Deshacer entre páginas.** Un paso de deshacer puede llevar, además de la instantánea, la lista de bloques que movió y a qué página (`history::Moved`).
  - **Cmd+Z en la página de origen** los trae desde donde estén ahora (también si después se movieron a una tercera página). Vuelven **con lo que se editó en el destino** (decisión del propietario): se leen de la base antes de restaurar. Un bloque que ya no existe vuelve como era.
  - **Cmd+Mayús+Z** los manda otra vez al final de su página de destino.
  - Si algo falla, el paso no se pierde y no cambia nada.
- **Las demás páginas olvidan su deshacer.** Una página a la que llegan bloques, o de la que se van al deshacer, pierde el historial que guardaba su pestaña, porque sus instantáneas ya no cuadran con la base. El panel de al lado se vuelve a leer si la muestra.
- **«Convertir en subpágina»** (H12.6b) usa lo mismo: crea una subpágina de la página abierta, le mueve los bloques y deja en su sitio un enlace a ella. El texto del primer bloque pasa a ser el título (decisión del propietario), y ese bloque no se repite dentro. Si no tiene texto (un archivo, un separador), la página se llama «Sin título» y todo es contenido. Deshacer devuelve los bloques y quita el enlace; la subpágina, vacía, va a la papelera.

## Consecuencias
- `History` guarda junto a cada paso un efecto opcional. Las instantáneas no cambian: el formato del historial de versiones (ADR 0009) y las plantillas no se tocan.
- La página de destino no recibe una versión en el historial de versiones por el movimiento: se crea con su próximo guardado, como cualquier cambio.
- No hay migración.

### Historias
1. **H12.6a — Mover a…:** `Database::move_blocks`, `history::Moved`, deshacer y rehacer entre páginas, el buscador en modo «mover», «Mover a…» en el menú del bloque (sobre la selección si el bloque está en ella) y en la paleta.
2. **H12.6b — Convertir en subpágina:** la página nueva con su título, el enlace en su sitio, y el deshacer que la manda a la papelera.
