use crate::{playlist_index::usb_export_source, system};
use aifficator_core::usb_export::{
    validate_classic_destination, write_classic_destination_with_progress, ExportPlan,
    ExportProgress, ExportStage, UsbExportError,
};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, State};
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
    plan: ExportPlan,
    summary: ExportPreview,
    ffmpeg: PathBuf,
    ffprobe: PathBuf,
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
) -> Result<ExportPreview, String> {
    {
        let mut session = manager.inner.lock().map_err(|e| e.to_string())?;
        if is_running(&session) {
            return Err("Ya hay una exportación en curso.".into());
        }
        session.prepared = None;
    }
    let prepared = tauri::async_runtime::spawn_blocking(move || {
        let output = destination(&destination_path)?;
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
        let snapshot = usb_export_source::snapshot(&app, &playlist_ids)?;
        let plan = snapshot.plan;
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
        let summary = ExportPreview {
            plan_id: Uuid::new_v4().to_string(),
            destination_path: output.to_string_lossy().into_owned(),
            playlist_ids,
            playlist_names: plan
                .playlists
                .iter()
                .filter(|p| !p.is_folder)
                .map(|p| p.name.clone())
                .collect(),
            track_count: plan.tracks.len(),
            copied_tracks: plan.tracks.iter().filter(|t| t.copy_source).count(),
            converted_tracks: plan.tracks.iter().filter(|t| !t.copy_source).count(),
            source_bytes: plan.source_bytes,
            estimated_audio_bytes: snapshot.estimated_audio_bytes,
            without_grid: plan
                .tracks
                .iter()
                .filter(|t| t.analysis.tempos.is_empty())
                .count(),
            with_cues: plan
                .tracks
                .iter()
                .filter(|t| !t.analysis.cues.is_empty())
                .count(),
            warning_count: notices.len(),
            warnings: notices.into_iter().take(30).collect(),
        };
        Ok::<_, String>(Prepared {
            plan,
            summary,
            ffmpeg: PathBuf::from(binary.path.unwrap_or_else(|| "ffmpeg".into())),
            ffprobe: PathBuf::from(probe.path.unwrap_or_else(|| "ffprobe".into())),
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

fn destination(path: &str) -> Result<PathBuf, String> {
    let output = Path::new(path);
    if !output.is_absolute() {
        return Err("Elige una carpeta de destino.".into());
    }
    validate_classic_destination(output).map_err(|e| match e {
        UsbExportError::DestinationConflict(_) => export_error(e),
        other => format!("No se pudo revisar el destino: {other}"),
    })
}

#[tauri::command]
pub(crate) fn usb_export_start(
    manager: State<'_, UsbExportManager>,
    plan_id: String,
) -> Result<ExportJob, String> {
    let mut session = manager.inner.lock().map_err(|e| e.to_string())?;
    if is_running(&session) {
        return Err("Ya hay una exportación en curso.".into());
    }
    if !session
        .prepared
        .as_ref()
        .is_some_and(|p| p.summary.plan_id == plan_id)
    {
        return Err("Revisa otra vez la selección antes de exportar.".into());
    }
    // Use the destination reviewed with this selection, then check it again
    // before consuming the plan. The writer also reserves its paths itself.
    let output = destination(&session.prepared.as_ref().unwrap().summary.destination_path)?;
    let prepared = session
        .prepared
        .take()
        .ok_or_else(|| "Falta revisar la selección.".to_string())?;
    let job = ExportJob {
        id: Uuid::new_v4().to_string(),
        status: JobStatus::Running,
        progress: ExportProgress {
            stage: ExportStage::Audio,
            completed_tracks: 0,
            total_tracks: prepared.plan.tracks.len(),
            current_track: None,
        },
        preview: prepared.summary,
        output_path: output.to_string_lossy().into_owned(),
        report_path: None,
        error: None,
    };
    session.job = Some(job.clone());
    drop(session);
    let shared = manager.inner.clone();
    tauri::async_runtime::spawn(async move {
        let progress_state = shared.clone();
        let result = tauri::async_runtime::spawn_blocking(move || {
            write_classic_destination_with_progress(
                &prepared.plan,
                &output,
                &prepared.ffmpeg,
                &prepared.ffprobe,
                |progress| {
                    if let Ok(mut state) = progress_state.lock() {
                        if let Some(job) = state.job.as_mut() {
                            job.progress = progress;
                        }
                    }
                },
            )
            .map_err(export_error)
        })
        .await
        .map_err(|e| format!("La exportación se interrumpió: {e}"))
        .and_then(|r| r);
        if let Ok(mut state) = shared.lock() {
            if let Some(job) = state.job.as_mut() {
                match result {
                    Ok(report) => {
                        job.status = JobStatus::Completed;
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
                        job.status = JobStatus::Failed;
                        job.error = Some(error);
                    }
                }
            }
        }
    });
    Ok(job)
}
