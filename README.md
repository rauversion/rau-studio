# Aifficator

Aplicacion nativa para importar exports XML de Rekordbox, seleccionar playlists, convertir audio a AIFF en carpetas `converted/` junto a los originales, y generar exports importables de vuelta en Rekordbox.

## Estado

Base inicial:

- parser de XML Rekordbox
- listado de playlists y tracks
- validacion de archivos no encontrados, ubicaciones invalidas, formatos no soportados y targets ya convertidos
- planificador de conversion por playlists seleccionadas
- comandos Tauri para conectar la UI con el core
- UI inicial para importar XML, revisar playlists, problemas y plan de conversion

## Requisitos locales

- Rust via asdf: `rust stable`
- Node.js y npm
- `ffmpeg`
- `ffprobe`

## Comandos

```sh
npm install
npm run tauri:dev
```

Para probar solo el core Rust:

```sh
cargo test -p aifficator-core
```

