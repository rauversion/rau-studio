use crate::{playlist_index::usb_export_source, system};
use aifficator_core::usb_export::{
    available_export_workers, check_source, incremental, write_destination_cached, ExportPlan,
    ExportProgress, ExportStage, PreparationProgress, UnavailableTrack, UsbExportError,
};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

#[derive(Default)]
pub(crate) struct UsbExportManager {
    inner: Arc<Mutex<Session>>,
}

#[derive(Default)]
struct Session {
    prepared: Option<Prepared>,
    job: Option<ExportJob>,
}

struct Prepared {
    plan: Option<ExportPlan>,
    summary: ExportPreview,
    ffmpeg: PathBuf,
    ffprobe: PathBuf,
    analysis_cache: PathBuf,
    update: Option<incremental::Update>,
}

#[derive(Clone, Serialize)]
pub(crate) struct ExportPreview {
    plan_id: String,
    destination_path: String,
    playlist_ids: Vec<String>,
    playlist_names: Vec<String>,
    track_count: usize,
    copied_tracks: usize,
    converted_tracks: usize,
    source_bytes: u64,
    estimated_audio_bytes: Option<u64>,
    without_grid: usize,
    with_cues: usize,
    warning_count: usize,
    warnings: Vec<PreviewWarning>,
    unavailable_tracks: Vec<UnavailableTrack>,
    empty_playlists: Vec<String>,
    updating: bool,
    sync_counts: Option<incremental::SyncCounts>,
    workers: usize,
    upload_workers: usize,
}

#[derive(Clone, Serialize)]
struct ExportLog {
    id: u64,
    time: String,
    level: String,
    name: Option<String>,
    message: String,
}

#[derive(Serialize)]
pub(crate) struct ExportConfiguration {
    available_cores: usize,
    default_workers: usize,
    upload_workers: usize,
}

#[tauri::command]
pub(crate) fn usb_export_configuration() -> ExportConfiguration {
    let cores = available_export_workers();
    ExportConfiguration {
        available_cores: cores,
        default_workers: cores,
        upload_workers: 1,
    }
}

fn log(job: &mut ExportJob, level: &str, name: Option<String>, message: String) {
    let id = job.logs.last().map_or(1, |entry| entry.id + 1);
    job.logs.push(ExportLog {
        id,
        time: chrono::Utc::now().to_rfc3339(),
        level: level.into(),
        name,
        message,
    });
    if job.logs.len() > 1000 {
        job.logs.drain(..job.logs.len() - 1000);
    }
}

#[tauri::command]
pub(crate) async fn usb_export_inspect_destination(
    app: AppHandle,
    destination_path: String,
) -> Result<incremental::DestinationInfo, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut info =
            incremental::inspect_destination(Path::new(&destination_path)).map_err(export_error)?;
        usb_export_source::match_destination(&app, &mut info)?;
        Ok(info)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn usb_export_recover(
    app: AppHandle,
    manager: State<'_, UsbExportManager>,
    destination_path: String,
) -> Result<incremental::DestinationInfo, String> {
    let shared = manager.inner.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut session = shared.lock().map_err(|e| e.to_string())?;
        if is_running(&session) {
            return Err("Ya hay una exportación en curso.".into());
        }
        session.prepared = None;
        incremental::recover(Path::new(&destination_path)).map_err(export_error)?;
        let mut info =
            incremental::inspect_destination(Path::new(&destination_path)).map_err(export_error)?;
        usb_export_source::match_destination(&app, &mut info)?;
        Ok(info)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(crate) enum StartResult {
    Started { job: ExportJob },
    ReviewRequired { preview: ExportPreview },
}

enum StartDecision {
    Review(ExportPreview),
    Ready(Prepared, PathBuf, ExportJob),
}

#[derive(Clone, Serialize)]
struct PreviewWarning {
    track_title: String,
    message: String,
}

fn notice(message: &str) -> String {
    if message.starts_with("No XML beat grid") {
        "Sin grilla guardada ni importada. Puedes analizar la pista desde su detalle antes de exportar."
    } else if message.starts_with("Cue RGB") {
        "Los colores personalizados de los cues no se exportan todavía."
    } else if message.starts_with("XML position mark") {
        "Los marcadores de carga y fundido no se exportan como cues."
    } else {
        message
    }
    .to_string()
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "snake_case")]
enum JobStatus {
    Running,
    Completed,
    Failed,
}

#[derive(Clone, Serialize)]
pub(crate) struct ExportJob {
    id: String,
    status: JobStatus,
    progress: ExportProgress,
    preview: ExportPreview,
    output_path: String,
    report_path: Option<String>,
    error: Option<String>,
    logs: Vec<ExportLog>,
}

fn is_running(session: &Session) -> bool {
    session
        .job
        .as_ref()
        .is_some_and(|j| matches!(j.status, JobStatus::Running))
}

#[tauri::command]
pub(crate) async fn usb_export_prepare(
    app: AppHandle,
    manager: State<'_, UsbExportManager>,
    playlist_ids: Vec<String>,
    destination_path: String,
    workers: Option<usize>,
) -> Result<ExportPreview, String> {
    {
        let mut session = manager.inner.lock().map_err(|e| e.to_string())?;
        if is_running(&session) {
            return Err("Ya hay una exportación en curso.".into());
        }
        session.prepared = None;
    }
    let prepared = tauri::async_runtime::spawn_blocking(move || {
        if !Path::new(&destination_path).is_absolute() {
            return Err("Elige una carpeta de destino.".into());
        }
        let mut info =
            incremental::inspect_destination(Path::new(&destination_path)).map_err(export_error)?;
        if !info.can_update {
            return Err(info
                .reason
                .unwrap_or_else(|| "No se puede actualizar este destino.".into()));
        }
        let output = PathBuf::from(&info.path);
        let binary = system::binary_status(&app, "ffmpeg");
        if !binary.installed {
            return Err(
                "No está disponible el conversor de audio. Revisa FFmpeg en Ajustes.".to_string(),
            );
        }
        let probe = system::binary_status(&app, "ffprobe");
        if !probe.installed {
            return Err(
                "No está disponible el lector de audio. Revisa FFprobe en Ajustes.".to_string(),
            );
        }
        usb_export_source::match_destination(&app, &mut info)?;
        let existing = if info.existing {
            Some(incremental::load_existing(&output).map_err(export_error)?)
        } else {
            None
        };
        let snapshot = usb_export_source::snapshot(&app, &playlist_ids, existing.as_ref())?;
        let update = if let Some(existing) = existing {
            let bindings = info
                .playlists
                .iter()
                .filter_map(|p| p.source_id.as_ref().map(|s| (p.id, s.clone())))
                .collect();
            Some(
                incremental::prepare(
                    existing,
                    snapshot
                        .plan
                        .clone()
                        .ok_or("La selección no contiene playlists.")?,
                    bindings,
                )
                .map_err(export_error)?,
            )
        } else {
            None
        };
        let plan = update
            .as_ref()
            .map(|u| u.plan.clone())
            .or(snapshot.plan)
            .filter(|p| !p.tracks.is_empty());
        let tracks = plan
            .as_ref()
            .map(|p| p.tracks.as_slice())
            .unwrap_or_default();
        let mut notices: Vec<_> = tracks
            .iter()
            .flat_map(|t| {
                t.analysis.warnings.iter().map(move |w| PreviewWarning {
                    track_title: t.title.clone(),
                    message: notice(w),
                })
            })
            .collect();
        if let Some(update) = &update {
            notices.extend(update.notices.iter().map(|message| PreviewWarning {
                track_title: "USB".into(),
                message: message.clone(),
            }));
        }
        let workers = workers
            .unwrap_or_else(available_export_workers)
            .clamp(1, available_export_workers());
        let summary = ExportPreview {
            plan_id: Uuid::new_v4().to_string(),
            destination_path: output.to_string_lossy().into_owned(),
            playlist_ids,
            empty_playlists: plan.as_ref().map_or_else(
                || snapshot.playlist_names.clone(),
                |p| {
                    p.playlists
                        .iter()
                        .filter(|p| !p.is_folder && p.tracks.is_empty())
                        .map(|p| p.name.clone())
                        .collect()
                },
            ),
            playlist_names: plan.as_ref().map_or(snapshot.playlist_names, |p| {
                p.playlists
                    .iter()
                    .filter(|p| !p.is_folder)
                    .map(|p| p.name.clone())
                    .collect()
            }),
            track_count: tracks.len(),
            copied_tracks: tracks
                .iter()
                .filter(|t| {
                    t.copy_source
                        && update
                            .as_ref()
                            .is_none_or(|u| u.work[&t.id] == incremental::Work::Export)
                })
                .count(),
            converted_tracks: tracks
                .iter()
                .filter(|t| {
                    !t.copy_source
                        && update
                            .as_ref()
                            .is_none_or(|u| u.work[&t.id] == incremental::Work::Export)
                })
                .count(),
            source_bytes: plan.as_ref().map_or(0, |p| p.source_bytes),
            estimated_audio_bytes: if update.is_some() {
                None
            } else {
                snapshot.estimated_audio_bytes
            },
            without_grid: tracks
                .iter()
                .filter(|t| t.analysis.tempos.is_empty())
                .count(),
            with_cues: tracks
                .iter()
                .filter(|t| !t.analysis.cues.is_empty())
                .count(),
            warning_count: notices.len(),
            warnings: notices.into_iter().take(30).collect(),
            unavailable_tracks: plan
                .as_ref()
                .map_or(snapshot.unavailable_tracks, |p| p.omitted_tracks.clone()),
            updating: update.is_some(),
            sync_counts: update.as_ref().map(|u| u.counts.clone()),
            workers,
            upload_workers: 1,
        };
        Ok::<_, String>(Prepared {
            plan,
            summary,
            ffmpeg: PathBuf::from(binary.path.unwrap_or_else(|| "ffmpeg".into())),
            ffprobe: PathBuf::from(probe.path.unwrap_or_else(|| "ffprobe".into())),
            analysis_cache: app
                .path()
                .app_cache_dir()
                .map_err(|e| e.to_string())?
                .join("usb-analysis-v1"),
            update,
        })
    })
    .await
    .map_err(|e| format!("No se pudo preparar la selección: {e}"))??;
    let mut session = manager.inner.lock().map_err(|e| e.to_string())?;
    if is_running(&session) {
        return Err("Ya hay una exportación en curso.".into());
    }
    let summary = prepared.summary.clone();
    session.prepared = Some(prepared);
    Ok(summary)
}

#[tauri::command]
pub(crate) fn usb_export_status(
    manager: State<'_, UsbExportManager>,
) -> Result<Option<ExportJob>, String> {
    Ok(manager.inner.lock().map_err(|e| e.to_string())?.job.clone())
}

fn export_error(error: UsbExportError) -> String {
    match error {
        UsbExportError::DestinationConflict(name) => format!(
            "El destino ya contiene «{name}». Elige otro destino o mueve la exportación anterior antes de continuar."
        ),
        other => other.to_string(),
    }
}

/// A source can disappear after the user reviews it. Build a new review and
/// invalidate the old consent before touching the destination in that case.
fn recheck_sources(prepared: &mut Prepared) -> Result<bool, String> {
    if let Some(update) = &prepared.update {
        if update.plan.tracks.iter().any(|t| {
            update.work[&t.id] == incremental::Work::Export
                && check_source(Some(&t.source_path)).is_err()
        }) {
            return Err("Hay archivos nuevos que dejaron de estar disponibles. Vuelve a revisar para omitirlos o cancelar; la biblioteca del USB se conserva.".into());
        }
        return Ok(false);
    }
    let Some(plan) = prepared.plan.as_mut() else {
        return Ok(false);
    };
    let mut unavailable = Vec::new();
    let mut available_sizes = std::collections::BTreeMap::new();
    for track in &plan.tracks {
        match check_source(Some(&track.source_path)) {
            Ok(size) => {
                available_sizes.insert(track.id, size);
            }
            Err(problem) => unavailable.push(UnavailableTrack {
                source_key: track.source_key.clone(),
                title: track.title.clone(),
                artist: track.artist.clone(),
                source_path: Some(track.source_path.clone()),
                playlist_names: plan
                    .playlists
                    .iter()
                    .filter(|p| p.tracks.contains(&track.id))
                    .map(|p| p.name.clone())
                    .collect(),
                problem,
            }),
        }
    }
    if unavailable.is_empty() {
        return Ok(false);
    }
    plan.tracks
        .retain(|track| available_sizes.contains_key(&track.id));
    for playlist in &mut plan.playlists {
        playlist
            .tracks
            .retain(|id| available_sizes.contains_key(id));
    }
    plan.source_bytes = available_sizes
        .values()
        .try_fold(0u64, |total, size| total.checked_add(*size))
        .ok_or("El tamaño de los archivos supera el límite de exportación.")?;
    plan.omitted_tracks.extend(unavailable);
    let summary = &mut prepared.summary;
    summary.plan_id = Uuid::new_v4().to_string();
    summary.unavailable_tracks = plan.omitted_tracks.clone();
    summary.track_count = plan.tracks.len();
    summary.source_bytes = plan.source_bytes;
    summary.estimated_audio_bytes = None;
    summary.copied_tracks = plan.tracks.iter().filter(|t| t.copy_source).count();
    summary.converted_tracks = plan.tracks.len() - summary.copied_tracks;
    summary.without_grid = plan
        .tracks
        .iter()
        .filter(|t| t.analysis.tempos.is_empty())
        .count();
    summary.with_cues = plan
        .tracks
        .iter()
        .filter(|t| !t.analysis.cues.is_empty())
        .count();
    summary.empty_playlists = plan
        .playlists
        .iter()
        .filter(|p| !p.is_folder && p.tracks.is_empty())
        .map(|p| p.name.clone())
        .collect();
    let notices: Vec<_> = plan
        .tracks
        .iter()
        .flat_map(|t| {
            t.analysis.warnings.iter().map(move |w| PreviewWarning {
                track_title: t.title.clone(),
                message: notice(w),
            })
        })
        .collect();
    summary.warning_count = notices.len();
    summary.warnings = notices.into_iter().take(30).collect();
    if plan.tracks.is_empty() {
        prepared.plan = None;
    }
    Ok(true)
}

#[tauri::command]
pub(crate) async fn usb_export_start(
    manager: State<'_, UsbExportManager>,
    plan_id: String,
    skip_unavailable: Option<bool>,
) -> Result<StartResult, String> {
    let shared = manager.inner.clone();
    let review_state = shared.clone();
    let decision = tauri::async_runtime::spawn_blocking(move || {
        let mut session = review_state.lock().map_err(|e| e.to_string())?;
        if is_running(&session) {
            return Err("Ya hay una exportación en curso.".to_string());
        }
        if !session
            .prepared
            .as_ref()
            .is_some_and(|p| p.summary.plan_id == plan_id)
        {
            return Err("Revisa otra vez la selección antes de exportar.".to_string());
        }
        let prepared = session.prepared.as_mut().unwrap();
        let changed = recheck_sources(prepared)?;
        if changed
            || prepared.plan.is_none()
            || (!prepared.summary.unavailable_tracks.is_empty() && skip_unavailable != Some(true))
        {
            return Ok(StartDecision::Review(prepared.summary.clone()));
        }
        // The core revalidates the destination/revision before any writes.
        // Reopening and decoding both databases here would repeat the review.
        let output = PathBuf::from(&prepared.summary.destination_path);
        let prepared = session.prepared.take().unwrap();
        let job = ExportJob {
            id: Uuid::new_v4().to_string(),
            status: JobStatus::Running,
            progress: ExportProgress {
                stage: ExportStage::Audio,
                completed_tracks: 0,
                total_tracks: prepared.plan.as_ref().unwrap().tracks.len(),
                current_track: None,
                action: Some("start".into()),
                detail: None,
                publication: None,
                preparation: Some(PreparationProgress {
                    completed_tracks: 0,
                    total_tracks: prepared.summary.copied_tracks
                        + prepared.summary.converted_tracks
                        + prepared
                            .summary
                            .sync_counts
                            .as_ref()
                            .map_or(0, |counts| counts.analysis_tracks),
                    reused_tracks: prepared.update.as_ref().map_or(0, |update| {
                        update
                            .work
                            .values()
                            .filter(|work| **work == incremental::Work::Reuse)
                            .count()
                    }),
                    workers: prepared.summary.workers,
                    active_tracks: Vec::new(),
                }),
            },
            preview: prepared.summary.clone(),
            output_path: output.to_string_lossy().into_owned(),
            report_path: None,
            error: None,
            logs: Vec::new(),
        };
        let mut job = job;
        let message = format!(
            "{} · {} pistas en biblioteca · {} trabajadores CPU · 1 escritor USB",
            output.display(),
            job.preview.track_count,
            job.preview.workers
        );
        log(&mut job, "info", None, message);
        session.job = Some(job.clone());
        Ok(StartDecision::Ready(prepared, output, job))
    })
    .await
    .map_err(|e| e.to_string())??;
    let (prepared, output, job) = match decision {
        StartDecision::Review(preview) => return Ok(StartResult::ReviewRequired { preview }),
        StartDecision::Ready(prepared, output, job) => (prepared, output, job),
    };
    tauri::async_runtime::spawn(async move {
        let progress_state = shared.clone();
        let result = tauri::async_runtime::spawn_blocking(move || {
            let mut notify = |mut progress: ExportProgress| {
                if let Ok(mut state) = progress_state.lock() {
                    if let Some(job) = state.job.as_mut() {
                        if progress.preparation.is_none() {
                            progress.preparation = job.progress.preparation.clone();
                        }
                        if progress.publication.is_none() {
                            progress.publication = job.progress.publication.clone();
                        }
                        if !matches!(
                            progress.action.as_deref(),
                            Some("publish_bytes" | "verify_bytes")
                        ) {
                            let count = match &progress.publication {
                                Some(publication)
                                    if matches!(
                                        progress.action.as_deref(),
                                        Some(
                                            "publish_media"
                                                | "publish_media_done"
                                                | "publish_track_done"
                                                | "sync_media"
                                                | "verify_media"
                                        )
                                    ) =>
                                {
                                    format!(
                                        "{}/{} archivos publicados",
                                        publication.completed_files, publication.total_files
                                    )
                                }
                                _ if progress.preparation.is_some() => {
                                    let preparation = progress.preparation.as_ref().unwrap();
                                    format!(
                                        "{}/{} pistas preparadas",
                                        preparation.completed_tracks, preparation.total_tracks
                                    )
                                }
                                _ => format!(
                                    "{}/{} pistas preparadas",
                                    progress.completed_tracks, progress.total_tracks
                                ),
                            };
                            let message = format!(
                                "{} · {count} · {}",
                                progress.action.as_deref().unwrap_or("progress"),
                                progress.detail.as_deref().unwrap_or("")
                            );
                            let level = match progress.action.as_deref() {
                                Some("error") => "error",
                                Some("cache_error" | "cleanup_error") => "warning",
                                _ => "info",
                            };
                            log(job, level, progress.current_track.clone(), message);
                        }
                        job.progress = progress;
                    }
                }
            };
            if let Some(update) = &prepared.update {
                incremental::write(
                    update,
                    &prepared.ffmpeg,
                    &prepared.ffprobe,
                    prepared.summary.workers,
                    Some(&prepared.analysis_cache),
                    &mut notify,
                )
            } else {
                write_destination_cached(
                    prepared
                        .plan
                        .as_ref()
                        .expect("reviewed export has available tracks"),
                    &output,
                    &prepared.ffmpeg,
                    &prepared.ffprobe,
                    prepared.summary.workers,
                    Some(&prepared.analysis_cache),
                    &mut notify,
                )
            }
            .map_err(export_error)
        })
        .await
        .map_err(|e| format!("La exportación se interrumpió: {e}"))
        .and_then(|r| r);
        if let Ok(mut state) = shared.lock() {
            if let Some(job) = state.job.as_mut() {
                match result {
                    Ok(report) => {
                        log(job, "info", None, "Exportación completada; ambas bibliotecas y el informe están guardados.".into());
                        job.status = JobStatus::Completed;
                        let preparation = job.progress.preparation.clone();
                        let publication = job.progress.publication.clone();
                        job.progress = ExportProgress::event(
                            ExportStage::Library,
                            job.preview.track_count,
                            job.preview.track_count,
                            None,
                            "complete",
                            "Exportación completada",
                        );
                        job.progress.preparation = preparation;
                        job.progress.publication = publication;
                        job.report_path = Some(
                            Path::new(&job.output_path)
                                .join("rau-export-report.json")
                                .to_string_lossy()
                                .into_owned(),
                        );
                        job.preview.warning_count = report
                            .tracks
                            .iter()
                            .map(|t| t.analysis.warnings.len())
                            .sum();
                    }
                    Err(error) => {
                        log(job, "error", None, error.clone());
                        job.status = JobStatus::Failed;
                        job.error = Some(error);
                    }
                }
            }
        }
    });
    Ok(StartResult::Started { job })
}
