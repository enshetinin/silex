# Cambios

Todos los cambios de Silex que se notan al usarlo. El formato sigue [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/) y las versiones, [versionado semántico](https://semver.org/lang/es/): desde la 1.0.0, una versión nueva siempre abre la base de datos de una anterior (ADR 0014).

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
