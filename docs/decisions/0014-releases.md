# ADR-0014 — Publicar versiones y «Buscar actualizaciones»

**Estado:** propuesta (2026-10-22), pendiente de aceptar por el propietario.

## Contexto
H9.4 prepara la 1.0: un CHANGELOG, la versión 1.0.0, paquetes que se publican al etiquetar y una forma de saber si hay una versión nueva. Hasta ahora el workflow «Paquetes» (ADR 0005, H5.10) construye y comprueba los instaladores y los guarda como *artifacts* del run, sin publicar nada.

Dos principios limitan la decisión:
- **Local-first y sin telemetría** (`CLAUDE.md`, PRD). La app no debe conectarse a ningún servidor por su cuenta, tampoco para mirar si hay versiones nuevas: esa petición diría a GitHub cuándo y desde dónde se usa Silex. El ADR 0005 ya descartó las actualizaciones automáticas.
- **Publicar es irreversible y público.** Una versión publicada la descarga la gente, así que no debe salir sola de un push.

## Decisión propuesta
1. **Una versión es una etiqueta `vX.Y.Z` en `main`.** La crea a mano el propietario, después de la PR de `develop` a `main` y de la verificación manual. Nunca la crean la CI ni un asistente.
2. **El workflow «Paquetes» también se lanza con esas etiquetas.** Antes de empaquetar comprueba, y si no se cumple falla sin construir nada:
   - que la etiqueta sin la `v` es la versión de `Cargo.toml`;
   - que `CHANGELOG.md` tiene una sección `## [X.Y.Z]`.
   Después construye y comprueba los paquetes como hasta ahora, en macOS, Windows y Ubuntu 22.04.
3. **Crea un borrador de release, no una publicada.** Un último job, solo para etiquetas, junta los paquetes de los tres sistemas y crea con `gh release create --draft` la release «Silex X.Y.Z» con esos archivos. Sus notas son la sección del CHANGELOG (`scripts/release-notes.sh`). El propietario la revisa en GitHub y la publica con un clic. Solo ese job tiene permiso de escritura (`contents: write`); los demás siguen con `contents: read`. Si la release ya existe, el job falla en lugar de sobrescribirla.
4. **«Buscar actualizaciones» abre la página de versiones en el navegador.** Está en Ajustes, con la versión instalada al lado («Silex 1.0.0»), y en la paleta. Abre `https://github.com/enshetinin/silex/releases` (`CARGO_PKG_REPOSITORY` + `/releases`) con `egui::Context::open_url`, que usa el navegador del sistema (`webbrowser`, ya presente a través de eframe). La app no hace ninguna petición de red: compara el usuario, a la vista. El texto lo dice: «Abre la página de versiones en el navegador. Silex no se conecta a internet.»
5. **CHANGELOG** en `CHANGELOG.md`, en español y con el formato de *Keep a Changelog*: una sección por versión, con «Añadido», «Cambiado», «Corregido» y «Límites conocidos». Solo cuenta lo verificado, como el roadmap.
6. **Versión semántica.** 1.0.0 fija el formato de la base de datos como estable: las migraciones siguen siendo aditivas y una versión nueva siempre abre la base de una anterior (ya lo exigen `CLAUDE.md` y los tests de migración). Un cambio que obligue a exportar e importar sería una 2.0.

## Alternativas descartadas
- **Comprobar la versión desde la app** (pedir `releases/latest` a la API de GitHub): cómodo, pero es una conexión de red que revela el uso de la app. Si se pide, necesitará su propio ADR, desactivada por defecto.
- **Publicar la release directamente** desde la CI: un error en una etiqueta publicaría paquetes sin revisar.
- **Una acción de terceros** (`softprops/action-gh-release`): `gh` ya está en los runners de GitHub y no añade una dependencia con permiso de escritura en el repositorio.

## Consecuencias
- Los paquetes siguen **sin firmar** (ADR 0005): macOS avisa de un desarrollador no identificado y Windows muestra SmartScreen. El README y las notas de la versión explican cómo abrirlos.
- El `.dmg` es solo para Apple Silicon (lo construye `macos-latest`); Intel queda pendiente (ADR 0005).
- Saber si hay una versión nueva depende de que el usuario lo mire. Es el precio de no conectarse.
- Si una release sale mal, se borra el borrador y la etiqueta y se repite; una publicada no se reescribe: se publica la siguiente versión de parche.
