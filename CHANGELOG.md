# Cambios

Todos los cambios de Silex que se notan al usarlo. El formato sigue [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/) y las versiones, [versionado semántico](https://semver.org/lang/es/): desde la 1.0.0, una versión nueva siempre abre la base de datos de una anterior (ADR 0014).

## [1.1.0] - 2026-11-04

Un editor más completo y bases de datos más potentes. Una base de la 1.0 se abre en la 1.1 y se actualiza sola, después de guardar una copia verificada; una base ya actualizada no se abre con la 1.0.

### Añadido
- **Formato en el texto:** negrita, cursiva, código, tachado y enlaces web, con atajos (Cmd/Ctrl+B, I, E, Mayús+X y Cmd/Ctrl+K sobre una selección), el submenú «Formato» del bloque y la paleta. Se conserva al copiar y pegar, y al exportar e importar Markdown.
- **Bloques nuevos:** separador (`---` o «/separador»), aviso con su icono («/aviso») y desplegable, que pliega lo que lleva dentro («/desplegable»). En Markdown, el aviso es `> [!NOTE]` y el desplegable, `<details>`.
- **Índice de la página:** «En esta página», un panel a la derecha con los títulos, que marca por dónde vas.
- **Mover bloques** a otra página y **convertirlos en una subpágina**, que deja un enlace en su sitio.
- **Pegar Markdown** crea bloques con su formato; Cmd/Ctrl+Mayús+V pega como texto plano. «Copiar como Markdown» en el menú de la página.
- **Icono de página:** un emoji delante del título, en la barra lateral, las pestañas, los enlaces y la búsqueda; también en Markdown.
- **Notas diarias:** «Hoy» en la barra lateral (Cmd/Ctrl+Mayús+D) abre o crea la nota del día dentro de «Diario», desde la plantilla que elijas en Ajustes.
- **Comparar versiones:** en el historial de una página, lo añadido, quitado y cambiado respecto a la actual, con las palabras cambiadas marcadas.
- **Bases de datos dentro de una página** («/base de datos»), con una tarjeta que muestra sus primeras filas.
- **Grafo de relaciones:** «Grafo» en la barra superior abre en una pestaña las páginas y sus enlaces, con búsqueda, zoom y el grafo de cada página.
- **Bases de datos:**
  - Reordenar filas y tarjetas arrastrándolas.
  - Propiedades nuevas: selección múltiple, URL (se abre con un clic), «Creado» y «Editado».
  - Cambiar el tipo de una propiedad, con una vista previa de qué pasa con los valores. Los que no encajan se guardan ocultos y vuelven si cambias otra vez al tipo anterior; Cmd/Ctrl+Z lo deshace.
  - Vista **calendario**: las filas en los días del mes según una propiedad de fecha; arrastrarlas cambia la fecha y «+» en un día crea una fila con esa fecha.
  - Vistas **lista** y **galería**, con la primera imagen de cada fila como portada.

### Límites conocidos
- Los paquetes **no están firmados**. En macOS, ábrelo la primera vez con clic derecho → Abrir (o en Ajustes del sistema → Privacidad y seguridad → «Abrir igualmente»). En Windows, SmartScreen avisa: «Más información» → «Ejecutar de todas formas».
- El `.dmg` es solo para Mac con Apple Silicon.
- La verificación manual se hace en macOS y Windows; en Linux solo pasan las pruebas automáticas y la instalación del `.deb` en la CI.
- «Hoy» en el calendario de una celda de fecha sale del reloj en UTC: entre medianoche y la diferencia con UTC puede marcar el día anterior. Las notas diarias y la vista calendario usan la hora local.
- El calendario solo pone las filas según propiedades de tipo Fecha, no según «Creado» o «Editado». La portada de la galería solo sale de imágenes PNG o JPEG.
- Un CSV abierto con doble clic en Excel con configuración regional española puede quedar en una sola columna (Excel espera `;`); «Datos → Desde texto/CSV» lo lee bien.
- En Windows, una exportación con páginas muy anidadas puede superar el límite clásico de 260 caracteres de ruta.

## [1.0.0] - 2026-10-22

Primera versión estable. Silex es un espacio de trabajo local para escribir y organizar notas: guarda todo en una base SQLite en tu equipo, sin cuentas, sin nube y sin telemetría.

### Añadido
- **Páginas:** anidadas en la barra lateral, con «Subir/Bajar» y «Mover a…», favoritos, renombrar, duplicar (con o sin subpáginas) y papelera con restaurar, borrar para siempre y vaciar.
- **Editor por bloques:** texto, títulos de tres niveles, listas con viñetas y numeradas, tareas, citas, código, archivos e imágenes, y enlaces a otras páginas. Menú `/`, sangría con Tab, selección de varios bloques, arrastrar bloques por su asa, y deshacer y rehacer de todo lo que se edita.
- **Buscar:** búsqueda global (Cmd/Ctrl+K) en títulos y contenido, sin distinguir mayúsculas ni acentos, con fragmentos y la ruta de cada página, y las páginas recientes cuando no se escribe nada. Paleta de comandos en la misma ventana. Buscar y reemplazar dentro de la página (Cmd/Ctrl+F; Cmd+Opción+F o Ctrl+H para reemplazar).
- **Bases de datos:** tablas con propiedades de texto, número, casilla, fecha (con calendario) y selección; filtros, orden, columnas ocultas y varias vistas por base, también de tablero (Kanban). Cada fila es una página. Seleccionar varias filas para enviarlas a la papelera, duplicarlas o poner un valor; deshacer en la tabla; cada base se abre en su última vista.
- **Varias páginas a la vez:** pestañas que se recuerdan al cerrar, y una página al lado, solo para leer.
- **Enlaces y referencias:** enlaces a páginas (también con `[[`) y la lista de páginas que enlazan a la abierta.
- **Historial de versiones** de cada página, con restaurar.
- **Plantillas** para crear páginas, con la fecha del día.
- **Adjuntos** dentro de la base (hasta 20 MB por archivo), con vista previa de imágenes y «Liberar espacio» para los que ya no se usan.
- **Importar y exportar:** importar archivos Markdown soltándolos en la ventana; exportar una página o todo el espacio de trabajo a Markdown (con adjuntos y enlaces entre páginas), y una base a CSV para Excel, Numbers o LibreOffice.
- **Datos seguros:** guardado automático, copias de seguridad automáticas y verificadas, restaurar una copia, aviso si la base está en una carpeta sincronizada en la nube, y permisos de los archivos solo para tu usuario.
- **Interfaz:** tema claro y oscuro (o el del sistema), tamaño de la interfaz de 50 % a 200 %, todo usable con el teclado y con nombres para lectores de pantalla.
- **Instaladores** para macOS (`.dmg`), Windows (instalador por usuario, sin administrador) y Linux (`.deb` y AppImage).
- **«Buscar actualizaciones»** en Ajustes y en la paleta: abre la página de versiones en el navegador. Silex no se conecta a internet.

### Límites conocidos
- Los paquetes **no están firmados**. En macOS, ábrelo la primera vez con clic derecho → Abrir (o en Ajustes del sistema → Privacidad y seguridad → «Abrir igualmente»). En Windows, SmartScreen avisa: «Más información» → «Ejecutar de todas formas».
- El `.dmg` es solo para Mac con Apple Silicon.
- La verificación manual se hizo en macOS y Windows; en Linux solo pasan las pruebas automáticas y la instalación del `.deb` en la CI.
- No hay formato dentro del texto (negrita, cursiva, enlaces web): llegará en la 1.1. Al importar Markdown, ese formato se conserva como texto.
- «Hoy» en el calendario de fechas sale del reloj en UTC: entre medianoche y la diferencia con UTC puede marcar el día anterior.
- Un CSV abierto con doble clic en Excel con configuración regional española puede quedar en una sola columna (Excel espera `;`); «Datos → Desde texto/CSV» lo lee bien.
- En Windows, una exportación con páginas muy anidadas puede superar el límite clásico de 260 caracteres de ruta.
