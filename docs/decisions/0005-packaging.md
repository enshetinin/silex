# ADR-0005 — Empaquetado, icono e identificador de la app

**Estado:** aceptada (2026-10-18).

## Contexto
El PRD pide instaladores por sistema operativo (P1) y el Hito 5, «empaquetado e iconos multiplataforma». Hasta ahora Silex solo se ejecutaba con `cargo run`. Una app empaquetada se abre desde el Finder o el Explorador, sin terminal, así que los errores de arranque no se ven en ningún sitio. Y el identificador de la app en macOS es difícil de cambiar después: de él dependen permisos, preferencias y, en Silex, la carpeta de datos.

## Alternativas
1. **cargo-packager** (CrabNebula, el equipo de Tauri; MIT/Apache-2.0). Una sola configuración en `Cargo.toml` para `.app`/`.dmg`, instalador NSIS o MSI de Windows y `.deb`/AppImage/pacman de Linux, con firma y notarización opcionales.
2. **Scripts nativos por sistema** (`hdiutil`/`codesign`, WiX o NSIS, `appimagetool`). Sin herramientas nuevas, pero son tres caminos distintos que mantener.
3. **cargo-bundle.** Parecido a la opción 1, pero con menos mantenimiento.
4. **cargo-dist.** Pensado para herramientas de línea de comandos: no genera `.app` ni `.dmg`.

## Decisión
Opción 1, como herramienta externa (no es dependencia del crate): `cargo install cargo-packager --locked`, versión 0.11.8. La configuración está en `[package.metadata.packager]` de `Cargo.toml`. `cargo packager --release` compila en release y deja los paquetes en `dist/`, que Git ignora.

- **Identificador `dev.silex.Silex`.** Coincide con la carpeta de datos que da `directories` en macOS (`~/Library/Application Support/dev.silex.Silex`). No debe cambiar nunca: los datos y los ajustes quedarían en otra carpeta.
- **Formatos.** macOS: `.app` y `.dmg` (H5.9). Windows: instalador NSIS; Linux: `.deb` y AppImage. Los de Windows y Linux se construirán y comprobarán en CI (H5.10).
- **Icono.** La fuente es `assets/app-icon/silex.svg`. `scripts/make-icons.sh` genera a partir de ella los PNG (16–1024 px), `Silex.icns` y `silex.ico`, con `rsvg-convert`, `iconutil` y `examples/pack_ico.rs`, y todos se versionan. Así, compilar o empaquetar no depende de esas herramientas. La ventana usa el PNG de 512 px incrustado en el binario (`eframe::icon_data`).
- **Linux.** El `app_id` de la ventana es `silex-desktop`, igual que el archivo `.desktop` que genera cargo-packager.
- **Sin `license-file`.** Con él, el `.dmg` y el instalador de Windows obligan a aceptar la licencia antes de abrirse, y la MIT no requiere aceptación.
- **Firma.**
  - macOS: firma ad-hoc (`signing-identity = "-"`) con hardened runtime. El paquete queda sellado, pero sin Developer ID ni notarización, así que Gatekeeper avisa al abrirlo por primera vez (`spctl` lo rechaza). Se abre con clic derecho → Abrir, o en Ajustes → Privacidad y seguridad → «Abrir igualmente».
  - Windows: sin firmar (SmartScreen avisará).
  - La firma real llegará cuando haya certificados. Sus credenciales irán solo en variables de entorno o secretos de CI, nunca en el repositorio.
- **Arranque sin terminal.** En Windows, la versión release es una app de ventana, sin consola (`windows_subsystem`). En todos los sistemas, si la app no puede abrir la base, `StartupError` muestra el error y el archivo de datos en una ventana en lugar de cerrarse en silencio.

## Consecuencias
- **Herramienta externa:** hay que instalar cargo-packager en local y en CI, y la versión se fija a mano. La primera vez que empaqueta para Windows o Linux descarga NSIS o las herramientas de AppImage. Esa red se usa solo al empaquetar; la app no la usa nunca.
- **Arquitectura:** el `.dmg` es de la arquitectura de la máquina que lo construye (`aarch64` en Apple Silicon). Intel o binario universal: pendiente.
- **Carpeta de datos compartida:** la app empaquetada y `cargo run` usan la misma carpeta de datos, salvo que se defina `SILEX_DB_PATH`.
- **Sin actualizaciones automáticas:** necesitan red y un servidor, lo que choca con el principio local-first. Tendrán su propio ADR si se piden.
