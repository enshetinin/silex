---
paths:
  - "src/**/*.rs"
  - "tests/**/*.rs"
---

# Reglas Rust para Silex

- Edición 2024; preferir claridad frente a macros o capas innecesarias.
- La lógica de SQLite se verifica mediante tests con base en memoria y tests de apertura en disco.
- Para entradas, usar `rusqlite::params!` (nunca `format!` para construir SQL con datos del usuario).
- Operaciones multi-tabla bajo una transacción con rollback ante error.
- Estados de error visibles para el usuario; cero panics por E/S, SQLite o texto inválido.
- El dominio no debe depender de `egui`. La UI consume servicios/modelos, no tipos SQL.
- Revisar ergonomía de teclado, accesibilidad, estados vacíos, latencia y persistencia al cerrar.
- Antes de añadir un crate, justificarlo y verificar mantenimiento y licencia.
