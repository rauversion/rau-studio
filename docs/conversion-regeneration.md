# Regeneración de archivos convertidos

Estado: implementada en File Conversion. La publicación y las pruebas de audio se
verificaron en macOS; queda pendiente la ejecución nativa de esas pruebas en Windows
y Linux.

## Objetivo y alcance

Permitir volver a generar AIFF desde los archivos originales para aplicar el
comportamiento actual del conversor, incluida la conservación de metadatos.
La primera entrega corresponde a **File Conversion**, con acciones por archivo
y por selección, disponibles también al abrir importaciones anteriores.

El diseño permite reutilizar el servicio desde Rekordbox Convert posteriormente;
su interfaz y el exportador XML quedan fuera de esta primera entrega.

## Situación actual verificada

- `src/FileConversionPage.tsx` usa `local_conversion_convert_items` tanto por fila
  como por selección. Los elementos convertidos pueden volver a enviarse.
- `src-tauri/src/local_conversion.rs` reutiliza los AIFF existentes y completa
  etiquetas vacías desde el original. Conserva las etiquetas no vacías del AIFF,
  su audio y sus streams de carátula. Verifica un temporal antes de sustituirlo.
- `crates/aifficator-core/src/conversion.rs` genera audio AIFF PCM de 16 bits,
  44,1 kHz y dos canales, con etiquetas de texto ID3v2.3. Las conversiones nuevas
  no copian carátulas y por defecto no sobrescriben archivos.
- Rekordbox Convert, en `src-tauri/src/lib.rs`, simplemente reutiliza una salida
  existente; no ejecuta la recuperación de metadatos del importador local.
- Playlist Library mantiene una copia indexada de los metadatos. Actualmente se
  actualiza al volver a agregar los archivos a una playlist.

Por tanto, volver a pulsar **Convertir** ya resuelve etiquetas faltantes en el
importador local, pero no reconstruye el audio ni actualiza etiquetas con valor.

## Comportamiento de las acciones

| Acción | Sin salida previa | Con salida previa |
| --- | --- | --- |
| Convertir | Crea el AIFF | Reutiliza el audio y completa metadatos faltantes |
| Regenerar | Recrea la salida si desapareció | Genera otro AIFF desde el original y sustituye la salida tras verificarlo |

La regeneración mantiene el nombre, la ruta, el identificador del elemento y su
pertenencia a grupos. Nunca usa el AIFF convertido como fuente del nuevo audio.
Un original AIFF/AIF sigue siendo un archivo final y no se regenera sobre sí mismo.

### Interfaz

- Añadir **Regenerar AIFF** en las acciones de cada archivo con salida existente
  o una conversión anterior registrada. Mantener **Convertir** accesible para la
  recuperación rápida de etiquetas.
- Añadir **Regenerar seleccionados (N)** junto a la conversión por selección.
  `N` representa los archivos elegibles de la selección visible.
- Ayuda breve: «Vuelve a crear el AIFF desde el original. Actualiza los metadatos
  presentes en el original y conserva la misma ubicación».
- Deshabilitar cuando falta el original, es AIFF/AIF o el destino está procesándose.
  Informar los elementos excluidos de una selección mixta y sus motivos.
- Mostrar las fases «Regenerando», «Verificando» y el resultado por archivo.
  Reutilizar la terminal y el selector de concurrencia actuales.
- Ante un fallo previo al reemplazo: «No se pudo regenerar; el AIFF anterior
  sigue disponible». La reproducción depende de la disponibilidad real del
  archivo, no del éxito del último intento.

### Política de metadatos propuesta

La diferencia con **Convertir** debe ser explícita:

1. En **Regenerar**, una etiqueta no vacía del original tiene prioridad sobre la
   misma etiqueta del AIFF anterior. Esto permite aplicar correcciones al original.
2. Conservar las etiquetas de usuario presentes únicamente en el AIFF anterior;
   un campo vacío del original no las borra.
3. Excluir etiquetas técnicas del contenedor o del encoder. Normalizar nombres y
   alias de campos antes de combinar y verificar los valores.
4. Conservar una carátula que ya tenga el AIFF anterior. Si no puede preservarse,
   informar el fallo antes de sustituirlo. Importar carátulas nuevas desde el
   original sería una mejora separada del perfil actual.
5. Los metadatos guardados solamente en una aplicación externa no están disponibles
   para este proceso. No prometer sincronización de cues, grids o etiquetas de Rekordbox.

Ejemplo: si el original cambia de título, regenerar actualiza el título del AIFF;
un comentario añadido solamente al AIFF se conserva. **Convertir** mantiene el
título existente y únicamente completa campos faltantes.

## Contrato y responsabilidades

Extender el comando existente con `mode: convert | regenerate`, opcional y con
`convert` por defecto. Evitar un booleano genérico de sobrescritura: el modo
selecciona una operación completa con su política de metadatos y publicación.

- **Frontend:** selección, acciones, mensajes y actualización de resultados.
- **Coordinador local:** resuelve los IDs desde SQLite, valida elegibilidad,
  deduplica destinos, reserva trabajo y administra concurrencia e historial.
- **Servicio de archivos AIFF:** prepara el temporal, ejecuta el perfil compartido,
  combina metadatos, valida y publica el resultado. Extraerlo a un módulo enfocado,
  reutilizando la recuperación actual sin duplicar su lógica.
- **Core:** continúa definiendo el perfil y los argumentos de conversión. FFmpeg
  escribe a un temporal único con protección contra sobrescritura.

Conservar los estados `queued`, `running`, `converted`, `already_converted`,
`already_aiff` y `failed`. Una regeneración exitosa termina en `converted`;
la operación y su resultado se distinguen con datos adicionales, no con estados
paralelos para cada fase.

Añadir `operation_id`, `mode` y `phase` a los eventos existentes. Persistir el tipo
de última operación en el elemento y el detalle en `local_conversion_events`.
Esto permite describir correctamente el resultado al reabrir un grupo e ignorar
eventos atrasados de un intento anterior. Añadir un contador `regenerated_total`,
separado de `converted_total`, en el resultado del lote.

## Escritura y concurrencia

Secuencia por archivo:

1. Resolver y reservar la ruta de destino en el backend; volver a validar fuente
   y destino aunque la interfaz los haya considerado disponibles.
2. Rechazar fuente y destino que identifiquen el mismo archivo, destinos enlazados
   simbólicamente y colisiones conocidas. Dos originales como `Tema.mp3` y
   `Tema.flac` no pueden regenerar silenciosamente el mismo `converted/Tema.aiff`.
   Revisar el lote y las referencias registradas; bloquear los elementos ambiguos.
3. Capturar identidad, tamaño y fecha de modificación de fuente y salida; leer
   los metadatos necesarios y preparar `.rau-regenerate-<uuid>.aiff` en la misma
   carpeta de destino.
4. Generar audio desde el original y aplicar la política de etiquetas y carátula.
5. Verificar el temporal: archivo no vacío, contenedor AIFF, codec, canales,
   frecuencia, duración coherente y etiquetas esperadas. Comprobar decodificación
   completa sin errores antes de reemplazar una salida útil.
6. Comprobar nuevamente que fuente y salida no cambiaron durante el trabajo.
   Estas comprobaciones detectan cambios, pero no equivalen a un bloqueo de
   escritura frente a aplicaciones externas.
7. Publicar mediante reemplazo atómico en el mismo sistema de archivos. Preservar
   permisos del destino. Usar la primitiva apropiada de cada plataforma, sin
   borrar primero el AIFF anterior. Si inicialmente no había salida, publicar sin
   reemplazar un archivo que haya aparecido durante el procesamiento.
8. Registrar éxito y actualizar las vistas. Ante fallo previo a la publicación,
   eliminar solamente el temporal y conservar la salida anterior.

La reserva debe cubrir también **Convertir**, incluida su reparación de etiquetas,
y cualquier escritor del conversor Rekordbox que use ese destino. No basta con
`busy` en React: la protección debe abarcar peticiones simultáneas. Aplicar el
límite actual de 1 a 4 trabajos en el coordinador compartido de conversiones.

El reemplazo del archivo y la actualización de SQLite no forman una sola
transacción. Registrar una fase de publicación con la identidad del temporal
validado; si la app se interrumpe, reconciliar ese intento antes de permitir otro.
Un fallo de historial posterior al reemplazo debe indicar «AIFF regenerado;
historial pendiente de actualizar», sin afirmar que se conservó la versión vieja.
Limpiar únicamente temporales identificados como propios e inactivos.

## Playlist Library y reproducción

Después de publicar, actualizar los registros existentes vinculados al elemento
o a la ruta final, resolviendo también sus alias. Conservar IDs, playlists,
calificaciones y atributos gestionados por el catálogo; actualizar únicamente
los campos derivados del archivo y los índices de búsqueda afectados.

Extraer una operación de actualización enfocada: no reutilizar sin cambios el
alta de playlists, que puede crear registros y reemplazar atributos completos.
Si falla esta actualización, mantener el AIFF válido, informar el pendiente y
permitir reintentar el refresco sin volver a convertir audio.

Invalidar las vistas y la caché de reproducción para que el siguiente play abra
la nueva versión aunque conserve la ruta. Detener la reproducción local del
destino afectado antes de publicarlo y evitar nuevas aperturas durante ese paso.
Un archivo bloqueado por otra aplicación produce un error conservando la salida.
Conservar una ruta no garantiza que Rekordbox relea sus metadatos automáticamente.

## Entrega y criterios de aceptación

1. Servicio de regeneración, validación, publicación y exclusión por destino.
2. Contrato Tauri, persistencia del intento y eventos compatibles.
3. Botones individuales y por lote, traducciones y resultados comprensibles.
4. Refresco del catálogo y reproducción; actualización de documentación vigente.

Verificación requerida al implementar:

- Regenerar una conversión antigua produce el perfil actual y los metadatos
  esperados, conserva la ruta y no cambia un byte del original.
- Las etiquetas compartidas siguen la prioridad del original; las exclusivas del
  destino y su carátula se conservan. **Convertir** mantiene su política anterior.
- Error de FFmpeg, archivo inválido, disco lleno o fallo de publicación deja el
  AIFF anterior idéntico y disponible.
- Dobles clics, lotes simultáneos, colisiones de nombre y enlaces no causan
  escritores concurrentes ni modificaciones del original.
- Original ausente, original AIFF, salida desaparecida y selección mixta tienen
  resultados explícitos; un error no impide procesar otros archivos elegibles.
- Una interrupción antes o después del reemplazo se reconcilia sin confundir el
  resultado del archivo con el estado del historial o del catálogo.
- Las playlists mantienen sus referencias y no se duplican tracks; el siguiente
  play utiliza la versión nueva.
- Verificar publicación segura en macOS, Windows y Linux. Ejecutar las pruebas
  de audio con FFmpeg/ffprobe reales, incluidas las actuales marcadas como ignoradas.

No se proponen detección automática de archivos antiguos, regeneración en segundo
plano ni versiones permanentes de cada AIFF en esta entrega.
