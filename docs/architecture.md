# Arquitectura

## Objetivo

Aifficator toma un XML exportado desde Rekordbox, permite elegir playlists, convierte los archivos seleccionados a AIFF y genera un nuevo XML importable. Los originales no se reemplazan: cada archivo convertido queda en una carpeta `converted` dentro del mismo directorio del archivo fuente.

Ejemplo:

```text
/Music/Artist/Track.flac
/Music/Artist/converted/Track.aiff
```

## Stack

- Rust para core, filesystem, validacion, conversion y export.
- Tauri 2 para app desktop nativa.
- Svelte + TypeScript para UI.
- SQLite para historial de imports, conversiones y exports.
- `ffmpeg`/`ffprobe` como herramientas de audio.
- `quick-xml` para leer exports Rekordbox.

## Pipeline

1. Importar XML.
2. Parsear `COLLECTION/TRACK`.
3. Parsear `PLAYLISTS/NODE` y referencias `TRACK Key`.
4. Validar archivos:
   - location faltante o invalida
   - archivo no encontrado
   - archivo sin permisos/metadata ilegible
   - formato no soportado
   - archivo ya convertido en `converted`
   - referencias de playlist a tracks inexistentes
5. Mostrar playlists, tracks e issues en UI.
6. Crear plan de conversion desde playlists seleccionadas.
7. Convertir con progreso en tiempo real.
8. Persistir resultado en SQLite.
9. Generar XML nuevo para importar en Rekordbox.

## Formatos

AIFF se considera formato final y se omite.

Formatos convertibles iniciales:

- FLAC
- MP3
- WAV/WAVE
- ALAC
- M4A
- AAC

La salida recomendada para compatibilidad maxima:

```sh
ffmpeg -i input.flac -map 0:a:0 -vn -c:a pcm_s16be output.aiff
```

Luego se puede agregar una opcion `pcm_s24be` si se quiere preservar 24-bit.

## Validacion y Reporte

El import no debe empezar conversiones de inmediato. Primero genera un reporte.

Severidades:

- `error`: bloquea conversion de ese track.
- `warning`: requiere atencion, pero no necesariamente bloquea.
- `info`: dato util, como AIFF existente o target ya creado.

Codigos iniciales:

- `missing_location`
- `invalid_location`
- `file_not_found`
- `cannot_read_file`
- `unsupported_format`
- `already_aiff`
- `target_already_exists`
- `duplicate_source`
- `missing_playlist_track`
- `target_collision`

## DB Propuesta

```sql
create table import_sessions (
  id text primary key,
  xml_path text not null,
  product_name text,
  product_version text,
  imported_at text not null
);

create table tracks (
  id text primary key,
  import_session_id text not null,
  rekordbox_track_id text not null,
  name text,
  artist text,
  kind text,
  source_path text,
  target_path text,
  validation_status text not null,
  foreign key (import_session_id) references import_sessions(id)
);

create table playlists (
  id text primary key,
  import_session_id text not null,
  path text not null,
  name text not null,
  node_type text,
  foreign key (import_session_id) references import_sessions(id)
);

create table playlist_tracks (
  playlist_id text not null,
  rekordbox_track_id text not null,
  position integer not null,
  primary key (playlist_id, position)
);

create table conversion_jobs (
  id text primary key,
  import_session_id text not null,
  status text not null,
  created_at text not null,
  finished_at text
);

create table conversion_items (
  id text primary key,
  job_id text not null,
  rekordbox_track_id text not null,
  source_path text not null,
  target_path text not null,
  status text not null,
  progress real not null default 0,
  error text,
  started_at text,
  finished_at text,
  foreign key (job_id) references conversion_jobs(id)
);

create table exports (
  id text primary key,
  import_session_id text not null,
  conversion_job_id text,
  export_path text not null,
  created_at text not null
);
```

## Siguiente Corte

El core ya separa import, validacion y plan. El siguiente paso es implementar el runner de conversion con eventos Tauri en tiempo real y despues el exporter XML que clona tracks convertidos preservando `TEMPO`, `POSITION_MARK` y el orden de playlists.

