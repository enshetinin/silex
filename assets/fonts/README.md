# Fuentes incluidas

Se incrustan en el binario (`include_bytes!` en `src/app/theme.rs`).

| Archivo | Origen | Versión | Licencia | SHA-256 |
|---|---|---|---|---|
| `Inter-Regular.ttf` | https://github.com/rsms/inter/releases/download/v4.1/Inter-4.1.zip (`extras/ttf/`) | 4.1 | SIL OFL 1.1 (`LICENSE-Inter-OFL.txt`) | `40d692fce188e4471e2b3cba937be967878f631ad3ebbbdcd587687c7ebe0c82` |
| `Inter-SemiBold.ttf` | ídem | 4.1 | SIL OFL 1.1 | `78a843fade9d4612a5567302fb595b56976eb5fcebf4fea5a5912d638bafcde3` |
| `Lucide.ttf` | https://cdn.jsdelivr.net/npm/lucide-static@1.49.0/font/lucide.ttf | 1.49.0 | ISC (`LICENSE-Lucide-ISC.txt`) | `d76dbb156455cc3ca20b5d387904c921893597dbad5a02728eb407173511ac8e` |

Los códigos de los iconos (`src/app/icons.rs`) salen de `lucide-static@1.49.0/font/lucide.css`; el comentario de cada constante es el nombre del icono en Lucide (https://lucide.dev/icons).
Hasta H10.1 se usaba Phosphor 2.1.1.

Para actualizar: descargar la nueva versión, comprobar la licencia, sustituir el archivo y actualizar esta tabla y los códigos.
