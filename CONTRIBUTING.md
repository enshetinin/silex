# Contribuir

1. Ramas:
   - `main`: solo recibe PR desde `develop` cuando un hito está completo y verificado.
   - `develop`: integración. Recibe una PR por historia.
   - `feature/<historia>` (p. ej. `feature/h2-4-favoritos`): sale de `develop`, contiene una sola historia acotada de `docs/ROADMAP.md` y vuelve a `develop` por PR.
   - No se hace commit directo en `main`.
2. Abre un issue o explica la motivación en la PR si afecta datos, editor o arquitectura.
3. Añade pruebas, especialmente para datos persistentes y migraciones.
4. Ejecuta las comprobaciones de `CLAUDE.md` y describe cualquier entorno donde no pudiste ejecutarlas.
5. Documenta nuevos comportamientos y actualiza el roadmap solo cuando estén verificados.
6. Evita cambios incompatibles del esquema y dependencias innecesarias.

Mantén tus datos personales y archivos SQLite fuera de las PRs.
