# Seguridad y privacidad

La aplicación se diseña como **local-first**; no existe servicio remoto, autenticación, cifrado ni telemetría en el starter. **SQLite no está cifrada**, así que el sistema de archivos y el dispositivo deben considerarse el perímetro de seguridad actual.

No abras archivos o copias de seguridad de origen desconocido en una versión no auditada. Para vulnerabilidades, no publiques datos personales ni una base de datos real en issues. Antes de hacer público el proyecto, configura un canal privado de notificación de vulnerabilidades en GitHub Security Advisories.

Las políticas de permisos de `.claude/settings.json` son una capa de reducción de riesgo, no una sandbox ni una garantía contra todos los comandos destructivos. Revisa comandos y diffs propuestos por asistentes de código.
