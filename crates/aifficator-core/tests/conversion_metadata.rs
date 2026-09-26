use aifficator_core::conversion::{ffmpeg_args, ConversionSettings};
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

fn run(command: &mut Command) -> String {
    let output = command.output().expect("run FFmpeg tool");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("UTF-8 output")
}

fn tags(path: &Path) -> BTreeMap<String, String> {
    run(Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format_tags",
            "-of",
            "default=noprint_wrappers=1",
        ])
        .arg(path))
    .lines()
    .filter_map(|line| line.strip_prefix("TAG:")?.split_once('='))
    .map(|(key, value)| (key.to_ascii_lowercase(), value.to_string()))
    .collect()
}

#[test]
#[ignore = "requires ffmpeg and ffprobe on PATH"]
fn aiff_conversion_preserves_track_metadata_from_supported_containers() {
    let directory = tempfile::tempdir().expect("temporary fixtures");
    let metadata = [
        ("title", "Canción de prueba"),
        ("artist", "Artista de la canción"),
        ("album_artist", "Artista del álbum"),
        ("album", "Álbum de prueba"),
        ("genre", "House"),
        ("date", "2024"),
        ("track", "3/10"),
        ("disc", "1/2"),
        ("comment", "Comentario de prueba"),
        ("composer", "Compositor"),
        ("bpm", "125"),
        ("initial_key", "Am"),
    ];
    for (extension, codec) in [
        ("flac", "flac"),
        ("mp3", "libmp3lame"),
        ("m4a", "alac"),
        ("wav", "pcm_s24le"),
    ] {
        let source = directory
            .path()
            .join(format!("Canción original.{extension}"));
        let target = directory.path().join(format!("Converted {extension}.aiff"));
        let mut fixture = Command::new("ffmpeg");
        fixture.args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000",
            "-t",
            "0.1",
            "-c:a",
            codec,
        ]);
        for (key, value) in metadata {
            fixture.args(["-metadata", &format!("{key}={value}")]);
        }
        run(fixture.arg(&source));
        let original_tags = tags(&source);
        for key in [
            "title", "artist", "album", "genre", "date", "track", "comment",
        ] {
            assert!(
                original_tags.contains_key(key),
                "{extension} fixture missing {key}"
            );
        }

        run(Command::new("ffmpeg").args(ffmpeg_args(
            &source,
            &target,
            &ConversionSettings::default(),
        )));
        let converted_tags = tags(&target);
        for (key, _) in metadata {
            if let Some(expected) = original_tags.get(key) {
                assert_eq!(
                    converted_tags.get(key),
                    Some(expected),
                    "{extension}: lost {key}"
                );
            }
        }
        let audio = run(Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "a:0",
                "-show_entries",
                "stream=codec_name,sample_rate,channels",
                "-of",
                "default=noprint_wrappers=1",
            ])
            .arg(&target));
        assert!(audio.contains("codec_name=pcm_s16be"));
        assert!(audio.contains("sample_rate=44100"));
        assert!(audio.contains("channels=2"));

        let before = b"Existing output must not be overwritten";
        std::fs::write(&target, before).expect("existing output fixture");
        Command::new("ffmpeg")
            .args(ffmpeg_args(
                &source,
                &target,
                &ConversionSettings::default(),
            ))
            .output()
            .expect("attempt repeated conversion");
        assert_eq!(
            std::fs::read(&target).unwrap(),
            before,
            "must not overwrite an existing file"
        );
    }
}
