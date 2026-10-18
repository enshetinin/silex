---
name: next-milestone
description: Planifica e implementa el siguiente hito de Silex siguiendo PRD, roadmap, tests y criterios de aceptación.
disable-model-invocation: true
---

Lee `CLAUDE.md`, `docs/PRD.md`, `docs/ARCHITECTURE.md` y `docs/ROADMAP.md`.

1. Identifica el **primer hito incompleto**. Comprueba el código y no te bases en lo que afirma el roadmap.
2. Explica de forma concisa alcance, exclusiones, archivos afectados y criterios de aceptación. Si cambia el esquema, diseña migración antes que UI.
3. Implementa la porción vertical más pequeña, preferentemente con tests primero.
4. Ejecuta `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` y `cargo check --workspace`. Corrige los fallos que introduzcas.
5. No inventes resultados. Anota en `docs/ROADMAP.md` únicamente lo verificado, con límites pendientes.
6. Entrega un resumen de diff, evidencia de pruebas y siguiente paso recomendado.

Nunca modifiques datos locales existentes para probar sin una copia segura. No realices push ni release sin permiso.
