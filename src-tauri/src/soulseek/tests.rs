use super::*;
use soulseek_client::soulseek_core::{
    wire::{self, CodeWidth, Frame, Reader, Writer},
    Cancellation, Limits, ResumeMode, ShareIndex,
};
use std::{
    fs,
    io::Write,
    net::{Ipv4Addr, SocketAddr},
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::Notify,
    time::timeout,
};

const DEADLINE: Duration = Duration::from_secs(10);

// A private server routes two real protocol clients over loopback. No public
// account, credentials or network service is used in these integration tests.
struct LocalServer {
    address: SocketAddr,
    ports: Arc<Mutex<HashMap<String, u16>>>,
    registered: Arc<Notify>,
    stop: Cancellation,
}
impl Drop for LocalServer {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}
impl LocalServer {
    async fn start() -> Self {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let server = Self {
            address: listener.local_addr().unwrap(),
            ports: Arc::default(),
            registered: Arc::default(),
            stop: Cancellation::new(),
        };
        let ports = server.ports.clone();
        let registered = server.registered.clone();
        let stop = server.stop.clone();
        tokio::spawn(async move {
            loop {
                let accepted = tokio::select! { _ = stop.cancelled() => break, result = listener.accept() => result };
                let (mut socket, _) = accepted.unwrap();
                let ports = ports.clone();
                let registered = registered.clone();
                let stop = stop.clone();
                tokio::spawn(async move {
                    let limits = Limits::default();
                    let login = wire::read_frame(&mut socket, CodeWidth::U32, &limits)
                        .await
                        .unwrap();
                    assert_eq!(login.code, 1);
                    let user = Reader::new(&login.payload, &limits).string().unwrap();
                    send(&mut socket, 1, |w| {
                        w.bool(true).unwrap();
                        w.string("local test").unwrap();
                        w.u32(u32::from(Ipv4Addr::LOCALHOST)).unwrap();
                        w.string("").unwrap();
                        w.bool(false).unwrap();
                    })
                    .await;
                    loop {
                        let frame = tokio::select! { _ = stop.cancelled() => break, result = wire::read_frame(&mut socket,CodeWidth::U32,&limits) => match result { Ok(frame) => frame, Err(_) => break } };
                        let mut reader = Reader::new(&frame.payload, &limits);
                        match frame.code {
                            2 => {
                                ports
                                    .lock()
                                    .await
                                    .insert(user.clone(), reader.u32().unwrap() as u16);
                                registered.notify_waiters();
                            }
                            3 => {
                                let target = reader.string().unwrap();
                                let port = *ports.lock().await.get(&target).unwrap();
                                send(&mut socket, 3, |w| {
                                    w.string(&target).unwrap();
                                    w.u32(u32::from(Ipv4Addr::LOCALHOST)).unwrap();
                                    w.u32(port as u32).unwrap();
                                })
                                .await;
                            }
                            _ => {}
                        }
                    }
                });
            }
        });
        server
    }

    async fn session(&self, account: &str) -> Session {
        self.ports.lock().await.remove(account);
        let mut config = ClientConfig::new(self.address.to_string());
        config.listen_addr = (Ipv4Addr::LOCALHOST, 0).into();
        config.connect_timeout = Duration::from_secs(1);
        config.request_timeout = Duration::from_secs(2);
        let client = Client::connect(config, account, "test-only").await.unwrap();
        timeout(DEADLINE, async {
            loop {
                let wait = self.registered.notified();
                if self.ports.lock().await.contains_key(account) {
                    break;
                }
                wait.await;
            }
        })
        .await
        .unwrap();
        let manager = DownloadManager::new(client.clone(), ManagerConfig::default()).unwrap();
        Session {
            id: id(),
            server: self.address.to_string(),
            account: account.into(),
            client,
            manager,
            catalogue: None,
        }
    }
}

async fn send(socket: &mut TcpStream, code: u32, build: impl FnOnce(&mut Writer<'_>)) {
    let limits = Limits::default();
    let mut writer = Writer::new(&limits);
    build(&mut writer);
    wire::write_frame(
        socket,
        &Frame {
            code,
            payload: writer.finish(),
        },
        CodeWidth::U32,
        &limits,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn a_fresh_manager_restores_a_folder_partial_and_its_output_guard() {
    let server = LocalServer::start().await;
    let uploader = server.session("uploader").await;
    let source = tempfile::tempdir().unwrap();
    fs::create_dir(source.path().join("disc")).unwrap();
    let content = b"verified music content from the original remote source";
    fs::write(source.path().join("disc/track.flac"), content).unwrap();
    fs::write(source.path().join("cover.jpg"), b"cover").unwrap();
    let mut shares = ShareIndex::new();
    shares.add_directory("Album", source.path()).unwrap();
    uploader.client.set_shares(shares).await.unwrap();

    let output = tempfile::tempdir().unwrap();
    let root = output.path().canonicalize().unwrap();
    let history = tempfile::tempdir().unwrap();
    let path = history.path().join("history.db");
    let initial = server.session("downloader").await;
    let plan = FolderPlan::browse(&initial.client, "uploader", "Album", &root)
        .await
        .unwrap();
    assert_eq!(plan.requests().len(), 2);
    let mut records: Vec<_> = plan
        .requests()
        .iter()
        .map(|request| record(&initial, request, &root, "Album", Some("batch")))
        .collect();
    let track = records
        .iter_mut()
        .find(|d| d.file.path.ends_with("track.flac"))
        .unwrap();
    let stable_id = track.id.clone();
    let partial_path = PathBuf::from(&track.destination);
    fs::create_dir_all(partial_path.parent().unwrap()).unwrap();
    fs::write(&partial_path, &content[..5]).unwrap();
    track.checkpoint = partial::capture(&partial_path, track.file.size).unwrap();
    track.bytes = 5;
    track.state = DownloadState::Running;
    track.attempts = 1;
    let mut db = Store::open(&path).unwrap();
    db.insert_downloads(&records).unwrap();
    drop(db);
    // Simulate a crash after additional bytes, before the next durable tick.
    fs::OpenOptions::new()
        .append(true)
        .open(&partial_path)
        .unwrap()
        .write_all(&content[5..9])
        .unwrap();
    initial.manager.shutdown().await;
    initial.client.disconnect();
    drop(initial);

    let session = server.session("downloader-restored").await;
    let mut db = Store::open(&path).unwrap();
    db.recover().unwrap();
    let mut service = Service {
        store: Some(db),
        session: Some(session),
        ..Default::default()
    };
    let restored = service.db().unwrap().download(&stable_id).unwrap();
    assert_eq!(restored.state, DownloadState::Interrupted);
    assert!(service
        .check_account(&restored.server, &restored.account)
        .is_err());
    // Reconnect with the original account; runtime IDs are unrelated to UUIDs.
    service.disconnect().await.unwrap();
    service.session = Some(server.session("downloader").await);
    let request = restore_request(&restored).unwrap();
    assert_eq!(request.output_root(), Some(root.as_path()));
    assert_eq!(request.resume, ResumeMode::Resume { expected_offset: 9 });
    for d in service.db().unwrap().downloads().unwrap() {
        let handle = service.start_record(&d).unwrap();
        service.jobs.insert(
            d.id.clone(),
            ActiveJob {
                handle,
                previous_attempts: d.attempts,
            },
        );
    }
    for active in service.jobs.values() {
        let finished = timeout(DEADLINE, active.handle.wait()).await.unwrap();
        assert_eq!(
            finished.state,
            JobState::Completed,
            "{:?}",
            finished.last_error
        );
    }
    service.sync_jobs().unwrap();
    let completed = service.db().unwrap().download(&stable_id).unwrap();
    assert_eq!(completed.state, DownloadState::Completed);
    assert_eq!(completed.attempts, 2);
    assert_eq!(completed.bytes, content.len() as u64);
    assert_eq!(fs::read(partial_path).unwrap(), content);
    assert_eq!(fs::read(root.join("cover.jpg")).unwrap(), b"cover");
    assert!(service.jobs.is_empty());
    let directories = service
        .session()
        .unwrap()
        .client
        .browse("uploader")
        .await
        .unwrap();
    let catalogue = RemoteCatalogue {
        username: "uploader".into(),
        directories: Arc::new(directories),
    };
    assert!(browsed_file(&catalogue, "another-user", "Album\\disc", "track.flac").is_err());
    assert!(browsed_file(&catalogue, "uploader", "Album\\disc", "missing.flac").is_err());
    let file = browsed_file(&catalogue, "uploader", "Album\\disc", "track.flac").unwrap();
    assert_eq!(file.size, content.len() as u64);
    assert_eq!(file.name, "Album\\disc\\track.flac");
    let destination = preferences::file_destination(Some(&root), &file.name, None).unwrap();
    enqueue_file(&mut service, "uploader", file, destination.clone()).unwrap();
    for active in service.jobs.values() {
        assert_eq!(
            timeout(DEADLINE, active.handle.wait()).await.unwrap().state,
            JobState::Completed
        );
    }
    service.sync_jobs().unwrap();
    assert_eq!(fs::read(destination).unwrap(), content);
    service.disconnect().await.unwrap();
    uploader.client.disconnect();
}

#[test]
fn credentials_round_trip_preserves_password_whitespace() {
    let credentials = Credentials {
        server: "localhost:2242".into(),
        username: "me".into(),
        password: "  exact password  ".into(),
    };
    let stored = serde_json::to_string(&credentials).unwrap();
    let restored: Credentials = serde_json::from_str(&stored).unwrap();
    assert_eq!(restored.password, "  exact password  ");
    let profile = Profile {
        server: restored.server,
        username: restored.username,
        password_saved: true,
    };
    assert!(!serde_json::to_string(&profile)
        .unwrap()
        .contains("exact password"));
}
