# Fuentes incluidas

Se incrustan en el binario (`include_bytes!` en `src/app/theme.rs`).

| Archivo | Origen | Versión | Licencia | SHA-256 |
|---|---|---|---|---|
| `Inter-Regular.ttf` | https://github.com/rsms/inter/releases/download/v4.1/Inter-4.1.zip (`extras/ttf/`) | 4.1 | SIL OFL 1.1 (`LICENSE-Inter-OFL.txt`) | `40d692fce188e4471e2b3cba937be967878f631ad3ebbbdcd587687c7ebe0c82` |
| `Inter-SemiBold.ttf` | ídem | 4.1 | SIL OFL 1.1 | `78a843fade9d4612a5567302fb595b56976eb5fcebf4fea5a5912d638bafcde3` |
| `Phosphor-Regular.ttf` | https://cdn.jsdelivr.net/npm/@phosphor-icons/web@2.1.1/src/regular/Phosphor.ttf | 2.1.1 | MIT (`LICENSE-Phosphor-MIT.txt`) | `06b91e022b7ee899a63efced879392a74f0bacbda54e4467e9f663220d173a10` |

Los códigos de los iconos (`src/app/icons.rs`) salen de `@phosphor-icons/web@2.1.1/src/regular/style.css`.
No se usa el crate `egui-phosphor`: su versión 0.14 depende de egui 0.36, y Silex usa egui 0.37.

Para actualizar: descargar la nueva versión, comprobar la licencia, sustituir el archivo y actualizar esta tabla y los códigos.
