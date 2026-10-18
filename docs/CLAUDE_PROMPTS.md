# Prompts recomendados para Claude Code

## Sesión 1: inspección en modo plan
```text
Lee CLAUDE.md, docs/PRD.md, docs/ARCHITECTURE.md y docs/ROADMAP.md.
Inspecciona todo el starter. En modo plan, prepara el Hito 0 para mi sistema operativo.
Identifica APIs de crates que puedan haber cambiado, fallos de compilación,
problemas de migración/FTS y tests faltantes. No toques archivos todavía.
Entrega un plan breve con criterios de aceptación verificables.
```

## Sesión 2: compilar y probar
```text
Implementa únicamente el Hito 0 aprobado. Mantén la GUI 100% Rust/egui.
Ejecuta cargo fmt --all -- --check, cargo clippy --workspace --all-targets -- -D warnings,
cargo test --workspace y cargo check --workspace. Corrige errores sin silenciar lints.
Arranca la aplicación si este entorno tiene GUI y prueba crear página, escribir,
guardar y reabrir. Reporta comandos y resultados reales.
```

## Sesión 3: persistencia robusta
```text
Lee docs/DATA_MODEL.md. Diseña el Hito 1, comenzando por backups SQLite
consistentes y restauración. Primero propone la migración y los tests de
integridad/crash, después el código. No utilices shutil/copia del archivo
.sqlite3 en caliente con WAL. Mantén el diseño local-first.
```

## Sesión 4: editor avanzado
```text
Haz una propuesta de arquitectura del editor por bloques antes de implementarla.
Compara operaciones y modelos de selección/cursor/IME. Mantén modelos
independientes de egui. Diseña tests para Enter, Backspace, Tab, listas,
checkbox, undo/redo, pegado y documentos grandes. Limítate al primer slice.
```

## Revisión adversarial
```text
Revisa el diff como un revisor exigente. Busca pérdida de datos, SQL incorrecto,
FTS desincronizado, fugas de recursos, migraciones no reversibles,
regresiones de teclado/IME, bloqueo del hilo UI, accesibilidad y requisitos
violados. Señala archivo/línea y severidad. No edites nada durante la revisión.
```

## Buenas prácticas de sesiones
- Una tarea concreta por sesión; usa Plan Mode para cambios multiarchivo.
- Incluye aceptación y comandos reproducibles en cada petición.
- Referencia archivos por ruta en vez de pegar código extensísimo.
- Solicita pruebas y evidencias, no afirmaciones de que todo funciona.
- Cambios acotados en commits legibles; revisa el diff.
- No instales un MCP/plugin externo hasta que una tarea real lo necesite.
