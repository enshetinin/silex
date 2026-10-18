# ADR-0004 — Deshacer/rehacer de documento con instantáneas

**Estado:** aceptada (2026-10-18). Sustituye a ADR-0002.

## Contexto
ADR-0002 usaba el deshacer de egui por campo de texto. Con el editor de bloques (H3.2–H3.3), Intro, Retroceso, Tab, el menú `/`, mover, duplicar o borrar bloques cambian la estructura del documento. El deshacer por campo no puede revertir esas operaciones, y además podía devolver texto que ya se había movido a otro bloque.

## Alternativas
1. **Instantáneas del documento** (título + lista de bloques) en memoria, una por paso. Simple y correcto para cualquier operación presente o futura. Cuesta memoria proporcional al tamaño de la página por paso.
2. **Comandos inversos** (cada operación sabe deshacerse). Usa menos memoria, pero cada operación nueva necesita su inversa probada; es fácil equivocarse.
3. **`document_operations` en SQLite.** Historial persistente; prematuro sin requisito de historial entre sesiones.

## Decisión
Opción 1 (`src/history.rs`).
- Cada cambio guarda el estado **anterior**. Escribir seguido en el mismo campo, sin pausas de más de 1 s, es un solo paso. Cualquier otro cambio es un paso propio.
- Máximo 100 pasos por página. El historial se vacía al cambiar de página, al restaurar una copia y al mostrar el estado vacío.
- Cmd/Ctrl+Z, Cmd/Ctrl+Mayús+Z y Cmd/Ctrl+Y se interceptan antes que los campos de texto cuando el foco está en el título, en un bloque o en ninguno (selección de bloques). En el buscador y en el renombrado de páginas sigue funcionando el deshacer de egui.
- Al deshacer o rehacer, el cursor va al final del primer bloque que cambia. Los ids de los campos se renuevan (`doc_generation`) para que egui no conserve cursores ni historial anteriores.
- El resultado se guarda por la vía normal (autoguardado y sincronización de `save_document`).

## Consecuencias
- Tiempo: clonar y comparar una página de 1000 bloques cuesta unos 85 µs por pulsación (medido en el test de escala, macOS).
- Memoria: una página de 1000 bloques ocupa del orden de cientos de KB por paso. Con 100 pasos, decenas de MB en el peor caso. Si hiciera falta, se compartirían bloques sin cambios (`Arc<Block>`) sin cambiar la interfaz.
- El historial no sobrevive a reiniciar la app.
- Las operaciones sobre páginas (mover, papelera, renombrar otra página) no entran en este historial; la papelera tiene su propia restauración.
