# Silex

Un espacio de trabajo local para escribir y organizar notas, al estilo de Notion. Todo se guarda en una base SQLite en tu equipo: sin cuentas, sin nube y sin telemetría. La interfaz es nativa, escrita en Rust con egui, y funciona en macOS, Windows y Linux.

- Páginas anidadas, editor por bloques (títulos, listas, tareas, citas, código, archivos e imágenes, enlaces entre páginas) con menú `/` y arrastrar bloques.
- Búsqueda global y paleta de comandos (Cmd/Ctrl+K), y buscar y reemplazar en la página (Cmd/Ctrl+F).
- Bases de datos con tablas y tableros, filtros, orden y varias vistas.
- Pestañas, historial de versiones, plantillas y papelera.
- Copias de seguridad automáticas, exportación a Markdown y CSV, e importación de Markdown.

La lista completa está en [CHANGELOG.md](CHANGELOG.md).

> [!WARNING]
> **Advertencia de seguridad — Aplicación sin firma digital**
>
> Silex todavía no dispone de certificados de firma digital. Por este motivo, **macOS y Windows pueden mostrar advertencias de seguridad al instalar o abrir la aplicación**, indicando que el desarrollador no está identificado o que el software no es de confianza.
>
> Estas advertencias aparecen porque la aplicación no está firmada digitalmente, **no porque se haya detectado software malicioso**.
>
> Descarga Silex únicamente desde las [versiones oficiales de GitHub](https://github.com/enshetinin/silex/releases). Más abajo encontrarás las instrucciones para continuar con la instalación en cada sistema operativo.

## Instalar

Descarga el paquete de tu sistema en [Versiones](https://github.com/enshetinin/silex/releases). Los paquetes aún **no están firmados**, así que el sistema avisa la primera vez:

- **macOS** (Apple Silicon): abre el `.dmg` y arrastra Silex a Aplicaciones. La primera vez, clic derecho sobre Silex → Abrir, o Ajustes del sistema → Privacidad y seguridad → «Abrir igualmente».
- **Windows**: ejecuta el instalador (`…-setup.exe`). Se instala solo para tu usuario, sin pedir permisos de administrador. Si aparece SmartScreen: «Más información» → «Ejecutar de todas formas».
- **Linux**: instala el `.deb` (`sudo apt install ./<archivo>.deb`) o da permiso de ejecución al AppImage y ábrelo.

Para saber si hay una versión nueva, usa «Buscar actualizaciones…» en Ajustes: abre esa página en el navegador. Silex no se conecta a internet por su cuenta.

## Tus datos

Silex guarda la base y sus copias de seguridad en:

| Sistema | Carpeta |
| --- | --- |
| macOS | `~/Library/Application Support/dev.silex.Silex` |
| Windows | `%LOCALAPPDATA%\silex\Silex\data` |
| Linux | `~/.local/share/silex` |

Desinstalar no borra esa carpeta. En Ajustes, «Privacidad» muestra la ruta y «Mostrar la carpeta» la abre. Las exportaciones van a `Documentos/Silex/Exportaciones`.

## Compilar desde el código

Necesitas Rust (la versión fija de `rust-toolchain.toml`). En Linux, además: `libxkbcommon-dev libwayland-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev pkg-config`.

```sh
cargo run --release
```

Para probar sin tocar tus datos, usa otra base: `SILEX_DB_PATH=/tmp/prueba.sqlite3 cargo run`.

Antes de enviar cambios, pasa las comprobaciones de [CLAUDE.md](CLAUDE.md) (`cargo fmt`, `clippy`, `test` y `check`) y lee [CONTRIBUTING.md](CONTRIBUTING.md). Los instaladores se generan con `cargo packager --release` (ADR 0005), y una versión se publica como explica el [ADR 0014](docs/decisions/0014-releases.md).

## Documentación

- [docs/PRD.md](docs/PRD.md): alcance y criterios.
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) y [docs/decisions/](docs/decisions/): cómo está hecho y por qué.
- [docs/DATA_MODEL.md](docs/DATA_MODEL.md): el esquema de la base.
- [docs/ROADMAP.md](docs/ROADMAP.md): lo hecho y lo que viene.
- [SECURITY.md](SECURITY.md): cómo informar de un problema de seguridad.

## Licencia

MIT. Ver [LICENSE](LICENSE).
