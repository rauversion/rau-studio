//! Bounded preparation workers; only the coordinator invokes progress.
use super::*;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc,
};
enum Event {
    Activity(usize, String, String),
    Done(usize, Result<ExportedTrack>),
}
pub(super) fn prepare(
    tracks: &[ExportTrack],
    workers: usize,
    task: impl Fn(&ExportTrack, &dyn Fn(&str, &str)) -> Result<ExportedTrack> + Sync,
    progress: &mut impl FnMut(ExportProgress),
) -> Result<Vec<ExportedTrack>> {
    let workers = workers.clamp(1, 4).min(tracks.len().max(1));
    let next = AtomicUsize::new(0);
    let failed = AtomicBool::new(false);
    let (tx, rx) = mpsc::sync_channel::<Event>(workers * 8);
    let mut results = BTreeMap::new();
    let mut completed = 0;
    let mut error = None;
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let tx = tx.clone();
            let next = &next;
            let failed = &failed;
            let task = &task;
            scope.spawn(move || {
                while !failed.load(Ordering::Relaxed) {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(track) = tracks.get(index) else {
                        break;
                    };
                    let notify = |action: &str, detail: &str| {
                        let _ = tx.send(Event::Activity(index, action.into(), detail.into()));
                    };
                    let result = task(track, &notify);
                    if result.is_err() {
                        failed.store(true, Ordering::Relaxed);
                    }
                    if tx.send(Event::Done(index, result)).is_err() {
                        break;
                    }
                }
            });
        }
        drop(tx);
        for event in rx {
            match event {
                Event::Activity(index, action, detail) => progress(ExportProgress::event(
                    if action == "analysis" {
                        ExportStage::Analysis
                    } else {
                        ExportStage::Audio
                    },
                    completed,
                    tracks.len(),
                    Some(tracks[index].title.clone()),
                    &action,
                    &detail,
                )),
                Event::Done(index, result) => match result {
                    Ok(record) => {
                        completed += 1;
                        progress(ExportProgress::event(
                            ExportStage::Audio,
                            completed,
                            tracks.len(),
                            Some(tracks[index].title.clone()),
                            "ready",
                            "Pista preparada y comprobada",
                        ));
                        results.insert(index, record);
                    }
                    Err(problem) => {
                        progress(ExportProgress::event(
                            ExportStage::Audio,
                            completed,
                            tracks.len(),
                            Some(tracks[index].title.clone()),
                            "error",
                            &problem.to_string(),
                        ));
                        if error.is_none() {
                            error = Some(problem);
                        }
                    }
                },
            }
        }
    });
    if let Some(error) = error {
        return Err(error);
    }
    if results.len() != tracks.len() {
        return Err(invalid("audio preparation did not finish every track"));
    }
    Ok(results.into_values().collect())
}
