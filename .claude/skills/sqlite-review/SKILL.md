---
name: sqlite-review
description: Audita migraciones, integridad, FTS5, transacciones y persistencia SQLite en Silex.
disable-model-invocation: true
---

Realiza una revisión de solo lectura del esquema, `src/db.rs`, migraciones y tests.

Comprueba: `PRAGMA foreign_keys`, versión de esquema, transaccionalidad, WAL y durabilidad, borrado lógico, índices, parámetros SQL, consistencia FTS5 tras INSERT/UPDATE/DELETE, backup y restauración, tests de upgrade, rutas de datos por OS y comportamiento ante fallos.

Entrega hallazgos priorizados por gravedad, con archivo y línea, propuesta de corrección y casos de prueba. No hagas cambios hasta que el usuario solicite implementarlos.
