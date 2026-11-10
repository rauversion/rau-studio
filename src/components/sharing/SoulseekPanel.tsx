import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import { Download, FolderOpen, Globe2, LoaderCircle, Pause, Play, RefreshCcw, Save, Search, Square, Trash2, Wifi, WifiOff } from "lucide-react";
import { useCallback, useEffect, useMemo, useState, type FormEvent } from "react";
import { Button } from "../ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "../ui/card";
import { useI18n } from "../../i18n";
import { RemoteFileBrowser } from "./RemoteFileBrowser";
import { normalizeRemotePath, type RemoteDirectory, type RemoteFile } from "./remoteCatalogue";

type SavedSearch = { id: string; server: string; account: string; query: string; state: string; result_count: number; created_at: string; last_error: string | null };
type Hit = { id: string; username: string; file: RemoteFile; slots_free: boolean; average_speed: number; queue_length: number };
type Job = {
  id: string; batch_id: string | null; server: string; account: string; username: string;
  file: RemoteFile; destination: string; state: string; bytes: number; attempts: number; last_error: string | null;
};
type Snapshot = { connected: boolean; server: string | null; account: string | null; searches: SavedSearch[]; downloads: Job[]; last_error: string | null };
type Profile = { server: string; username: string; password_saved: boolean };
type DownloadSettings = { download_directory: string | null };
const defaultServer = "server.slsknet.org:2242";
const field = "h-9 w-full rounded-md border border-border bg-background px-3 text-sm outline-none focus:ring-2 focus:ring-ring disabled:opacity-50";
const nameOf = (path: string) => path.split(/[\\/]/).pop() || path;
const parentOf = (path: string) => path.replace(/[\\/][^\\/]+$/, "");
const failure = (error: unknown) => error instanceof Error ? error.message : String(error);
const bytes = (value: number) => {
  if (value < 1024) return `${value} B`;
  const index = Math.min(3, Math.floor(Math.log(value) / Math.log(1024)));
  return `${(value / 1024 ** index).toFixed(1)} ${["B", "KB", "MB", "GB"][index]}`;
};
const stateLabel: Record<string, string> = {
  queued: "En cola", running: "Descargando", retry_waiting: "Esperando reintento", paused: "En pausa",
  interrupted: "Interrumpido", completed: "Completado", cancelled: "Cancelado", failed: "Fallido",
  searching: "Buscando", stopped: "Detenido"
};

export function SoulseekPanel() {
  const { t } = useI18n();
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [server, setServer] = useState(defaultServer);
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [savedPassword, setSavedPassword] = useState(false);
  const [remember, setRemember] = useState(true);
  const [query, setQuery] = useState("");
  const [selectedSearch, setSelectedSearch] = useState("");
  const [results, setResults] = useState<Hit[]>([]);
  const [resultFilter, setResultFilter] = useState("");
  const [remoteUser, setRemoteUser] = useState("");
  const [directories, setDirectories] = useState<RemoteDirectory[]>([]);
  const [catalogueUser, setCatalogueUser] = useState("");
  const [catalogueAccount, setCatalogueAccount] = useState("");
  const [selectedFolder, setSelectedFolder] = useState("");
  const [downloadDirectory, setDownloadDirectory] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Poll persisted snapshots, so changing tabs never owns a search or transfer.
  useEffect(() => {
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      try {
        const next = await invoke<Snapshot>("soulseek_snapshot");
        if (!disposed) setSnapshot(next);
      } catch (cause) { if (!disposed) setError(failure(cause)); }
      if (!disposed) timer = setTimeout(() => void poll(), 1500);
    }
    void poll();
    void Promise.all([invoke<Profile>("soulseek_profile"), invoke<Snapshot>("soulseek_snapshot"), invoke<DownloadSettings>("soulseek_download_settings")]).then(([profile, current, downloads]) => {
      if (disposed) return;
      setServer(current.server || profile.server || defaultServer); setUsername(current.account || profile.username); setSavedPassword(profile.password_saved);
      setDownloadDirectory(downloads.download_directory);
    }).catch((cause) => { if (!disposed) setError(failure(cause)); });
    return () => { disposed = true; clearTimeout(timer); };
  }, []);

  useEffect(() => {
    if (!selectedSearch && snapshot?.searches.length) setSelectedSearch(snapshot.searches[0].id);
  }, [selectedSearch, snapshot?.searches]);

  useEffect(() => {
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    setResults([]);
    if (!selectedSearch) return;
    async function poll() {
      try {
        const hits = await invoke<Hit[]>("soulseek_search_results", { searchId: selectedSearch });
        if (!disposed) setResults(hits);
      } catch (cause) { if (!disposed) setError(failure(cause)); }
      if (!disposed) timer = setTimeout(() => void poll(), 1500);
    }
    void poll();
    return () => { disposed = true; clearTimeout(timer); };
  }, [selectedSearch]);

  const run = useCallback(async (action: () => Promise<void>) => {
    setBusy(true); setError(null);
    try { await action(); setSnapshot(await invoke<Snapshot>("soulseek_snapshot")); }
    catch (cause) { setError(failure(cause)); }
    finally { setBusy(false); }
  }, []);

  const activeSearch = snapshot?.searches.find((search) => search.id === selectedSearch);
  const matchingAccount = !activeSearch || (activeSearch.server === snapshot?.server && activeSearch.account === snapshot?.account);
  const visibleResults = useMemo(() => {
    const filter = resultFilter.toLowerCase();
    return results.filter((hit) => `${hit.username} ${hit.file.path} ${hit.file.extension}`.toLowerCase().includes(filter));
  }, [results, resultFilter]);
  const accountKey = `${snapshot?.server}/${snapshot?.account}`;
  const connected = snapshot?.connected === true;

  function connect(event: FormEvent) {
    event.preventDefault();
    void run(async () => {
      setSnapshot(await invoke<Snapshot>("soulseek_connect", { server, username, password: password || null, remember }));
      setPassword(""); setSavedPassword(remember);
    });
  }

  function search(event: FormEvent) {
    event.preventDefault();
    void run(async () => {
      const created = await invoke<SavedSearch>("soulseek_search", { query });
      setSelectedSearch(created.id);
    });
  }

  async function browseUser(user: string, folder?: string) {
    await run(async () => {
      const catalogue = await invoke<RemoteDirectory[]>("soulseek_browse", { username: user });
      setDirectories(catalogue); setCatalogueUser(user); setRemoteUser(user); setCatalogueAccount(accountKey);
      setSelectedFolder(normalizeRemotePath(folder || ""));
    });
  }

  async function selectDownloadDirectory() {
    await run(async () => {
      const directory = await open({ directory: true, multiple: false, title: t("Carpeta de descarga"), defaultPath: downloadDirectory || undefined });
      if (typeof directory !== "string") return;
      const settings = await invoke<DownloadSettings>("soulseek_save_download_directory", { directory });
      setDownloadDirectory(settings.download_directory);
    });
  }

  async function fileDestination(file: RemoteFile, chooseDestination: boolean) {
    if (downloadDirectory && !chooseDestination) return null;
    const filename = nameOf(file.path).replace(/[:\0]/g, "_");
    return save({ title: t("Guardar descarga"), defaultPath: downloadDirectory ? `${downloadDirectory.replace(/[\\/]$/, "")}/${filename}` : filename });
  }

  async function downloadFile(hit: Hit, chooseDestination = false) {
    await run(async () => {
      const destination = await fileDestination(hit.file, chooseDestination);
      if (!destination && (!downloadDirectory || chooseDestination)) return;
      setSnapshot(await invoke<Snapshot>("soulseek_download_file", { resultId: hit.id, destination }));
    });
  }

  async function downloadBrowsedFile(directory: string, file: RemoteFile, chooseDestination = false) {
    await run(async () => {
      const destination = await fileDestination(file, chooseDestination);
      if (!destination && (!downloadDirectory || chooseDestination)) return;
      setSnapshot(await invoke<Snapshot>("soulseek_download_browsed_file", { username: catalogueUser, directory, filename: file.path, destination }));
    });
  }

  async function downloadFolder(chooseDestination = false) {
    await run(async () => {
      let destination: string | null = null;
      if (!downloadDirectory || chooseDestination) {
        const selected = await open({ directory: true, multiple: false, title: t("Guardar el contenido de la carpeta en…"), defaultPath: downloadDirectory || undefined });
        if (typeof selected !== "string") return;
        destination = selected;
      }
      setSnapshot(await invoke<Snapshot>("soulseek_download_folder", { username: catalogueUser, folder: selectedFolder, destination }));
    });
  }

  function jobAction(job: Job, action: string) {
    void run(async () => { setSnapshot(await invoke<Snapshot>("soulseek_download_action", { downloadId: job.id, action })); });
  }

  return (
    <main className="min-h-screen bg-background p-4 text-foreground lg:p-6">
      <div className="mx-auto grid max-w-[1480px] gap-4">
        <header className="flex flex-wrap items-start justify-between gap-3">
          <div>
            <div className="flex items-center gap-2 text-xs font-semibold uppercase tracking-[0.18em] text-muted-foreground"><Globe2 className="h-4 w-4" />Soulseek</div>
            <h1 className="mt-1 text-2xl font-semibold tracking-tight">{t("Buscar y descargar música")}</h1>
            <p className="mt-1 text-sm text-muted-foreground">{t("Busca archivos, explora carpetas y administra tus descargas.")}</p>
          </div>
          <span className="flex items-center gap-2 rounded-full border border-border px-3 py-1.5 text-xs">
            {connected ? <Wifi className="h-4 w-4 text-emerald-500" /> : <WifiOff className="h-4 w-4 text-muted-foreground" />}
            {connected ? `${snapshot.account} · ${snapshot.server}` : t("Desconectado")}
          </span>
        </header>
        {(error || snapshot?.last_error) && <div role="alert" className="rounded-md border border-destructive/30 bg-destructive/10 p-3 text-sm text-destructive">{error || snapshot?.last_error}</div>}

        <Card>
          <CardHeader><CardTitle>{t("Cuenta de Soulseek")}</CardTitle>{busy && <LoaderCircle aria-label={t("Cargando")} className="h-4 w-4 animate-spin" />}</CardHeader>
          <CardContent className="p-3">
            <form onSubmit={connect} className="grid items-end gap-3 md:grid-cols-[1fr_1fr_1fr_auto]">
              <label className="grid gap-1 text-xs text-muted-foreground">{t("Servidor y puerto")}<input className={field} value={server} onChange={(e) => setServer(e.target.value)} placeholder={defaultServer} disabled={connected || busy} required autoCapitalize="none" autoCorrect="off" /></label>
              <label className="grid gap-1 text-xs text-muted-foreground">{t("Usuario")}<input className={field} value={username} onChange={(e) => setUsername(e.target.value)} disabled={connected || busy} required autoComplete="username" autoCapitalize="none" autoCorrect="off" /></label>
              <label className="grid gap-1 text-xs text-muted-foreground">{t("Contraseña")}<input className={field} type="password" value={password} onChange={(e) => setPassword(e.target.value)} placeholder={savedPassword ? t("Contraseña guardada") : ""} disabled={connected || busy} autoComplete="current-password" /></label>
              {connected ? <Button type="button" variant="secondary" disabled={busy} onClick={() => void run(async () => { setSnapshot(await invoke<Snapshot>("soulseek_disconnect")); })}>{t("Desconectar")}</Button> : <Button disabled={busy} type="submit"><Wifi className="h-4 w-4" />{t("Conectar")}</Button>}
              <div className="flex flex-wrap items-center gap-4 md:col-span-4">
                <label className="flex items-center gap-2 text-xs text-muted-foreground"><input type="checkbox" checked={remember} disabled={connected || busy} onChange={(e) => setRemember(e.target.checked)} />{t("Recordar credenciales en este equipo")}</label>
                {savedPassword && <Button size="sm" variant="ghost" type="button" disabled={busy} onClick={() => void run(async () => { await invoke("soulseek_forget_credentials"); setSavedPassword(false); })}><Trash2 className="h-3.5 w-3.5" />{t("Olvidar credenciales")}</Button>}
              </div>
            </form>
          </CardContent>
        </Card>

        <Card>
          <CardHeader><CardTitle>{t("Carpeta de descarga")}</CardTitle><FolderOpen className="h-4 w-4 text-muted-foreground" /></CardHeader>
          <CardContent className="grid gap-2 p-3">
            <div className="flex flex-wrap items-center gap-2">
              <input className={`${field} min-w-0 flex-1`} readOnly value={downloadDirectory || ""} placeholder={t("Preguntar al descargar")} aria-label={t("Carpeta de descarga")} title={downloadDirectory || undefined} />
              <Button variant="secondary" disabled={busy} onClick={() => void selectDownloadDirectory()}><FolderOpen className="h-4 w-4" />{t("Elegir carpeta")}</Button>
              {downloadDirectory && <Button variant="ghost" disabled={busy} onClick={() => void run(async () => { const settings = await invoke<DownloadSettings>("soulseek_save_download_directory", { directory: null }); setDownloadDirectory(settings.download_directory); })}>{t("Preguntar cada vez")}</Button>}
            </div>
            <p className="text-xs text-muted-foreground">{t("Se guarda para nuevas descargas. Cada carpeta remota tendrá su propio subdirectorio; las descargas existentes mantienen su destino.")}</p>
          </CardContent>
        </Card>

        <div className="grid items-start gap-4 xl:grid-cols-[minmax(0,1fr)_minmax(380px,0.85fr)]">
          <Card className="min-w-0">
            <CardHeader><CardTitle>{t("Búsquedas")}</CardTitle><span className="text-xs text-muted-foreground">{t("El historial se conserva al cerrar.")}</span></CardHeader>
            <CardContent className="grid gap-3 p-3">
              <form className="flex gap-2" onSubmit={search}>
                <input aria-label={t("Buscar en Soulseek")} className={field} placeholder={t("Artista, álbum o canción")} value={query} onChange={(e) => setQuery(e.target.value)} minLength={2} maxLength={256} required />
                <Button disabled={!connected || busy}><Search className="h-4 w-4" />{t("Buscar")}</Button>
              </form>
              <div className="flex flex-wrap items-center gap-2">
                <select aria-label={t("Historial de búsquedas")} className={`${field} min-w-0 flex-1`} value={selectedSearch} onChange={(e) => setSelectedSearch(e.target.value)}>
                  <option value="">{t("Selecciona una búsqueda")}</option>
                  {snapshot?.searches.map((s) => <option key={s.id} value={s.id}>{s.query} · {s.result_count} · {t(stateLabel[s.state] || s.state)} · {s.account}</option>)}
                </select>
                {activeSearch?.state === "searching" && <Button size="sm" variant="secondary" disabled={busy} onClick={() => void run(async () => { await invoke("soulseek_stop_search", { searchId: selectedSearch }); })}><Square className="h-3 w-3" />{t("Detener")}</Button>}
                {activeSearch && <Button aria-label={t("Eliminar búsqueda")} size="icon" variant="ghost" disabled={busy} onClick={() => void run(async () => { await invoke("soulseek_delete_search", { searchId: selectedSearch }); const next = await invoke<Snapshot>("soulseek_snapshot"); setSnapshot(next); setSelectedSearch(next.searches[0]?.id || ""); setResults([]); })}><Trash2 className="h-4 w-4" /></Button>}
              </div>
              {activeSearch && <div className="flex items-center justify-between gap-2 text-xs text-muted-foreground"><span>{activeSearch.server} · {new Date(activeSearch.created_at).toLocaleString()}</span><span>{results.length} / 5000</span></div>}
              {activeSearch?.last_error && <p role="alert" className="text-xs text-destructive">{activeSearch.last_error}</p>}
              <input className={field} value={resultFilter} onChange={(e) => setResultFilter(e.target.value)} aria-label={t("Filtrar resultados")} placeholder={t("Filtrar por usuario, ruta o formato")} />
              <div className="max-h-[480px] overflow-auto rounded-md border border-border">
                <table className="w-full text-left text-xs">
                  <thead className="sticky top-0 bg-card text-muted-foreground"><tr><th className="p-2">{t("Archivo")}</th><th className="p-2">{t("Usuario")}</th><th className="p-2">{t("Tamaño")}</th><th className="p-2">{t("Disponibilidad")}</th><th className="p-2"><span className="sr-only">{t("Acciones")}</span></th></tr></thead>
                  <tbody>
                    {visibleResults.slice(0, 250).map((hit) => <tr key={hit.id} className="border-t border-border hover:bg-muted/30">
                      <td className="max-w-[320px] p-2"><div className="truncate font-medium" title={hit.file.path}>{nameOf(hit.file.path)}</div><div className="truncate text-muted-foreground" title={parentOf(hit.file.path)}>{parentOf(hit.file.path)}</div></td>
                      <td className="p-2">{hit.username}</td><td className="whitespace-nowrap p-2">{bytes(hit.file.size)}</td>
                      <td className="p-2 text-muted-foreground">{hit.slots_free ? t("Libre") : t("Cola: {count}", { count: hit.queue_length })}<div>{bytes(hit.average_speed)}/s</div></td>
                      <td className="p-2"><div className="flex gap-1"><Button title={t("Explorar carpetas")} aria-label={t("Explorar carpetas")} size="icon" variant="ghost" disabled={!connected || !matchingAccount || busy} onClick={() => void browseUser(hit.username, parentOf(hit.file.path))}><FolderOpen className="h-4 w-4" /></Button><Button title={t("Descargar archivo")} aria-label={t("Descargar archivo")} size="icon" variant="ghost" disabled={!connected || !matchingAccount || busy} onClick={() => void downloadFile(hit)}><Download className="h-4 w-4" /></Button>{downloadDirectory && <Button title={t("Guardar como…")} aria-label={t("Guardar como…")} size="icon" variant="ghost" disabled={!connected || !matchingAccount || busy} onClick={() => void downloadFile(hit, true)}><Save className="h-4 w-4" /></Button>}</div></td>
                    </tr>)}
                  </tbody>
                </table>
                {!visibleResults.length && <p className="p-8 text-center text-sm text-muted-foreground">{t(activeSearch?.state === "searching" ? "Esperando resultados de Soulseek…" : "Los resultados guardados aparecerán aquí.")}</p>}
              </div>
              {visibleResults.length > 250 && <p className="text-xs text-muted-foreground">{t("Mostrando 250 de {count}. Filtra para ver otros resultados.", { count: visibleResults.length })}</p>}
            </CardContent>
          </Card>

          <Card className="min-w-0">
            <CardHeader><CardTitle>{t("Carpetas remotas")}</CardTitle><FolderOpen className="h-4 w-4 text-muted-foreground" /></CardHeader>
            <CardContent className="grid gap-3 p-3">
              <form className="flex gap-2" onSubmit={(event) => { event.preventDefault(); void browseUser(remoteUser); }}>
                <input className={field} value={remoteUser} onChange={(e) => setRemoteUser(e.target.value)} placeholder={t("Usuario remoto")} aria-label={t("Usuario remoto")} required />
                <Button type="submit" size="icon" title={t("Explorar carpetas")} aria-label={t("Explorar carpetas")} disabled={!connected || busy}><Search className="h-4 w-4" /></Button>
              </form>
              <RemoteFileBrowser directories={directories} username={catalogueUser} path={selectedFolder} disabled={!connected || busy || catalogueAccount !== accountKey} hasDefaultDirectory={!!downloadDirectory} onNavigate={setSelectedFolder} onDownload={(directory, file, saveAs) => void downloadBrowsedFile(directory, file, saveAs)} />
              {selectedFolder && <p className="break-all text-xs text-muted-foreground">{catalogueUser} · {selectedFolder}</p>}
              <div className="flex flex-wrap gap-2"><Button disabled={!connected || !selectedFolder || busy || catalogueAccount !== accountKey} onClick={() => void downloadFolder()}><Download className="h-4 w-4" />{t("Descargar carpeta completa")}</Button>{downloadDirectory && <Button variant="secondary" disabled={!connected || !selectedFolder || busy || catalogueAccount !== accountKey} onClick={() => void downloadFolder(true)}>{t("Elegir otro destino")}</Button>}</div>
              <p className="text-xs text-muted-foreground">{t(downloadDirectory ? "Incluye todas las subcarpetas y conserva el nombre de la carpeta remota." : "Incluye subcarpetas. Los archivos se guardan dentro de la carpeta que elijas.")}</p>
            </CardContent>
          </Card>
        </div>

        <Card>
          <CardHeader><CardTitle>{t("Descargas")} <span className="ml-1 text-muted-foreground">{snapshot?.downloads.length || 0}</span></CardTitle><span className="text-xs text-muted-foreground">{t("4 simultáneas · 1 por usuario")}</span></CardHeader>
          <CardContent>
            {!snapshot?.downloads.length && <p className="p-8 text-center text-sm text-muted-foreground">{t("Las descargas y los archivos parciales se conservan al cerrar.")}</p>}
            {snapshot?.downloads.map((job) => {
              const live = ["queued", "running", "retry_waiting"].includes(job.state);
              const terminal = ["completed", "cancelled", "failed"].includes(job.state);
              const canResume = ["paused", "interrupted", "failed", "cancelled"].includes(job.state);
              const sameAccount = job.server === snapshot.server && job.account === snapshot.account;
              return <div key={job.id} className="grid gap-3 border-b border-border p-3 last:border-b-0 md:grid-cols-[minmax(0,1fr)_180px_auto]">
                <div className="min-w-0"><div className="truncate text-sm font-medium" title={job.file.path}>{nameOf(job.file.path)}</div><div className="truncate text-xs text-muted-foreground" title={job.destination}>{job.username} → {job.destination}</div><div className="mt-1 text-xs text-muted-foreground">{job.account} · {job.server}{job.batch_id ? ` · ${t("Carpeta")}` : ""}</div>{job.last_error && <p role="alert" className="mt-1 break-words text-xs text-destructive">{job.last_error}</p>}</div>
                <div className="grid content-center gap-1 text-xs"><div className="flex justify-between gap-2"><span>{t(stateLabel[job.state] || job.state)}</span><span>{job.attempts} {t("intentos")}</span></div><progress aria-label={`${t("Descarga")}: ${nameOf(job.file.path)}`} className="h-1.5 w-full accent-primary" max={Math.max(1, job.file.size)} value={job.bytes} /><span className="text-muted-foreground">{bytes(job.bytes)} / {bytes(job.file.size)}</span></div>
                <div className="flex items-center gap-1">
                  {live && <Button size="icon" variant="ghost" disabled={busy} title={t("Pausar")} aria-label={t("Pausar")} onClick={() => jobAction(job, "pause")}><Pause className="h-4 w-4" /></Button>}
                  {canResume && <Button size="icon" variant="ghost" disabled={busy || !connected || !sameAccount} title={t(job.state === "failed" ? "Reintentar" : "Reanudar")} aria-label={t(job.state === "failed" ? "Reintentar" : "Reanudar")} onClick={() => jobAction(job, job.state === "failed" ? "retry" : "resume")}>{job.state === "failed" ? <RefreshCcw className="h-4 w-4" /> : <Play className="h-4 w-4" />}</Button>}
                  {!terminal && <Button size="icon" variant="ghost" disabled={busy} title={t("Cancelar")} aria-label={t("Cancelar")} onClick={() => jobAction(job, "cancel")}><Square className="h-4 w-4" /></Button>}
                  {terminal && <Button size="icon" variant="ghost" disabled={busy} title={t("Quitar del historial")} aria-label={t("Quitar del historial")} onClick={() => jobAction(job, "forget")}><Trash2 className="h-4 w-4" /></Button>}
                </div>
              </div>;
            })}
            {snapshot?.downloads.some((job) => ["interrupted", "paused"].includes(job.state)) && <p className="p-3 text-xs text-muted-foreground">{t("Conecta la cuenta original y pulsa Reanudar. Se comprobará el parcial antes de continuar.")}</p>}
          </CardContent>
        </Card>
      </div>
    </main>
  );
}
