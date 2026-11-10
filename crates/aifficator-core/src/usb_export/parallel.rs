//! CPU preparation and a bounded queue feeding a single USB writer.
use super::transfer::PreparedTrack;
use super::*;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc,
};
use std::time::Instant;

enum Event {
    Started(usize),
    Activity(usize, String, String),
    Prepared(usize, u64),
    Transfer(usize, String, String, PublicationProgress),
    Uploaded(usize, ExportedTrack),
    Failed(usize, UsbExportError),
}

pub(super) fn run(
    tracks: &[ExportTrack],
    requested_workers: usize,
    reused: usize,
    total_files: usize,
    destination: &Path,
    task: impl Fn(&ExportTrack, &dyn Fn(&str, &str)) -> Result<PreparedTrack> + Sync,
    progress: &mut impl FnMut(ExportProgress),
) -> Result<Vec<ExportedTrack>> {
    let workers = requested_workers
        .max(1)
        .min(transfer::available_workers())
        .min(tracks.len());
    let mut preparation = PreparationProgress {
        completed_tracks: 0,
        total_tracks: tracks.len(),
        reused_tracks: reused,
        workers,
        active_tracks: Vec::new(),
    };
    let mut publication = PublicationProgress {
        completed_files: 0,
        total_files,
        completed_bytes: 0,
        total_bytes: 0,
        current_file_bytes: 0,
        current_file_processed_bytes: 0,
        total_bytes_known: tracks.is_empty(),
        current_track: None,
        current_file: None,
        phase: "waiting".into(),
        queued_tracks: 0,
    };
    let mut initial = ExportProgress::event(
        ExportStage::Audio,
        0,
        tracks.len(),
        None,
        "pipeline_start",
        "Preparación local y copia USB independientes; solo se copian pistas nuevas o modificadas",
    );
    initial.preparation = Some(preparation.clone());
    initial.publication = Some(publication.clone());
    progress(initial);
    if tracks.is_empty() {
        return Ok(Vec::new());
    }
    let next = AtomicUsize::new(0);
    let failed = AtomicBool::new(false);
    let (events, receive) = mpsc::sync_channel::<Event>(workers * 8 + 8);
    // At most two ready tracks wait behind the active USB transfer.
    let (queue, ready) = mpsc::sync_channel::<(usize, PreparedTrack)>(2);
    let mut records = BTreeMap::new();
    let mut active = BTreeMap::new();
    let mut first_error = None;
    let mut known_bytes = 0u64;
    let mut uploaded = 0usize;
    std::thread::scope(|scope| {
        let transfer_events = events.clone();
        let failure = &failed;
        let initial_publication = publication.clone();
        scope.spawn(move || {
            let mut status = initial_publication;
            'upload: for (index, mut prepared) in ready {
                if failure.load(Ordering::Relaxed) {
                    break;
                }
                for file in &prepared.files {
                    if failure.load(Ordering::Relaxed) {
                        break 'upload;
                    }
                    status.current_file_bytes = file.bytes;
                    status.current_file_processed_bytes = 0;
                    status.current_track = Some(prepared.record.track.title.clone());
                    status.current_file = Some(file.relative.clone());
                    let _ = transfer_events.send(Event::Transfer(
                        index,
                        "publish_media".into(),
                        format!("{} · {} bytes", file.relative, file.bytes),
                        status.clone(),
                    ));
                    let started = Instant::now();
                    let result = transfer::write_file(file, |phase, processed| {
                        status.current_file_processed_bytes = processed;
                        let _ = transfer_events.send(Event::Transfer(
                            index,
                            phase.into(),
                            file.relative.clone(),
                            status.clone(),
                        ));
                    });
                    if let Err(error) = result {
                        failure.store(true, Ordering::Relaxed);
                        let _ = transfer_events.send(Event::Failed(
                            index,
                            invalid(format!(
                                "«{}», {}: {error}",
                                prepared.record.track.title, file.relative
                            )),
                        ));
                        break 'upload;
                    }
                    status.completed_files += 1;
                    status.completed_bytes += file.bytes;
                    status.current_file_bytes = 0;
                    status.current_file_processed_bytes = 0;
                    let _ = transfer_events.send(Event::Transfer(
                        index,
                        "publish_media_done".into(),
                        format!(
                            "{} · {:.1} s · copiado y verificado",
                            file.relative,
                            started.elapsed().as_secs_f64()
                        ),
                        status.clone(),
                    ));
                }
                if let Err(error) = transfer::capture_signatures(destination, &mut prepared.record)
                {
                    failure.store(true, Ordering::Relaxed);
                    let _ = transfer_events.send(Event::Failed(
                        index,
                        invalid(format!("«{}»: {error}", prepared.record.track.title)),
                    ));
                    break;
                }
                let _ = transfer_events.send(Event::Uploaded(index, prepared.record));
            }
        });
        for _ in 0..workers {
            let sender = events.clone();
            let queue = queue.clone();
            let next = &next;
            let failed = &failed;
            let task = &task;
            scope.spawn(move || {
                while !failed.load(Ordering::Relaxed) {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(track) = tracks.get(index) else {
                        break;
                    };
                    let _ = sender.send(Event::Started(index));
                    let notify = |action: &str, detail: &str| {
                        let _ = sender.send(Event::Activity(index, action.into(), detail.into()));
                    };
                    let result = task(track, &notify).and_then(|prepared| {
                        let bytes = prepared.bytes()?;
                        Ok((prepared, bytes))
                    });
                    match result {
                        Ok((prepared, bytes)) => {
                            if failed.load(Ordering::Relaxed) {
                                break;
                            }
                            if sender.send(Event::Prepared(index, bytes)).is_err()
                                || queue.send((index, prepared)).is_err()
                            {
                                break;
                            }
                        }
                        Err(error) => {
                            failed.store(true, Ordering::Relaxed);
                            let _ = sender.send(Event::Failed(
                                index,
                                invalid(format!("«{}»: {error}", track.title)),
                            ));
                            break;
                        }
                    }
                }
            });
        }
        drop(queue);
        drop(events);
        for event in receive {
            let (index, stage, action, detail) = match event {
                Event::Started(index) => {
                    active.insert(index, tracks[index].title.clone());
                    (
                        index,
                        ExportStage::Audio,
                        "prepare_start".into(),
                        "Preparando audio y análisis en el ordenador".into(),
                    )
                }
                Event::Activity(index, action, detail) => (
                    index,
                    if action == "analysis" {
                        ExportStage::Analysis
                    } else {
                        ExportStage::Audio
                    },
                    action,
                    detail,
                ),
                Event::Prepared(index, bytes) => {
                    preparation.completed_tracks += 1;
                    active.remove(&index);
                    if let Some(bytes) = known_bytes.checked_add(bytes) {
                        known_bytes = bytes;
                    } else {
                        failed.store(true, Ordering::Relaxed);
                        if first_error.is_none() {
                            first_error = Some(invalid("USB transfer size limit"));
                        }
                    }
                    (
                        index,
                        ExportStage::Audio,
                        "ready".into(),
                        "Pista preparada; en cola para copiar al pendrive".into(),
                    )
                }
                Event::Transfer(index, action, detail, state) => {
                    publication = state;
                    publication.phase = action.clone();
                    (index, ExportStage::Library, action, detail)
                }
                Event::Uploaded(index, record) => {
                    uploaded += 1;
                    records.insert(index, record);
                    publication.current_track = None;
                    publication.current_file = None;
                    publication.phase = "waiting".into();
                    (
                        index,
                        ExportStage::Library,
                        "publish_track_done".into(),
                        "Pista copiada y verificada".into(),
                    )
                }
                Event::Failed(index, error) => {
                    let detail = error.to_string();
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                    (index, ExportStage::Library, "error".into(), detail)
                }
            };
            preparation.active_tracks = active.values().cloned().collect();
            publication.total_bytes = known_bytes;
            publication.total_bytes_known =
                preparation.completed_tracks == preparation.total_tracks;
            publication.queued_tracks = preparation.completed_tracks.saturating_sub(uploaded);
            let mut event = ExportProgress::event(
                stage,
                preparation.completed_tracks,
                preparation.total_tracks,
                Some(tracks[index].title.clone()),
                &action,
                &detail,
            );
            event.preparation = Some(preparation.clone());
            event.publication = Some(publication.clone());
            progress(event);
        }
    });
    if let Some(error) = first_error {
        return Err(error);
    }
    if records.len() != tracks.len() {
        return Err(invalid(
            "USB processing or transfer queue did not finish every track",
        ));
    }
    Ok(records.into_values().collect())
}
