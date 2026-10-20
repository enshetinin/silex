# ADR-0012 — Evaluación de cifrado y sincronización

**Estado:** aceptada (2026-10-20), con las decisiones del propietario del final.

## Contexto
El roadmap (Hito 7) pide «evaluar cifrado y sincronización únicamente con ADR, threat model y pruebas», y el PRD los deja para después de un ADR. Este documento es esa evaluación: qué protege Silex hoy, frente a quién, qué opciones hay y qué costarían. No añade código por sí mismo.

### Qué datos hay y dónde
| Dato | Dónde | En claro |
|---|---|---|
| Páginas, bloques, bases, versiones, ajustes y adjuntos | `silex.sqlite3` en la carpeta de datos del sistema (`ProjectDirs::data_local_dir`) | Sí |
| Copias de seguridad (10, verificadas) | `backups/` junto a la base | Sí |
| Copias de adjuntos abiertos con su app | caché de la app, `abiertos/` (se vacía al **arrancar**, no al cerrar) | Sí |
| «Guardar una copia» y exportaciones Markdown | `Documentos/Silex/Exportaciones` | Sí, a propósito: son para el usuario |
| Temporales de exportación | junto al destino, borrados al terminar o al fallar | Sí, por segundos |

Silex no fija permisos propios: los archivos heredan los del sistema (en Linux, según el `umask` y los permisos de la carpeta personal, otros usuarios del equipo podrían leerlos). No hay red, telemetría ni registros con contenido.

## Modelo de amenazas
| Quién | Qué puede hacer | Qué le frena hoy | Qué le frenaría el cifrado en la app |
|---|---|---|---|
| **Robo o pérdida del equipo apagado** | Leer el disco | El cifrado de disco del sistema (FileVault, BitLocker, LUKS), si está activado | Sí, también sin cifrado de disco, si la clave no está guardada en el equipo |
| **Otro usuario del mismo equipo** | Leer archivos con permisos laxos | Los permisos de la carpeta personal (variable en Linux) | Sí, pero bastan permisos estrictos (más barato) |
| **Alguien con la sesión abierta, o malware del usuario** | Todo lo que el usuario puede: leer la base, la memoria, el teclado | Nada | **No**: con la sesión abierta la base está descifrada y la clave en memoria |
| **El proveedor de una nube** (si el usuario pone la base o las copias en una carpeta sincronizada) | Leer los archivos subidos | Nada; además, sincronizar un SQLite abierto en modo WAL puede corromperlo | Sí para leerlos; la corrupción sigue |
| **Quien recibe una exportación o una copia en un USB** | Leerla | Nada | Solo las copias de seguridad (las exportaciones son en claro a propósito) |

Conclusión: el cifrado de disco del sistema ya cubre el caso más común (equipo perdido o robado). El cifrado dentro de Silex añade protección solo sin cifrado de disco, o con copias que salen del equipo. Frente a lo que el usuario puede hacer con su sesión abierta, no protege nada.

## Alternativas: cifrado
1. **Confiar en el cifrado de disco y endurecer lo que Silex controla.** Permisos solo para el usuario en la carpeta de datos y sus archivos (Unix), vaciar las copias de adjuntos abiertos también al cerrar, y explicar en la app dónde están los datos y por qué conviene el cifrado de disco. Barato, sin riesgo de perder datos y sin dependencias.
2. **SQLCipher opcional** (cifrar la base con una contraseña que el usuario activa). rusqlite lo soporta (`bundled-sqlcipher`, o `bundled-sqlcipher-vendored-openssl` para no depender del OpenSSL del sistema). Costes:
   - **Compilación:** compila OpenSSL desde el código fuente en los tres sistemas (en Windows necesita Perl), con más crates en `Cargo.lock` y más tiempo de CI y de empaquetado.
   - **Claves:** una contraseña olvidada es una base perdida. Guardarla en el llavero del sistema (otro crate, `keyring`) protege menos, porque el llavero se abre con la sesión.
   - **Todo lo demás:** las copias de seguridad, la verificación de copias, la restauración, «Liberar espacio» (`VACUUM`), las migraciones y el hilo de cada worker (cada uno abre su conexión y necesitaría la clave) tendrían que conocer la clave. La migración de una base en claro se hace con `sqlcipher_export`.
   - **Lo que sigue en claro:** las copias de adjuntos abiertos (las lee otra app) y las exportaciones.
   - **Rendimiento:** SQLCipher documenta una sobrecarga baja, pero habría que medirla aquí con la semilla (búsqueda, guardado, copias).
3. **Cifrado obligatorio.** Lo mismo que la 2, pero pidiendo contraseña en cada arranque a todos los usuarios. Mucho riesgo de pérdida para el caso común, que ya cubre el disco.
4. **Cifrar campos sueltos en la app** (texto de bloques, adjuntos). Rompe la búsqueda FTS5, que lee el texto en claro, y deja a la vista la estructura y los títulos. Descartada.

## Alternativas: sincronización
1. **Sin sincronización, y proteger contra el error más probable**, que es poner la base en una carpeta de iCloud Drive, Dropbox, OneDrive o Google Drive. Un SQLite en uso (WAL, `-shm`) sincronizado por archivos puede corromperse o duplicarse. Silex avisaría al arrancar si la base o sus copias están en una de esas carpetas. Para pasar los datos a otro equipo ya sirven las copias de seguridad (copiar una y restaurarla).
2. **Sincronización por carpeta con registro de cambios.** Cada equipo escribe sus cambios en archivos propios dentro de una carpeta compartida (sin abrir la base de otro). Cada equipo aplica los de los demás con marcas de tiempo híbridas: gana el último por bloque y por celda, y los adjuntos se identifican por su contenido. Es local-first y no necesita servidor. Pero es un proyecto en sí mismo: borrar contra editar, mover páginas, el orden de los bloques, versiones de esquema distintas entre equipos y pruebas de convergencia. Necesitaría su propio hito y ADR.
3. **Servidor propio o de terceros.** Choca con los principios del PRD (sin backend en la nube, sin telemetría por defecto). Descartada.

## Decisión
- **Cifrado: alternativa 1 ahora.** SQLCipher opcional (2) queda como hito futuro si el propietario lo pide, con su ADR, sus medidas y sus pruebas de pérdida de clave.
- **Sincronización: alternativa 1 ahora.** La 2 queda como hito futuro con ADR propio.
- **Una historia corta, H7.12 «Datos locales más protegidos»:**
  - en Unix, la carpeta de datos y la de copias con permisos `0700` y la base y las copias con `0600`, comprobados al arrancar y corregidos si son más abiertos;
  - vaciar `abiertos/` también al cerrar la app, además de al arrancar;
  - aviso al arrancar si la base o sus copias están en una carpeta sincronizada conocida (con «No volver a avisar»);
  - una sección «Privacidad» en Ajustes: dónde están los datos, que todo es local, y que el cifrado de disco del sistema es lo que protege un equipo perdido;
  - tests de cada punto (permisos en Linux y macOS en la CI; la detección de carpetas como función pura).

## Consecuencias
- **Sin dependencias nuevas** ni cambios de esquema.
- **Lo que no se protege**, y se dice: un equipo sin cifrado de disco, y lo que el usuario (o algo con su sesión) haga con la sesión abierta.
- **Las exportaciones y «Guardar una copia» siguen en claro:** son para que el usuario las lleve donde quiera.
- **El cierre del punto del roadmap:** con este ADR aceptado (y H7.12, si se decide), «Evaluar cifrado y sincronización» queda hecho. Cifrar o sincronizar de verdad serían hitos nuevos.

## Decisiones del propietario (2026-10-20)
1. Cifrado: confiar en el cifrado de disco del sistema y endurecer lo que controla Silex. SQLCipher opcional, solo si se pide más adelante, con su ADR.
2. Sincronización: ninguna por ahora, con aviso si la base o sus copias están en una carpeta sincronizada. La sincronización por carpeta, solo si se pide más adelante, con su ADR.
3. H7.12 «Datos locales más protegidos» se hace ahora.
