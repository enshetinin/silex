# Pruebas manuales

Lo que los tests automáticos no pueden comprobar: IME reales, el portapapeles del sistema con otras aplicaciones y el dibujo de fuentes. Ejecútalas antes de cerrar un hito que toque el editor y anota el resultado (fecha, SO, versión) en la PR.

Usa siempre una base de prueba: `SILEX_DB_PATH=/tmp/silex-qa.sqlite3 cargo run`.

## 1. IME (composición de texto)
Para cada método de entrada: escribe en mitad de un bloque con texto, confirma con Intro y comprueba lo que se espera.

| Método | Cómo activarlo | Escribe | Esperado |
|---|---|---|---|
| Japonés (Romaji) | macOS: Ajustes → Teclado → Fuentes de entrada. Windows: Configuración → Idioma. Linux: IBus/Fcitx | `nihon` → Intro | «日本» (o la conversión elegida) en el **mismo** bloque, sin salto de línea ni bloque nuevo |
| Chino (Pinyin) | ídem | `zhongwen` → Espacio/Intro | «中文» en el mismo bloque |
| Coreano (2-Set) | ídem | `gksrmf` | «한글», sílabas compuestas correctamente |
| Teclas muertas | Teclado español: tecla de acento | `´` y luego `e` | «é» |
| Acentos en macOS | Mantener pulsada `e` | elegir «ê» | «ê» |

Durante la composición, Retroceso solo debe editar el texto en composición, nunca fusionar bloques. Después de confirmar, Intro vuelve a dividir el bloque.

## 2. Portapapeles con otras aplicaciones
1. Selecciona bloques (Esc, Mayús+↓), Cmd/Ctrl+C y pega en un editor de texto. Debe salir una línea por bloque, con sangría, «•», números y `[x]`.
2. Copia ese texto, pégalo en un bloque vacío de Silex: se reconstruyen tipos, sangría y tareas.
3. Copia varias líneas desde un navegador (con saltos de línea Windows `\r\n` si es posible) y pégalas: un bloque por línea, sin caracteres raros.
4. Pega una sola línea en mitad de un bloque: se inserta en el cursor y no se crean bloques.
5. Pega varias líneas dentro de un bloque de código: quedan dentro del bloque.

## 3. Dibujo de texto
Escribe o pega en bloques distintos y comprueba que se ven (no cuadrados vacíos):
- `año ñandú é` (é escrita como e + acento combinado)
- `日本語 中文 한글`
- `مرحبا بالعالم` y `שלום עולם` (derecha a izquierda)
- `👨‍👩‍👧 🇪🇸 😀`

Qué esperar:
- CJK y otros alfabetos: eframe carga **fuentes del sistema** para los caracteres que faltan (`system_fonts`, activo por defecto). Si alguno sale como cuadrados, anótalo con el SO.
- Árabe y hebreo: egui **no hace shaping ni texto bidireccional**, así que el árabe puede verse con letras sueltas y ambos en orden de izquierda a derecha. Es una limitación conocida del dibujo. Los datos se guardan, se buscan y se copian bien (lo cubren los tests de integridad Unicode).

## 4. Cursor y foco
- ↑/↓ en un bloque de varias líneas se mueve dentro del bloque y solo salta al anterior o siguiente desde la primera o la última línea.
- Tras Intro, Retroceso, Tab, pegar, deshacer o mover, el cursor queda donde se espera y la vista se desplaza hasta él en documentos largos (semilla: `cargo run --example seed -- /tmp/silex-qa.sqlite3`).

## 5. Accesibilidad y ajustes
- Con VoiceOver (macOS: Cmd+F5), NVDA (Windows) u Orca (Linux): recorre la barra lateral y la página. Cada botón de icono se anuncia con su nombre («Deshacer», «Más acciones», «Nueva subpágina»…) y cada bloque, con su tipo («Tarea», «Título 1»…).
- Tab / Mayús+Tab recorren los controles con un contorno visible en la barra lateral; Intro o Espacio activa el control enfocado. Nota: desde «Buscar», Tab pasa primero por el pie (Papelera, Copias, Atajos, Ajustes) y luego por el árbol.
- Ajustes → Oscuro / 125 %: se aplica al momento; cierra y abre la app y sigue igual. Cmd+− / Cmd++ / Cmd+0 cambian el tamaño y también se recuerda.

## 6. Búsqueda (Cmd+K)
Con la semilla (`cargo run --example seed -- /tmp/silex-qa.sqlite3`) y una página propia con texto conocido:
- Cmd/Ctrl+K y el botón «Buscar» abren la ventana con el cursor en el campo; Cmd/Ctrl+K otra vez, Esc o un clic fuera la cierran.
- Escribe una palabra a medias («ejem»): aparecen resultados al momento, sin tirones al teclear rápido. Primero las páginas cuyo título coincide; cada resultado muestra su ruta y un fragmento con la coincidencia en negrita.
- Busca sin acentos una palabra que los tiene («expedicion» → «expedición»).
- ↑/↓ mueven la selección (la lista se desplaza); Intro abre la página con el cursor en la coincidencia, y la vista se desplaza hasta ella en páginas largas.
- Escribe algo sin guardar, abre la búsqueda y busca esa palabra: aparece (se guarda al abrir).
- Con un IME (japonés): componer en el campo y confirmar con Intro no abre ningún resultado.
- Con VoiceOver/NVDA/Orca: el campo y cada resultado se anuncian (título, ruta y fragmento).
- Acciones (H5.8): con el campo vacío aparece la lista de acciones con sus atajos. Prueba al menos «Nueva página», «oscuro» (cambia el tema y se recuerda al reiniciar), «favorit» (añadir y, la segunda vez, quitar), «papelera» (abre la papelera, no borra), «aumentar» y «Ver los atajos de teclado». Con una base en archivo aparecen también las copias y «Exportar todo a Markdown».

## 7. Paquete de macOS (H5.9)
Genera el paquete con `cargo packager --release` (requiere `cargo install cargo-packager --locked`).
- Abre `dist/Silex_<versión>_<arquitectura>.dmg`: se ve `Silex` con su icono y un acceso a Aplicaciones. Arrástralo a Aplicaciones.
- Primera apertura: macOS avisa de que no puede comprobar el desarrollador (no hay firma de Developer ID). Clic derecho → Abrir, o Ajustes → Privacidad y seguridad → «Abrir igualmente». Las siguientes veces se abre sin aviso.
- El icono aparece en el Dock, en el selector Cmd+Tab y en el Finder; la ventana se llama «Silex».
- Los datos son los mismos que con `cargo run` (misma carpeta), salvo que uses `SILEX_DB_PATH`.
- Error de arranque: `SILEX_DB_PATH=/tmp/no-es-sqlite.txt /Applications/Silex.app/Contents/MacOS/silex-desktop` (con un archivo de texto cualquiera) muestra una ventana con el error y la ruta; «Copiar el error» y «Cerrar» funcionan, y el archivo no cambia.

## 8. Paquetes de Windows y Linux (H5.10)
Descárgalos de los *artifacts* del workflow «Paquetes» (GitHub → Actions → el run → Artifacts).
- Windows: ejecuta `Silex_<versión>_x64-setup.exe`. SmartScreen avisará (no está firmado): «Más información» → «Ejecutar de todas formas». No pide permisos de administrador. El icono de Silex aparece en el instalador, en el acceso directo del escritorio y del menú Inicio, en la barra de tareas y en «Aplicaciones instaladas». Desinstala desde ahí: los datos siguen en `%LOCALAPPDATA%\silex\Silex\data`, y al reinstalar vuelven a aparecer.
- Linux (`.deb`): `sudo apt install ./silex-desktop_<versión>_amd64.deb`. Silex aparece en el menú de aplicaciones con su icono, y la ventana abierta usa ese icono en el dock o la barra (X11 y Wayland).
- Linux (AppImage): `chmod +x silex-desktop_<versión>_x86_64.AppImage` y ábrelo. Debe arrancar en Ubuntu 22.04 o posterior.

