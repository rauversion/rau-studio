mod model;
mod partial;
pub(crate) mod preferences;
mod store;
#[cfg(test)]
mod tests;

use crate::settings;
use model::{Download, DownloadState, RemoteFile, Search, SearchHit};
use serde::{Deserialize, Serialize};
use soulseek_client::{
    soulseek_core::{Client, ClientConfig, Directory, Event, FileEntry},
    DownloadManager, DownloadRequest, FolderPlan, JobHandle, JobState, ManagerConfig,
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use store::{now, Store};
use tauri::{AppHandle, Manager, State};
use tokio::sync::Mutex;
use uuid::Uuid;

const DEFAULT_SOULSEEK_SERVER: &str = "server.slsknet.org:2242";

#[derive(Default)]
pub struct SoulseekState {
    service: Arc<Mutex<Service>>,
    pub exiting: AtomicBool,
}

#[derive(Default)]
struct Service {
    store: Option<Store>,
    session: Option<Session>,
    jobs: HashMap<String, ActiveJob>,
    searches: HashMap<u32, ActiveSearch>,
    last_error: Option<String>,
}

struct Session {
    id: String,
    server: String,
    account: String,
    client: Client,
    manager: DownloadManager,
    catalogue: Option<RemoteCatalogue>,
}

struct RemoteCatalogue {
    username: String,
    directories: Arc<Vec<Directory>>,
}

struct ActiveJob {
    handle: JobHandle,
    previous_attempts: u32,
}
struct ActiveSearch {
    id: String,
    started: Instant,
}

// Never serialize a password into a command response, history row or log.
#[derive(Deserialize, Serialize)]
struct Credentials {
    server: String,
    username: String,
    password: String,
}

#[derive(Serialize)]
pub struct Profile {
    server: String,
    username: String,
    password_saved: bool,
}

#[derive(Serialize)]
pub struct Snapshot {
    connected: bool,
    server: Option<String>,
    account: Option<String>,
    searches: Vec<Search>,
    downloads: Vec<Download>,
    last_error: Option<String>,
}

#[derive(Serialize)]
pub struct RemoteDirectory {
    path: String,
    files: Vec<RemoteFile>,
}

fn message(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn id() -> String {
    Uuid::new_v4().to_string()
}

impl Service {
    fn db(&self) -> Result<&Store, String> {
        self.store
            .as_ref()
            .ok_or_else(|| "El almacenamiento de Soulseek no está disponible.".into())
    }
    fn db_mut(&mut self) -> Result<&mut Store, String> {
        self.store
            .as_mut()
            .ok_or_else(|| "El almacenamiento de Soulseek no está disponible.".into())
    }
    fn session(&self) -> Result<&Session, String> {
        self.session
            .as_ref()
            .ok_or_else(|| "Conecta tu cuenta de Soulseek para continuar.".into())
    }

    fn snapshot(&self) -> Result<Snapshot, String> {
        Ok(Snapshot {
            connected: self.session.is_some(),
            server: self.session.as_ref().map(|s| s.server.clone()),
            account: self.session.as_ref().map(|s| s.account.clone()),
            searches: self.db()?.searches()?,
            downloads: self.db()?.downloads()?,
            last_error: self.last_error.clone(),
        })
    }

    fn check_account(&self, server: &str, account: &str) -> Result<(), String> {
        let session = self.session()?;
        if session.server != server || session.account != account {
            return Err("Conecta la misma cuenta y servidor que originaron este trabajo.".into());
        }
        Ok(())
    }

    fn sync_jobs(&mut self) -> Result<(), String> {
        let mut finished = Vec::new();
        for (id, active) in &self.jobs {
            let snapshot = active.handle.snapshot();
            let mut d = self.db()?.download(id)?;
            // Paused files cannot grow through this manager. Check them again
            // when the user resumes, instead of reading every paused file each tick.
            if snapshot.state == JobState::Paused && d.state == DownloadState::Paused {
                continue;
            }
            let previous = d.clone();
            d.state = match snapshot.state {
                JobState::Queued => DownloadState::Queued,
                JobState::Running => DownloadState::Running,
                JobState::RetryWaiting => DownloadState::RetryWaiting,
                JobState::Paused => DownloadState::Paused,
                JobState::Completed => DownloadState::Completed,
                JobState::Cancelled => DownloadState::Cancelled,
                JobState::Failed => DownloadState::Failed,
            };
            d.attempts = active.previous_attempts.saturating_add(snapshot.attempts);
            d.last_error = snapshot.last_error.clone();
            // A failed CreateNew request must never adopt an unrelated existing file.
            if snapshot.attempts > 0 || d.checkpoint.is_some() {
                let checkpoint = (|| {
                    if let Some(saved) = &d.checkpoint {
                        partial::verify(Path::new(&d.destination), d.file.size, Some(saved), true)?;
                    }
                    partial::capture(Path::new(&d.destination), d.file.size)
                })();
                match checkpoint {
                    Ok(Some(checkpoint)) => {
                        d.bytes = checkpoint.bytes;
                        d.checkpoint = Some(checkpoint);
                    }
                    Ok(None) => {
                        d.last_error =
                            Some("El archivo parcial de este trabajo ya no existe.".into());
                        if snapshot.state.is_terminal() {
                            d.state = DownloadState::Failed;
                        }
                        self.db()?.update_download(&d)?;
                        if !snapshot.state.is_terminal() {
                            return Err(d.last_error.unwrap());
                        }
                    }
                    Err(error) => {
                        d.last_error = Some(error.clone());
                        if snapshot.state.is_terminal() {
                            d.state = DownloadState::Failed;
                        }
                        self.db()?.update_download(&d)?;
                        if !snapshot.state.is_terminal() {
                            return Err(error);
                        }
                    }
                }
            }
            if d.state != previous.state
                || d.bytes != previous.bytes
                || d.attempts != previous.attempts
                || d.last_error != previous.last_error
                || d.checkpoint != previous.checkpoint
            {
                self.db()?.update_download(&d)?;
            }
            if snapshot.state.is_terminal() {
                finished.push((id.clone(), snapshot.id));
            }
        }
        for (id, runtime_id) in finished {
            if let Some(session) = &self.session {
                session.manager.forget(runtime_id).map_err(message)?;
            }
            self.jobs.remove(&id);
        }
        Ok(())
    }

    async fn stop_search(&mut self, search_id: &str, state: &str) -> Result<(), String> {
        let token = self
            .searches
            .iter()
            .find(|(_, s)| s.id == search_id)
            .map(|(t, _)| *t);
        if let Some(token) = token {
            if let Some(session) = &self.session {
                session.client.cancel_search(token).await;
            }
            self.searches.remove(&token);
        }
        self.db()?.finish_search(search_id, state, None)
    }

    async fn disconnect(&mut self) -> Result<(), String> {
        let mut result = Ok(());
        let ids: Vec<_> = self.searches.values().map(|s| s.id.clone()).collect();
        for id in ids {
            if let Err(error) = self.stop_search(&id, "interrupted").await {
                result = Err(error);
            }
        }
        // Pause and checkpoint after writers stop, then discard runtime handles.
        for active in self.jobs.values() {
            active.handle.pause().await;
        }
        if let Err(error) = self.sync_jobs() {
            result = Err(error);
        }
        if let Some(session) = self.session.take() {
            session.manager.shutdown().await;
            session.client.disconnect();
        }
        self.jobs.clear();
        result
    }

    fn admit(&mut self, records: Vec<Download>) -> Result<(), String> {
        let manager = self.session()?.manager.clone();
        if records.len() > store::MAX_JOBS.saturating_sub(manager.snapshots().len()) {
            return Err(
                "La cola no tiene capacidad para todos los archivos de esta carpeta.".into(),
            );
        }
        // Durable admission precedes scheduling. A crash in between leaves an
        // interrupted intent, never a worker with no durable source/destination.
        self.db_mut()?.insert_downloads(&records)?;
        for mut record in records {
            match self.start_record(&record) {
                Ok(handle) => {
                    self.jobs.insert(
                        record.id.clone(),
                        ActiveJob {
                            handle,
                            previous_attempts: record.attempts,
                        },
                    );
                }
                Err(reason) => {
                    record.state = DownloadState::Failed;
                    record.last_error = Some(reason);
                    self.db()?.update_download(&record)?;
                }
            }
        }
        Ok(())
    }

    fn start_record(&self, record: &Download) -> Result<JobHandle, String> {
        self.check_account(&record.server, &record.account)?;
        self.session()?
            .manager
            .enqueue(restore_request(record)?)
            .map_err(message)
    }
}

pub fn setup(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let directory = app.path().app_data_dir()?;
    std::fs::create_dir_all(&directory)?;
    let state = app.state::<SoulseekState>();
    let mut db =
        Store::open(&directory.join("aifficator.sqlite3")).map_err(std::io::Error::other)?;
    db.recover().map_err(std::io::Error::other)?;
    let service = state.service.clone();
    // setup runs before the event loop; no task can have acquired this mutex.
    service
        .try_lock()
        .map_err(|_| std::io::Error::other("Soulseek setup already running"))?
        .store = Some(db);
    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            let mut service = service.lock().await;
            if let Err(error) = service.sync_jobs() {
                // Stop writers on a checkpoint/storage failure; keep the error
                // visible and retry persistence on the next tick.
                for active in service.jobs.values() {
                    active.handle.pause().await;
                }
                service.last_error = Some(error);
            }
            let expired: Vec<_> = service
                .searches
                .values()
                .filter(|s| s.started.elapsed() >= Duration::from_secs(60))
                .map(|s| s.id.clone())
                .collect();
            for id in expired {
                if let Err(error) = service.stop_search(&id, "completed").await {
                    service.last_error = Some(error);
                }
            }
        }
    });
    Ok(())
}

pub fn request_exit(app: &AppHandle, code: i32) -> bool {
    let state = app.state::<SoulseekState>();
    if state.exiting.swap(true, Ordering::SeqCst) {
        return false;
    }
    let service = state.service.clone();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = service.lock().await.disconnect().await {
            eprintln!("Soulseek shutdown: {error}");
        }
        app.exit(code);
    });
    true
}

#[tauri::command]
pub async fn soulseek_profile(app: AppHandle) -> Result<Profile, String> {
    let credentials = settings::load_soulseek_credentials(&app)?
        .map(|v| serde_json::from_str::<Credentials>(&v))
        .transpose()
        .map_err(message)?;
    Ok(match credentials {
        Some(c) => Profile {
            server: c.server,
            username: c.username,
            password_saved: !c.password.is_empty(),
        },
        None => Profile {
            server: DEFAULT_SOULSEEK_SERVER.into(),
            username: String::new(),
            password_saved: false,
        },
    })
}

#[tauri::command]
pub async fn soulseek_forget_credentials(app: AppHandle) -> Result<(), String> {
    settings::save_soulseek_credentials(&app, None)
}

#[tauri::command]
pub async fn soulseek_snapshot(state: State<'_, SoulseekState>) -> Result<Snapshot, String> {
    state.service.lock().await.snapshot()
}

#[tauri::command]
pub async fn soulseek_connect(
    app: AppHandle,
    state: State<'_, SoulseekState>,
    server: String,
    username: String,
    password: Option<String>,
    remember: bool,
) -> Result<Snapshot, String> {
    let server = match server.trim() {
        "" => DEFAULT_SOULSEEK_SERVER,
        server => server,
    }
    .to_string();
    let username = username.trim().to_string();
    if server.len() > 255 || !server.contains(':') || username.is_empty() || username.len() > 128 {
        return Err("Indica servidor con puerto y usuario de Soulseek.".into());
    }
    let password = match password.filter(|p| !p.is_empty()) {
        Some(p) => p,
        None => {
            let stored = settings::load_soulseek_credentials(&app)?
                .ok_or("Ingresa la contraseña de Soulseek.")?;
            let saved: Credentials = serde_json::from_str(&stored).map_err(message)?;
            if saved.server != server || saved.username != username {
                return Err("La contraseña guardada pertenece a otra cuenta o servidor.".into());
            }
            saved.password
        }
    };
    let mut service = state.service.lock().await;
    if service.session.is_some() {
        return Err("Desconecta la sesión actual antes de cambiar de cuenta.".into());
    }
    let mut config = ClientConfig::new(&server);
    config.limits.max_items = 1_000_000;
    let client = Client::connect(config, &username, &password)
        .await
        .map_err(message)?;
    let manager =
        DownloadManager::new(client.clone(), ManagerConfig::default()).map_err(message)?;
    let persisted = if remember {
        settings::save_soulseek_credentials(
            &app,
            Some(
                serde_json::to_string(&Credentials {
                    server: server.clone(),
                    username: username.clone(),
                    password,
                })
                .map_err(message)?,
            ),
        )
    } else {
        settings::save_soulseek_credentials(&app, None)
    };
    if let Err(error) = persisted {
        client.disconnect();
        return Err(error);
    }
    let mut events = client.subscribe();
    let session_id = id();
    service.session = Some(Session {
        id: session_id.clone(),
        server,
        account: username,
        client,
        manager,
        catalogue: None,
    });
    service.last_error = None;
    let runtime = state.service.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            match events.recv().await {
                Ok(Event::SearchResult(result)) => {
                    let mut service = runtime.lock().await;
                    if service.session.as_ref().map(|s| &s.id) != Some(&session_id) {
                        break;
                    }
                    if let Some(search_id) =
                        service.searches.get(&result.token).map(|s| s.id.clone())
                    {
                        let hits: Vec<_> = result
                            .files
                            .iter()
                            .map(|f| SearchHit {
                                id: id(),
                                search_id: search_id.clone(),
                                username: result.username.clone(),
                                file: f.into(),
                                slots_free: result.slots_free,
                                average_speed: result.average_speed,
                                queue_length: result.queue_length,
                            })
                            .collect();
                        if let Err(error) = service
                            .db_mut()
                            .and_then(|db| db.save_hits(&search_id, &hits))
                        {
                            service.last_error = Some(error);
                        }
                    }
                }
                Ok(Event::Disconnected) => {
                    let mut service = runtime.lock().await;
                    if service.session.as_ref().map(|s| &s.id) != Some(&session_id) {
                        break;
                    }
                    if let Err(error) = service.disconnect().await {
                        service.last_error = Some(error);
                    }
                    service.last_error.get_or_insert_with(|| {
                        "Se perdió la conexión con Soulseek. Conecta de nuevo para reanudar.".into()
                    });
                    break;
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    let mut service = runtime.lock().await;
                    if service.session.as_ref().map(|s| &s.id) != Some(&session_id) {
                        break;
                    }
                    service.last_error = Some("Soulseek produjo más eventos de los que se pudieron guardar. Algunos resultados pueden faltar.".into());
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                _ => {}
            }
        }
    });
    service.snapshot()
}

#[tauri::command]
pub async fn soulseek_disconnect(state: State<'_, SoulseekState>) -> Result<Snapshot, String> {
    let mut service = state.service.lock().await;
    service.disconnect().await?;
    service.snapshot()
}

#[tauri::command]
pub async fn soulseek_search(
    state: State<'_, SoulseekState>,
    query: String,
) -> Result<Search, String> {
    let query = query.trim().to_string();
    if query.len() < 2 || query.len() > 256 {
        return Err("La búsqueda debe tener entre 2 y 256 caracteres.".into());
    }
    let mut service = state.service.lock().await;
    if service.searches.len() >= 8 {
        return Err("Detén una búsqueda antes de iniciar otra.".into());
    }
    let session = service.session()?;
    let search = Search {
        id: id(),
        server: session.server.clone(),
        account: session.account.clone(),
        query: query.clone(),
        state: "searching".into(),
        result_count: 0,
        created_at: now(),
        last_error: None,
    };
    service.db()?.insert_search(&search)?;
    match service.session()?.client.search(query).await {
        Ok(token) => {
            service.searches.insert(
                token,
                ActiveSearch {
                    id: search.id.clone(),
                    started: Instant::now(),
                },
            );
        }
        Err(error) => {
            service
                .db()?
                .finish_search(&search.id, "failed", Some(&error.to_string()))?;
            return Err(error.to_string());
        }
    }
    Ok(search)
}

#[tauri::command]
pub async fn soulseek_stop_search(
    state: State<'_, SoulseekState>,
    search_id: String,
) -> Result<(), String> {
    state
        .service
        .lock()
        .await
        .stop_search(&search_id, "stopped")
        .await
}

#[tauri::command]
pub async fn soulseek_delete_search(
    state: State<'_, SoulseekState>,
    search_id: String,
) -> Result<(), String> {
    let mut service = state.service.lock().await;
    service.stop_search(&search_id, "stopped").await?;
    service.db()?.delete_search(&search_id)
}

#[tauri::command]
pub async fn soulseek_search_results(
    state: State<'_, SoulseekState>,
    search_id: String,
) -> Result<Vec<SearchHit>, String> {
    state.service.lock().await.db()?.hits(&search_id)
}

#[tauri::command]
pub async fn soulseek_browse(
    state: State<'_, SoulseekState>,
    username: String,
) -> Result<Vec<RemoteDirectory>, String> {
    if username.trim().is_empty() || username.len() > 128 {
        return Err("Indica el usuario remoto.".into());
    }
    let (client, session_id) = {
        let service = state.service.lock().await;
        let session = service.session()?;
        (session.client.clone(), session.id.clone())
    };
    let directories = Arc::new(client.browse(&username).await.map_err(message)?);
    let response = directories
        .iter()
        .map(|d| RemoteDirectory {
            path: d.name.clone(),
            files: d.files.iter().map(RemoteFile::from).collect(),
        })
        .collect();
    let mut service = state.service.lock().await;
    let session = service
        .session
        .as_mut()
        .ok_or("La sesión de Soulseek se cerró.")?;
    if session.id != session_id {
        return Err(
            "La sesión cambió mientras se consultaba el usuario. Vuelve a explorarlo.".into(),
        );
    }
    session.catalogue = Some(RemoteCatalogue {
        username,
        directories,
    });
    Ok(response)
}

fn record(
    session: &Session,
    request: &DownloadRequest,
    root: &Path,
    remote_root: &str,
    batch: Option<&str>,
) -> Download {
    let timestamp = now();
    Download {
        id: id(),
        batch_id: batch.map(str::to_owned),
        server: session.server.clone(),
        account: session.account.clone(),
        username: request.username.clone(),
        file: (&request.file).into(),
        destination: request.destination.to_string_lossy().into_owned(),
        output_root: root.to_string_lossy().into_owned(),
        remote_root: remote_root.into(),
        state: DownloadState::Queued,
        bytes: 0,
        attempts: 0,
        last_error: None,
        checkpoint: None,
        created_at: timestamp.clone(),
        updated_at: timestamp,
    }
}

fn remote_parent(path: &str) -> Result<(&str, &str), String> {
    path.rsplit_once(['\\', '/'])
        .ok_or_else(|| "Soulseek no entregó una ruta remota completa.".into())
}

fn restore_request(record: &Download) -> Result<DownloadRequest, String> {
    let destination = Path::new(&record.destination);
    let root = Path::new(&record.output_root);
    partial::validate_destination(root, destination)?;
    let offset = partial::verify(
        destination,
        record.file.size,
        record.checkpoint.as_ref(),
        record.state == DownloadState::Interrupted,
    )?;
    let (directory, name) = remote_parent(&record.file.path)?;
    let mut file = FileEntry::from(&record.file);
    file.name = name.to_string();
    // Rebuild the library's checked output-root guard as well as the request.
    // Restoring with DownloadRequest::new alone would lose the folder guard.
    let plan = FolderPlan::from_catalogue(
        &record.username,
        &record.remote_root,
        &[Directory {
            name: directory.into(),
            files: vec![file],
        }],
        root,
    )
    .map_err(message)?;
    let mut request = plan
        .into_requests()
        .into_iter()
        .next()
        .ok_or("El archivo no pertenece a la carpeta remota guardada.")?;
    request.file = FileEntry::from(&record.file);
    request.destination = destination.into(); // A single-file save may use a custom local name.
    if let Some(offset) = offset {
        request = request.resume_from(offset);
    }
    Ok(request)
}

#[tauri::command]
pub async fn soulseek_download_file(
    app: AppHandle,
    state: State<'_, SoulseekState>,
    result_id: String,
    destination: Option<String>,
) -> Result<Snapshot, String> {
    let mut service = state.service.lock().await;
    let hit = service.db()?.hit(&result_id)?;
    let search = service.db()?.search(&hit.search_id)?;
    service.check_account(&search.server, &search.account)?;
    let preferred = preferences::preferred_directory(&app)?;
    let destination = preferences::file_destination(
        preferred.as_deref(),
        &hit.file.path,
        destination.as_deref().map(Path::new),
    )?;
    enqueue_file(&mut service, &hit.username, (&hit.file).into(), destination)?;
    service.snapshot()
}

fn enqueue_file(
    service: &mut Service,
    username: &str,
    file: FileEntry,
    destination: PathBuf,
) -> Result<(), String> {
    let root = destination.parent().ok_or("Destino inválido.")?;
    let (remote_root, _) = remote_parent(&file.name)?;
    let request = DownloadRequest::new(username, file.clone(), &destination);
    let download = record(service.session()?, &request, root, remote_root, None);
    partial::validate_destination(root, &destination)?;
    partial::verify(&destination, file.size, None, false)?;
    service.admit(vec![download])
}

fn browsed_file(
    catalogue: &RemoteCatalogue,
    username: &str,
    directory: &str,
    filename: &str,
) -> Result<FileEntry, String> {
    if catalogue.username != username {
        return Err("Vuelve a explorar el usuario para actualizar su catálogo.".into());
    }
    let folder = catalogue
        .directories
        .iter()
        .find(|d| d.name == directory)
        .ok_or("La carpeta no está en el catálogo explorado.")?;
    let mut file = folder
        .files
        .iter()
        .find(|file| file.name == filename)
        .cloned()
        .ok_or("El archivo no está en el catálogo explorado.")?;
    file.name = format!(
        "{}\\{}",
        folder.name.trim_end_matches(['\\', '/']),
        file.name
    );
    Ok(file)
}

#[tauri::command]
pub async fn soulseek_download_browsed_file(
    app: AppHandle,
    state: State<'_, SoulseekState>,
    username: String,
    directory: String,
    filename: String,
    destination: Option<String>,
) -> Result<Snapshot, String> {
    let mut service = state.service.lock().await;
    let catalogue = service
        .session()?
        .catalogue
        .as_ref()
        .ok_or("Explora el usuario antes de descargar un archivo.")?;
    let file = browsed_file(catalogue, &username, &directory, &filename)?;
    let preferred = preferences::preferred_directory(&app)?;
    let destination = preferences::file_destination(
        preferred.as_deref(),
        &file.name,
        destination.as_deref().map(Path::new),
    )?;
    enqueue_file(&mut service, &username, file, destination)?;
    service.snapshot()
}

#[tauri::command]
pub async fn soulseek_download_folder(
    app: AppHandle,
    state: State<'_, SoulseekState>,
    username: String,
    folder: String,
    destination: Option<String>,
) -> Result<Snapshot, String> {
    let (client, session_id, cached) = {
        let service = state.service.lock().await;
        let session = service.session()?;
        (
            session.client.clone(),
            session.id.clone(),
            session
                .catalogue
                .as_ref()
                .filter(|c| c.username == username)
                .map(|c| c.directories.clone()),
        )
    };
    let preferred = preferences::preferred_directory(&app)?;
    let root = preferences::folder_destination(
        preferred.as_deref(),
        &folder,
        destination.as_deref().map(Path::new),
    )?;
    // Browse the complete catalogue: folder() on some peers omits descendants.
    let plan = match cached {
        Some(catalogue) => {
            FolderPlan::from_catalogue(&username, &folder, &catalogue, &root).map_err(message)?
        }
        None => FolderPlan::browse(&client, &username, &folder, &root)
            .await
            .map_err(message)?,
    };
    if plan.requests().is_empty() {
        return Err("La carpeta no contiene archivos descargables.".into());
    }
    let mut service = state.service.lock().await;
    if service.session()?.id != session_id {
        return Err(
            "La sesión cambió mientras se consultaba la carpeta. Vuelve a explorarla.".into(),
        );
    }
    let batch = id();
    let mut records = Vec::with_capacity(plan.requests().len());
    for request in plan.requests() {
        partial::validate_destination(&root, &request.destination)?;
        partial::verify(&request.destination, request.file.size, None, false)?;
        records.push(record(
            service.session()?,
            request,
            &root,
            &folder,
            Some(&batch),
        ));
    }
    service.admit(records)?;
    service.snapshot()
}

#[tauri::command]
pub async fn soulseek_download_action(
    state: State<'_, SoulseekState>,
    download_id: String,
    action: String,
) -> Result<Snapshot, String> {
    let mut service = state.service.lock().await;
    let mut download = service.db()?.download(&download_id)?;
    match action.as_str() {
        "pause" => {
            let active = service
                .jobs
                .get(&download_id)
                .ok_or("El trabajo no está activo.")?;
            active.handle.pause().await;
            service.sync_jobs()?;
        }
        "resume" | "retry" => {
            service.check_account(&download.server, &download.account)?;
            if let Some(active) = service.jobs.get(&download_id) {
                partial::validate_destination(
                    Path::new(&download.output_root),
                    Path::new(&download.destination),
                )?;
                partial::verify(
                    Path::new(&download.destination),
                    download.file.size,
                    download.checkpoint.as_ref(),
                    false,
                )?;
                let handle = active.handle.clone();
                let original_state = download.state;
                download.state = DownloadState::Queued;
                service.db()?.update_download(&download)?;
                let mut updates = handle.subscribe();
                if let Err(error) = handle.resume().await {
                    download.state = original_state;
                    service.db()?.update_download(&download)?;
                    return Err(error.to_string());
                }
                // Do not persist a stale Paused snapshot after resume has been
                // accepted: a crash must allow an append since this checkpoint.
                while updates.borrow_and_update().state == JobState::Paused {
                    if updates.changed().await.is_err() {
                        break;
                    }
                }
            } else {
                if !matches!(
                    download.state,
                    DownloadState::Paused
                        | DownloadState::Interrupted
                        | DownloadState::Failed
                        | DownloadState::Cancelled
                ) {
                    return Err("Este trabajo no se puede reanudar.".into());
                }
                let request = restore_request(&download)?;
                download.state = DownloadState::Queued;
                download.last_error = None;
                // Reserve the durable destination before a new worker can write.
                service.db()?.update_download(&download)?;
                let handle = match service.session()?.manager.enqueue(request) {
                    Ok(handle) => handle,
                    Err(error) => {
                        download.state = DownloadState::Failed;
                        download.last_error = Some(error.to_string());
                        service.db()?.update_download(&download)?;
                        return Err(error.to_string());
                    }
                };
                service.jobs.insert(
                    download_id.clone(),
                    ActiveJob {
                        handle,
                        previous_attempts: download.attempts,
                    },
                );
            }
            service.sync_jobs()?;
        }
        "cancel" => {
            if let Some(active) = service.jobs.get(&download_id) {
                active.handle.cancel().await;
                service.sync_jobs()?;
            } else if download.state != DownloadState::Completed {
                download.state = DownloadState::Cancelled;
                service.db()?.update_download(&download)?;
            }
        }
        "forget" => {
            service.db()?.delete_download(&download_id)?;
        }
        _ => return Err("Acción de descarga inválida.".into()),
    }
    service.snapshot()
}
