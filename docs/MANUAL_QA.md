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
- Windows: ejecuta `Silex_<versión>_x64-setup.exe`. SmartScreen avisará (no está firmado): «Más información» → «Ejecutar de todas formas». No pide permisos de administrador y se instala en `%LOCALAPPDATA%\Programs\Silex`. El icono de Silex aparece en el instalador, en el acceso directo del escritorio y del menú Inicio, en la barra de tareas y en «Aplicaciones instaladas». Desinstala desde ahí: los datos siguen en `%LOCALAPPDATA%\silex\Silex\data`, y al reinstalar vuelven a aparecer.
- Linux (`.deb`): `sudo apt install ./silex-desktop_<versión>_amd64.deb`. Silex aparece en el menú de aplicaciones con su icono, y la ventana abierta usa ese icono en el dock o la barra (X11 y Wayland).
- Linux (AppImage): `chmod +x silex-desktop_<versión>_x86_64.AppImage` y ábrelo. Debe arrancar en Ubuntu 22.04 o posterior.

## 9. Bases de datos (H6.1)
- «+» de «Páginas» → «Nueva base de datos»: se abre con el título listo para escribir, la pestaña «Tabla», la columna «Nombre» y «Todavía no hay filas.». En la barra lateral aparece con el icono de tabla.
- «Nueva fila» (o Cmd+Mayús+N, o la paleta): aparece una fila con el cursor en su nombre. Escribe y pulsa Intro; añade varias seguidas.
- Edita un nombre y, sin pulsar Intro, cambia de página o cierra la app: al volver, el nombre está guardado.
- «Abrir» en una fila: se abre como página, con la ruta «Base › Fila» arriba; escribe bloques y vuelve a la base desde la ruta. Las filas nunca aparecen en la barra lateral.
- Clic derecho en una fila: «Renombrar» pone el cursor en su celda; «Enviar a la papelera» la quita de la tabla, y restaurarla desde la papelera la devuelve a su base.
- Envía la base entera a la papelera y restáurala: vuelve con todas sus filas.
- Cmd+K: busca el nombre de una fila; el resultado muestra la ruta de la base.
- «…» → «Exportar a Markdown» en la base: un `.md` con una tabla de nombres. «Exportar todo» incluye además una carpeta con las filas.
- Con la semilla (`cargo run --example seed -- /tmp/silex-qa.sqlite3`), «Base de prueba» tiene 1000 filas: se abre al momento y el desplazamiento de la tabla es fluido.

## 10. Propiedades (H6.2a)
Con una base de datos abierta (o «Base de prueba» de la semilla):
- «Propiedad» (junto a «N filas») → escribe «Páginas» → «Número»: aparece la columna con el icono #. Repite con «Leído» (Casilla), «Terminado» (Fecha), «Estado» (Selección) y un texto sin nombre (se llama «Texto»).
- Número: clic en la celda, escribe `736,5` e Intro → se ve «736,5» alineado a la derecha. Escribe `doce` → aviso «no es un número» y la celda conserva su valor. Escribe algo y pulsa Escape → no cambia.
- Fecha: `19/10/2026` → se ve «2026-10-19». `30/02/2026` → aviso y sin cambios.
- Casilla: clic la marca y otro clic la desmarca. Con el teclado: Tab hasta la celda y Espacio.
- Selección: clic → escribe «Leído» e Intro → se crea la opción con color y queda puesta. En otra fila, escribe «lei» e Intro → elige «Leído», sin duplicarla. «Quitar la opción» vacía la celda.
- Cabecera de una propiedad: renombrar (Intro o clic fuera guardan; Escape no), «Mover a la izquierda/derecha» y «Borrar la propiedad» (aviso). «…» → «Propiedades borradas» → «Restaurar» la devuelve al final con sus valores.
- Escribe en una celda y, sin salir, cambia de página o cierra la app: al volver, el valor está guardado.
- Ventana por defecto (1100×760) con 5 o más propiedades: la tabla se desplaza en horizontal (barra fina bajo las filas, Mayús+rueda o trackpad) y la cabecera la sigue; el título no se mueve. Con 1000 filas el desplazamiento vertical es fluido y la cabecera queda fija.
- Cierra y vuelve a abrir la app: propiedades, orden y valores siguen ahí.
- Modo oscuro: las etiquetas de las opciones se leen bien.
- Con VoiceOver/NVDA/Orca: cada celda se anuncia como «Propiedad, fila: valor» y las casillas como casilla marcada o no.

## 11. Filas como páginas, teclado y opciones (H6.2b)
- «Abrir» en una fila: bajo el título aparecen sus propiedades con icono y nombre a la izquierda. Los números van a la izquierda, y lo vacío dice «Vacío». Edita un texto, un número, una fecha, la casilla y la selección (crea una opción nueva): vuelve a la base por la ruta y la tabla muestra los cambios.
- En la página de la fila, escribe en una propiedad y, sin salir del campo, cambia de página: al volver está guardado.
- En la tabla, clic en una celda y Escape (queda con el foco): ↑/↓/←/→ la mueven. En el nombre, ←/→ mueven el cursor y → al final del nombre pasa a la primera propiedad. Intro edita la celda enfocada; Intro otra vez guarda y deja el foco en ella.
- Con la semilla (1000 filas): mantén ↓ pulsada desde la primera fila; la tabla baja con el foco y la cabecera sigue fija. → hasta la última columna desplaza la tabla en horizontal.
- Cabecera de una propiedad de selección: aparece «Opciones». Cambia el nombre de una (Intro o clic fuera), su color (clic en la muestra → elige otro) y bórrala: pide confirmación y dice en cuántas filas se vacía; «Cancelar» no borra. Las celdas que la tenían quedan vacías; las demás, igual.
- El menú de opciones no cambia de tamaño ni se mueve mientras está abierto.
- Con VoiceOver/NVDA/Orca: en la página de la fila cada valor se anuncia como «Propiedad, fila: valor»; las muestras de color se anuncian con el nombre del color.

## 12. Ordenar, filtrar y columnas (H6.3a)
Con la semilla («Base de prueba», 1000 filas y 5 propiedades):
- «Filtrar» → «+ Añadir un filtro» → «Número» → «mayor que» → escribe `1000`: el recuento dice «N de 1000 filas» y la tabla se actualiza al teclear. «Filtrar (1)» queda acentuado. Escribe `abc`: aparece «Valor no válido» y se ven todas las filas.
- Añade un segundo filtro «Estado» «es» «Hecho»: solo filas que cumplen los dos. Quita uno con «×».
- Filtra hasta que no quede ninguna: «Ninguna fila coincide con los filtros.»
- «Ordenar» → «Notas» → «Descendente»; añade un segundo orden. Las filas vacías van al final.
- Con un filtro activo, edita una celda para que la fila deje de cumplirlo: la fila no desaparece mientras está en pantalla. Cambia de página y vuelve: ya no está.
- «Nueva fila» con un filtro activo: la fila nueva aparece al final, aunque esté vacía.
- «Columnas»: desmarca «Notas» → desaparece de la tabla (y las flechas la saltan); sigue en la página de una fila. Cabecera de una propiedad → «Ocultar en esta vista» hace lo mismo. «Mostrar todas» las devuelve.
- Cierra y vuelve a abrir la app: filtros, orden y columnas ocultas siguen ahí.
- Los menús no cambian de tamaño ni se mueven mientras están abiertos; elegir una columna u operador no los cierra.

## 13. Varias vistas (H6.3b)
- «+» junto a «Tabla»: aparece «Tabla 2» y se abre. Ponle un filtro; vuelve a «Tabla»: no tiene ese filtro. Vuelve a «Tabla 2»: sigue ahí.
- Clic en la pestaña abierta: menú con su nombre listo para cambiar (Intro o clic fuera lo guardan; Escape no). «Duplicar la vista» crea una copia con sus reglas. «Mover a la izquierda/derecha» cambia el orden de las pestañas.
- Clic derecho en una pestaña que no está abierta: abre su menú sin cambiar de vista.
- «Borrar la vista»: pide confirmación y dice que las filas no se pierden; «Cancelar» no borra. Con una sola vista, «Borrar la vista» está desactivado.
- Abre otra página y vuelve a la base: se abre en la última vista usada. Cierra la app y vuelve: las vistas, sus nombres y reglas siguen (se abre en la primera).

## 14. Deshacer en la tabla y exportación (H6.4)
- Cambia un número, marca una casilla y elige una opción; pulsa Cmd+Z tres veces (sin estar escribiendo): vuelven los valores anteriores, uno a uno. Cmd+Mayús+Z los rehace. Los botones de la barra superior hacen lo mismo.
- Renombra una fila en su celda (Intro) y Cmd+Z: vuelve el nombre anterior.
- «Nueva fila» y Cmd+Z: la fila va a la papelera (aparece en ella). Cmd+Mayús+Z la devuelve.
- Clic derecho en una fila → «Enviar a la papelera» y Cmd+Z: vuelve con sus valores.
- Borra una propiedad y Cmd+Z: vuelve con sus valores.
- Escribiendo en una celda, Cmd+Z deshace solo el texto del campo.
- Cambia de página y vuelve: el historial de la tabla empieza vacío.
- «…» → «Exportar a Markdown»: el `.md` tiene una columna por propiedad, con las casillas como `[x]`/`[ ]` y las selecciones por su nombre. Ábrelo en un visor de Markdown: la tabla se ve bien aunque haya `|` en algún texto.

## 15. Tablero (H6.5)
- En una base con una propiedad de selección (p. ej. «Estado» con dos opciones): «+» junto a las pestañas → «Tablero». Aparece «Tablero» con una columna «Sin Estado» y una por opción, cada una con su número.
- Arrastra una tarjeta a otra columna con el ratón: la columna se ilumina al pasar por encima y, al soltar, la tarjeta queda allí. Cambia a «Tabla»: la celda tiene el valor nuevo. Vuelve y pulsa Cmd+Z: la tarjeta vuelve a su columna.
- Suelta una tarjeta fuera de cualquier columna: no cambia nada.
- Clic en una tarjeta: se abre la fila como página. Tab hasta una tarjeta e Intro: lo mismo.
- Clic derecho en una tarjeta → «Mover a» → otra columna: se mueve. El resto del menú es el de siempre (renombrar, papelera…).
- «Nueva» al pie de una columna: aparece una tarjeta en ella con el nombre listo para escribir; Intro lo guarda. Cmd+Z la manda a la papelera.
- «Nuevo grupo» al final (desplazando a la derecha si hace falta): escribe un nombre e Intro → nueva columna.
- Clic en la cabecera de una columna (no en «Sin …»): menú con su nombre (se guarda al cerrar; Escape lo descarta), sus colores, «Mover a la izquierda/derecha» (cambia el orden de las columnas y de las opciones en la tabla) y «Borrar el grupo», que pide confirmación y dice cuántas tarjetas pasan a «Sin …»; «Cancelar» no borra.
- «Agrupar» con dos propiedades de selección: cambiar de una a otra reagrupa las tarjetas. «Propiedades»: desmarca una y desaparece de las tarjetas. Un filtro deja en cada columna solo las tarjetas que lo cumplen.
- En una base sin propiedad de selección, un tablero explica por qué está vacío y «Añadir la propiedad «Estado»» la crea.
- Cierra y vuelve a abrir la app: el tablero sigue agrupado por la misma propiedad.

## 16. Calendario en las fechas (H6.6)
- Clic en una celda de fecha vacía: se abre el calendario en el mes actual, con hoy rodeado. «‹» y «›» cambian de mes; clic en un día lo guarda y cierra.
- En una celda con fecha, el calendario se abre en su mes con su día marcado. «Quitar la fecha» la vacía; Cmd+Z la devuelve.
- Escribe `19/10/2026` en el campo: el calendario salta a ese mes; Intro la guarda como `2026-10-19`. Escribe `30/02/2026`: aparece la explicación y no se guarda; Escape cierra sin guardar.
- Con el teclado: flechas hasta la celda, Intro abre el calendario, Tab recorre los días, Intro elige.
- Lo mismo en la página de una fila (propiedades bajo el título).
- Pasada la medianoche (hora local), comprueba si «Hoy» marca el día correcto (usa UTC: en España, hasta la 1 o las 2 puede marcar el anterior).

## 17. Adjuntos (H7.1)
- Arrastra un PDF y una imagen a una página: aparece «Añadiendo…» y después una tarjeta por archivo, tras el bloque en el que estabas, con su icono, nombre y tamaño.
- Pulsa Cmd+Z: desaparecen; Cmd+Mayús+Z: vuelven.
- «Abrir»: se abre con la aplicación del sistema (Vista Previa, el visor de PDF…). Cambia la copia abierta y vuelve a abrirla desde Silex: tiene el contenido original (lo dice la ayuda del botón).
- «Guardar una copia»: aparece en Documentos/Silex/Exportaciones; repítelo y sale «nombre (2)», sin pisar la primera.
- Arrastra un script (`.sh`, `.command`, `.exe` en Windows): la tarjeta dice «Mostrar en la carpeta» y abre la carpeta, nunca el script.
- Arrastra un archivo de más de 20 MB y una carpeta: el aviso explica por qué no se adjuntaron.
- Con el teclado: ↓ desde el texto llega a la tarjeta (con el anillo de foco), Intro la abre, Retroceso la borra y Cmd+Z la devuelve. Retroceso al principio del párrafo de debajo no mezcla nada.
- Busca (Cmd+K) una palabra del nombre del archivo: aparece la página.
- Envía la página a la papelera y restáurala: los archivos siguen ahí. Haz una copia de seguridad, restáurala y abre un archivo.
- «…» → «Exportar a Markdown»: junto al `.md` aparece `Título (adjuntos)/` con los archivos, y el `.md` los enlaza (ábrelo en un visor de Markdown: la imagen se ve y el PDF se abre).
- Suelta un `.md` y un PDF a la vez: se importa la página y el PDF se adjunta a la que estaba abierta; el aviso cuenta las dos cosas.
