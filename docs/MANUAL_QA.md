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
- (H7.9) Con varios archivos grandes (cerca de 20 MB cada uno), exporta la página: mientras dura, el aviso dice «Exportando «Título» y sus adjuntos…» y se puede seguir escribiendo sin tirones. Al terminar, «Exportado a…» con «Abrir carpeta».
- Suelta un `.md` y un PDF a la vez: se importa la página y el PDF se adjunta a la que estaba abierta; el aviso cuenta las dos cosas.

## 18. Imágenes (H7.2)
- Arrastra una foto JPEG y un PNG: se ven dentro de la página, a su tamaño o al ancho de la columna, sin que la página salte cuando aparecen.
- Una foto muy alta (vertical) no pasa de unos 480 puntos de alto; una panorámica ocupa el ancho de la columna.
- Copia una captura de pantalla (Cmd+Mayús+Ctrl+4 en macOS) y pégala con Cmd+V en un párrafo: aparece «Imagen pegada.png» debajo. Cmd+Z la quita. Prueba también a copiar una imagen desde el navegador.
- Cambia el nombre de un `.txt` a `.png` y arrástralo: se queda como archivo, sin vista previa.
- Una página con muchas fotos grandes: al desplazarte, la app sigue fluida y la memoria no se dispara (Monitor de Actividad).
- Tema oscuro: la imagen y su pie se ven bien.

## 19. Diálogo de archivos (H7.3)
- En un párrafo vacío escribe `/archivo` e Intro: se abre el diálogo del sistema. Elige dos archivos (Cmd+clic): aparecen donde estaba el párrafo, sin dejar un párrafo vacío encima.
- Escribe `/pdf` o `/imagen`: el menú ofrece «Adjuntar archivos…».
- Abre el diálogo y cancela: no cambia nada (solo desaparece `/archivo`).
- Cmd+K → «adjuntar» → Intro, con el cursor en medio de la página: los archivos van tras ese bloque.
- En una base de datos, la paleta no ofrece «Adjuntar archivos…».
- Linux: prueba en GNOME o KDE (con portal) y, si puedes, en un escritorio sin portal; sin él, el aviso debe explicar el error y la app seguir funcionando.

## 20. Espacio (H7.4)
- Abre «Copias y exportación»: la sección «Espacio» dice cuánto ocupan la base, las copias y los adjuntos sin usar.
- Adjunta un archivo grande, bórralo de la página, cambia de página (para vaciar el historial) y vuelve al menú: aparece «1 adjunto sin usar».
- «Liberar espacio…» → lee la confirmación → «Liberar espacio»: el aviso dice cuánto se liberó, y en Finder el archivo `silex.sqlite3` ha encogido.
- Borra un archivo de la página y, sin cambiar de página, libera espacio: ese archivo no se borra (Cmd+Z lo devuelve y se abre).
- Tras liberar espacio, la búsqueda (Cmd+K) sigue encontrando todo, y crear y restaurar una copia funciona.
- «Cancelar» en la confirmación no borra nada.

## 21. Enlaces entre páginas (H7.5)
- En un párrafo vacío escribe `[[`: se abre «Enlazar a una página». Escribe parte de un título (sin tildes también vale), elige con ↑ ↓ e Intro: el enlace ocupa el lugar del párrafo. Cmd+Z lo quita.
- Escribe `[[` en medio de un párrafo con texto: los corchetes desaparecen y el enlace va debajo.
- `/enlace` e Intro, y Cmd+K → «enlazar»: abren lo mismo. Esc cierra sin cambiar nada.
- Clic en la tarjeta (o ↓ hasta ella e Intro): se abre la página enlazada.
- Renombra la página enlazada desde la barra lateral y desde su título: la tarjeta muestra el título nuevo, y Cmd+K con el título nuevo encuentra también la página que enlaza.
- Envía la página enlazada a la papelera: la tarjeta la muestra tachada «en la papelera». Ábrela: pregunta si restaurarla; «Restaurar y abrir» la devuelve y la abre.
- «Exportar todo a Markdown»: en la carpeta exportada, abre el `.md` de una página con enlaces en un visor de Markdown: el enlace abre el `.md` de la otra página, también en subcarpetas.
- «…» → «Exportar a Markdown» de una sola página: los enlaces salen como texto.

## 22. Backlinks (H7.6)
- Enlaza «Recetas» desde dos páginas y desde una fila de una base. Abre «Recetas»: al final aparece «Enlazada desde» con las tres, en orden alfabético, con su ruta en gris (la fila, con el nombre de su base).
- Una página con dos enlaces a «Recetas» aparece una sola vez. Una página sin enlaces hacia ella no muestra la sección.
- Clic en una de ellas: se abre. Con Tab hasta una y con Intro, también.
- Envía a la papelera una de las páginas que enlazan y vuelve a «Recetas»: ya no aparece. Restáurala: vuelve.
- Renombra una de las páginas que enlazan: la sección muestra el título nuevo.
- Con VoiceOver: cada fila se anuncia como enlace con el título de la página.

## 23. Historial de versiones (H7.7–H7.8)
Con la semilla (`cargo run --release --example seed -- /tmp/silex-qa.sqlite3`; después `SILEX_DB_PATH=/tmp/silex-qa.sqlite3 cargo run --release`), las páginas de «QA · Hito 7» (en Favoritos) traen lo necesario para las secciones 17–23: adjuntos (imagen, PDF, script, dos de 18 MB y uno sin usar), enlaces (también a una fila y a una página en la papelera), backlinks y versiones de días anteriores en «QA · Historial».
- Edita una página con algo de texto, espera más de 10 minutos y vuelve a editarla. «…» → «Historial de versiones…» lista una versión con la hora local («Hoy, 17:40»), no en UTC.
- ↑ ↓ eligen versión y la de la derecha cambia; no se puede editar. Las casillas, las listas, las citas, los archivos y los enlaces se ven como en la página.
- «Restaurar esta versión» (o Intro): la página vuelve a esa versión y lo dice. Cmd+Z la devuelve a como estaba. Abre el historial otra vez: lo de antes de restaurar es también una versión.
- Esc cierra sin cambiar nada. Cmd+K → «historial» abre lo mismo.
- En una base de datos el menú no ofrece el historial; en una fila, sí.
- Restaura una versión con un archivo adjunto que luego borraste de la página: el archivo vuelve. Haz «Liberar espacio» antes de restaurar: no lo borra.
- Cierra y vuelve a abrir la app: las versiones siguen ahí.
- Con VoiceOver: cada versión se anuncia con su fecha y hora, y el botón como «Restaurar esta versión».

## 24. Plantillas (H7.10–H7.11)
- Despliega «Plantillas» en la barra lateral: aparecen Reunión, Diario y Lista de tareas. Mándalas a la papelera, cierra y abre la app, despliega otra vez: no vuelven.
- En el menú «…» de una página: «Guardar como plantilla». Aparece en «Plantillas»; la página sigue igual y abierta. Una base de datos y una fila no lo ofrecen.
- Abre una plantilla: dice «Plantilla» (al pasar el ratón explica `{{fecha}}`) y no tiene estrella. Edítala, también su título; Cmd+Z y el historial funcionan.
- «Usar esta plantilla» (o el «+» de su fila): se crea y abre una página con `{{fecha}}` cambiado por la fecha de hoy, con el título listo para escribir. Los adjuntos y los enlaces de la plantilla funcionan en la página nueva.
- «+» de «Páginas» → «Nueva página desde plantilla…», y Cmd+K → «plantilla»: el buscador filtra al escribir, ↑ ↓ e Intro crean la página, Esc cierra sin crear nada. Desde el menú «…» de una página, la nueva va dentro de ella y lo dice.
- Las plantillas no salen en el árbol, en Cmd+K, al enlazar (`[[`) ni en «Enlazada desde». «Exportar todo» las escribe en `Plantillas/`.
- Con VoiceOver: «Plantillas» se anuncia como botón (desplegado o no), y cada plantilla del buscador por su título.

## 25. Pestañas (H8.1)
- Cmd/Ctrl+clic en una página de la barra lateral (o clic central, o «Abrir en una pestaña nueva» en su menú): aparece la barra de pestañas con las dos. Un clic normal cambia la página de la pestaña actual.
- Cmd/Ctrl+clic en una tarjeta de enlace y en «Enlazada desde»: en una pestaña nueva.
- Escribe en una pestaña, cambia a otra y vuelve: Cmd+Z deshace lo escrito, y la página está desplazada donde la dejaste.
- Ctrl+Tab y Ctrl+Mayús+Tab recorren las pestañas; Cmd/Ctrl+1…8 van a cada una y Cmd/Ctrl+9 a la última. En macOS, Ctrl+Tab es la tecla Control.
- Cmd/Ctrl+W y la «×» cierran la pestaña; la última no se cierra. Clic central en una pestaña también la cierra.
- Cmd/Ctrl+T abre la búsqueda; lo que elijas se abre en una pestaña nueva.
- Abre 13 pestañas: se cierra la que usaste hace más tiempo, nunca la que ves.
- Manda a la papelera una página con pestaña: la pestaña se cierra. Si era la activa, se ve la de al lado.
- Cierra la app con varias pestañas y vuelve a abrirla: vuelven, con la misma activa.
- Con VoiceOver: cada pestaña se anuncia como pestaña, con su título y si está seleccionada; su «×» como «Cerrar «Título»».

## 26. Vista dividida de consulta (H8.2)
- Alt+clic en una página de la barra lateral (o «Abrir al lado» en su menú): aparece a la derecha, con «Solo lectura»; la página abierta sigue editable.
- Alt+clic en una tarjeta de enlace o en «Enlazada desde»: también al lado.
- En el panel, un clic en un enlace abre esa página en el panel; un clic en un archivo lo abre con la app del sistema (un script solo se muestra en su carpeta).
- Una base de datos al lado muestra su tabla (nombre y propiedades); un clic en el nombre de una fila la abre en el panel. Con 1000 filas («Base de prueba» de la semilla) se desplaza con fluidez.
- Edita en la página principal la misma página que se ve al lado: tras el autoguardado, el panel la muestra al día.
- Manda a la papelera la página del panel: dice que está en la papelera.
- «Editar aquí» la pasa a la pestaña activa y cierra el panel; «×» lo cierra. Arrastra el borde del panel: cambia de ancho, sin dejar la página principal por debajo de 480 puntos.
- Estrecha la ventana: el panel desaparece y vuelve al ensancharla. Con la ventana estrecha, «Abrir al lado» explica por qué no se abre.
- Con VoiceOver: «Editar aquí» y «Cerrar el panel lateral» se anuncian como botones.

## 27. Datos locales más protegidos (H7.12)
- macOS y Linux: arranca la app con su base de siempre y comprueba `ls -l` de la carpeta de datos: la carpeta y `backups/` con `drwx------`, la base y las copias con `-rw-------`.
- Con `SILEX_DB_PATH=/tmp/silex-qa.sqlite3`: la base y sus copias quedan privadas, pero `/tmp` no cambia.
- Abre un adjunto con su app, cierra Silex: la carpeta de copias abiertas (en la caché de la app, `abiertos/`) ya no existe.
- Crea una carpeta `Dropbox` (o `OneDrive`) y arranca con `SILEX_DB_PATH` dentro de ella: aparece el aviso. «Entendido» lo cierra hasta el próximo arranque; «No volver a avisar», para siempre.
- Ajustes → «Privacidad»: dice dónde están los datos; «Mostrar la carpeta» la abre; explica el cifrado de disco del sistema. Con la base en una carpeta sincronizada, lo dice también ahí.
- Windows: arranca con normalidad (los permisos no se tocan) y el aviso de OneDrive funciona igual.

## 28. Cierre brusco (H9.1)
- Con la semilla, escribe en una página y, sin esperar, mata la app (`kill -9 <pid>` en macOS o Linux; «Finalizar tarea» en Windows). Ábrela: arranca sin errores y conserva lo escrito hasta el último «Guardado».
- Mátala mientras crea una copia de seguridad («Crear copia ahora») y mientras «Exportar todo» trabaja: al abrirla, las copias listadas se restauran bien, y en la carpeta de exportaciones solo hay exportaciones completas (y, como mucho, una carpeta oculta `.silex-export-…`, que se borra pasada una hora).

## 29. Arrastrar bloques (H9.2)
- Pasa el ratón por un bloque y arrastra su asa (⠿): el bloque queda marcado, su texto sigue al puntero y una línea azul marca dónde caerá. Suéltalo: se mueve. Cmd/Ctrl+Z lo devuelve.
- Un clic en el asa (sin arrastrar) sigue abriendo su menú.
- Selecciona varios bloques (Esc, Mayús+↑/↓) y arrastra el asa de uno de ellos: se mueven todos, en su orden.
- Arrastra y pulsa Esc antes de soltar: no se mueve nada. Soltar sobre el propio bloque tampoco mueve nada.
- En una página larga (la semilla), arrastra hasta el borde de arriba o de abajo: la página se desplaza sola.
- Arrastra una tarea marcada, una lista sangrada, un archivo y un enlace: llegan con su tipo, su sangría y su contenido. Cierra y abre la app: el orden se mantiene.
