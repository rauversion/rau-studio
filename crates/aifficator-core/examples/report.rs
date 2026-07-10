use aifficator_core::rekordbox::parse_rekordbox_xml_file;
use aifficator_core::validation::validate_library;
use std::env;

fn main() {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: cargo run -p aifficator-core --example report -- <rekordbox.xml>");
        std::process::exit(2);
    };

    let library = parse_rekordbox_xml_file(&path).expect("failed to parse Rekordbox XML");
    let report = validate_library(&library);

    println!("tracks_total={}", report.tracks_total);
    println!("playlists_total={}", report.playlists_total);
    println!("convert_candidates={}", report.convert_candidates);
    println!("already_aiff={}", report.already_aiff);
    println!("missing_files={}", report.missing_files);
    println!("unreadable_files={}", report.unreadable_files);
    println!("unsupported_tracks={}", report.unsupported_tracks);
    println!("duplicate_sources={}", report.duplicate_sources);
    println!(
        "playlist_reference_errors={}",
        report.playlist_reference_errors
    );
    println!("issues={}", report.issues.len());

    for (kind, count) in report.format_counts {
        println!("format[{kind}]={count}");
    }
}
