# ADR 0020 — Tablas embebidas, vistas enlazadas y relaciones

Implementación solicitada para la próxima versión de Silex.

Los bloques que enlazan una base de datos muestran una tabla editable. `/bbdd`
crea una base hija sin salir de la página; `/vista` elige una base existente.
Cada bloque conserva filtros, orden, columnas visibles y anchos independientes.
Las filas y propiedades pertenecen siempre a la base original. Quitar la vista
solo quita el bloque. La edición de celdas, nombres y creación de filas reutiliza
las operaciones y el historial de la tabla. La vista embebida ofrece botones de
deshacer y rehacer; los campos mantienen el deshacer de texto.

Los anchos se guardan en `ViewConfig.widths`, por identificador de propiedad
(`name` para Nombre), entre 80 y 1200 puntos. Se ajustan arrastrando el borde
derecho de la cabecera o con las flechas cuando el separador tiene el foco.

La migración 0016 reconstruye el CHECK de propiedades y añade:

- `db_linked_views`: configuración por bloque, con referencia a la base fuente.
  Se conserva cuando desaparece el bloque para permitir deshacer su eliminación.
- `db_relations`: propiedad y base de destino.
- `db_relation_values`: referencias de una fila a una o varias filas de destino.

La migración usa la transacción, copia previa y comprobación de claves foráneas
existentes. Cada escritura de relación valida la base y las filas de destino.
Los nombres se leen de las páginas originales. La papelera oculta los vínculos
sin borrarlos; restaurar devuelve los vínculos. El borrado definitivo usa las
cascadas de las claves foráneas. Duplicar una página conserva sus configuraciones
de vista; duplicar filas y bases conserva sus relaciones con los destinos originales.

Las relaciones son direccionales; esta entrega no añade fórmulas ni agregaciones.
El destino se elige al crear la propiedad y no se permite convertir una relación
a otro tipo. La vista embebida es de tabla; las demás vistas se abren en la base.
