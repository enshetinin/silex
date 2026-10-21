# PRD — Silex

## Visión
Silex es un workspace local-first para escribir, organizar y recuperar información personal y profesional con una experiencia inspirada en Notion, sin servicio remoto obligatorio.

## Usuario objetivo
Personas que quieren documentos y wikis personales rápidos, control sobre los datos, uso sin conexión y aplicaciones de escritorio instalables.

## Principios
1. Offline por defecto. Datos del usuario bajo su control.
2. Menos funcionalidades, más fiabilidad: jamás comprometer persistencia por estética.
3. Edición fluida con teclado y acciones reversibles.
4. Libertad de exportación y restauración.
5. Sin telemetría, registro ni dependencia de cuentas.

## Funcionalidades por prioridad
### P0 — núcleo utilizable
- Páginas: crear, renombrar, abrir, organización jerárquica, favoritos.
- Editor de bloques: párrafo, H1/H2/H3, listas, tareas; Enter añade bloque, Tab indenta, Backspace combina, comandos `/` básicos.
- Guardado automático fiable y feedback de errores.
- Búsqueda global FTS5 en título y texto.
- Papelera y recuperación, exportación Markdown por página.
- Atajos de teclado, estado vacío y enfoque accesible.
- Copia de seguridad y restauración verificadas.

### P1 — producto sólido
- Arrastrar/reordenar bloques, undo/redo, historial básico.
- Adjuntos locales y enlaces internos, backlinks.
- Importación Markdown, exportación por espacio.
- Vista dividida, pestañas, temas claro/oscuro.
- Actualizaciones y empaquetado firmado por sistema operativo.

### P2 — avanzado
- Bases de datos con propiedades tipadas, filtros y vistas tabla/Kanban.
- Plantillas, tablas embebidas, comandos rápidos extensibles.
- Solo tras ADR: sincronización opcional, cifrado, colaboración o IA.

## Alcance explícito del starter
La versión incluida en este repositorio es una **base técnica**, no la entrega completa de P0. Implementa únicamente páginas planas y tipos de bloque simples, búsqueda y persistencia. El roadmap representa el orden de entrega; no promete funciones que aún no existen.

## No objetivos iniciales
- Paridad total con Notion.
- Autenticación cloud, colaboración y multiusuario.
- Editor HTML o frontend React/WebView.
- Plugins ejecutando código arbitrario.
- Compatibilidad mobile en primera fase.

## Requisitos no funcionales
- No pérdida de cambios en cierres normales; tests de crash/power-loss antes de afirmar garantía fuerte.
- Migración reversible con copias seguras; datos existentes deben sobrevivir actualizaciones.
- Instalación sin servidor de base de datos.
- Arranque, navegación y edición perceptiblemente responsivos. Objetivos fijados con medidas (H9.3b, `docs/ARCHITECTURE.md`, «Rendimiento»): ventana lista en < 300 ms en caliente y < 1 s en frío, abrir una página en < 16 ms (un frame) y un frame en reposo en < 4 ms de CPU, con 5000 páginas.
- Accesibilidad: navegación teclado, lectura de etiquetas y foco; auditar antes de v1.0.

## Definición de terminado de una historia
- Código y tests integrados, `fmt`, `clippy`, `test` y `check` correctos.
- Verificación manual para cambios visuales.
- Sin pérdida de información en errores o transacciones parcialmente fallidas.
- Documentación y roadmap reflejan solo lo implementado.
