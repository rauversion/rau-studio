import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { AlertTriangle, ArrowLeft, CheckCircle2, FolderOpen, HardDrive, ListMusic, LoaderCircle, RefreshCcw, Search, Usb } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { Link, useSearchParams } from "react-router-dom";
import { Button } from "./components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "./components/ui/card";
import { translateBackendMessage, useI18n } from "./i18n";
import { cn } from "./lib/utils";
import { UnavailableTrackList, UnavailableTracksDialog, type UnavailableTrack } from "./components/usb/UnavailableTracksDialog";
import { TerminalDrawer, type TerminalLogEntry } from "./components/terminal-drawer";

type PlaylistOption = {
  id: string;
  target_kind: "indexed" | "draft";
  library_id: string;
  library_name: string;
  playlist_path?: string | null;
  name: string;
  track_count: number;
};

type ExportPreview = {
  plan_id: string;
  destination_path: string;
  playlist_ids: string[];
  playlist_names: string[];
  track_count: number;
  copied_tracks: number;
  converted_tracks: number;
  source_bytes: number;
  estimated_audio_bytes: number | null;
  without_grid: number;
  with_cues: number;
  warning_count: number;
  warnings: { track_title: string; message: string }[];
  unavailable_tracks: UnavailableTrack[];
  empty_playlists: string[];
  updating: boolean;
  sync_counts: { new_tracks: number; reused_tracks: number; updated_tracks: number; analysis_tracks: number; preserved_playlists: number } | null;
  workers: number;
};

type ExportJob = {
  id: string;
  status: "running" | "completed" | "failed";
  progress: { stage: "audio" | "analysis" | "library"; completed_tracks: number; total_tracks: number; current_track: string | null };
  preview: ExportPreview;
  output_path: string;
  report_path: string | null;
  error: string | null;
  logs: { id: number; time: string; level: string; name: string | null; message: string }[];
};

type DestinationInfo = {
  path: string; existing: boolean; can_update: boolean; reason: string | null; track_count: number; revision: string; recovery_available: boolean;
  playlists: { id: number; name: string; path: string; track_count: number; source_id: string | null }[];
};

type StartResult = { status: "started"; job: ExportJob } | { status: "review_required"; preview: ExportPreview };

export function UsbExportPage() {
  const { locale, t } = useI18n();
  const [params] = useSearchParams();
  const initialSelection = useRef({ library: params.get("library"), path: params.get("playlist"), draft: params.get("draft") });
  const [options, setOptions] = useState<PlaylistOption[]>([]);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [query, setQuery] = useState("");
  const [loading, setLoading] = useState(true);
  const [preparing, setPreparing] = useState(false);
  const [starting, setStarting] = useState(false);
  const [choosing, setChoosing] = useState(false);
  const [preview, setPreview] = useState<ExportPreview | null>(null);
  const [unavailableOpen, setUnavailableOpen] = useState(false);
  const [acceptedOmissions, setAcceptedOmissions] = useState<string | null>(null);
  const [destinationPath, setDestinationPath] = useState("");
  const [destinationInfo, setDestinationInfo] = useState<DestinationInfo | null>(null);
  const [inspecting, setInspecting] = useState(false);
  const [workers, setWorkers] = useState(2);
  const [terminalLogs, setTerminalLogs] = useState<TerminalLogEntry[]>([]);
  const [terminalExpanded, setTerminalExpanded] = useState(false);
  const terminalRef = useRef<HTMLDivElement>(null);
  const nextLog = useRef(1);
  const seenLogs = useRef({ job: "", id: 0 });
  const autoSelected = useRef(new Set<string>());
  const destinationEpoch = useRef(0);
  const refreshedJob = useRef("");
  const runningJobs = useRef(new Set<string>());
  const [job, setJob] = useState<ExportJob | null>(null);
  const [statusLoaded, setStatusLoaded] = useState(false);
  const [statusError, setStatusError] = useState(false);
  const [error, setError] = useState("");
  const mounted = useRef(false);
  const actionEpoch = useRef(0);
  const restoredJob = useRef("");
  const loadEpoch = useRef(0);
  const running = starting || job?.status === "running";
  const locked = running || preparing || choosing || inspecting || loading || !statusLoaded || statusError;
  const existingSources = useMemo(() => new Set(destinationInfo?.playlists.flatMap((p) => p.source_id ? [p.source_id] : []) ?? []), [destinationInfo]);
  const visible = useMemo(() => {
    const search = query.trim().toLocaleLowerCase();
    return options.filter((p) => `${p.name} ${p.library_name} ${p.playlist_path ?? ""}`.toLocaleLowerCase().includes(search));
  }, [options, query]);
  const selectable = visible.filter((p) => p.track_count > 0 || destinationInfo?.existing);
  const allVisibleSelected = selectable.length > 0 && selectable.every((p) => selected.has(p.id));
  const summary = running && job ? job.preview : preview;
  const percent = job ? (job.status === "completed" ? 100 : Math.min(99, Math.floor(job.progress.completed_tracks / Math.max(1, job.progress.total_tracks) * 100))) : 0;
  const actionDisabled = locked || selected.size === 0 || !destinationPath || !destinationInfo?.can_update || preview?.track_count === 0;
  const actionLabel = running ? "Exportación en curso…" : preparing ? "Revisando archivos y análisis…"
    : preview?.track_count === 0 ? "No hay temas disponibles" : preview?.updating ? "Actualizar pendrive" : preview ? "Preparar exportación" : "Revisar y continuar";
  const actionHint = running ? "Ya hay una exportación en curso."
    : preparing ? "Revisando archivos y análisis…"
    : choosing ? "Elige una carpeta en la ventana de selección."
    : inspecting ? "Leyendo la biblioteca del pendrive…"
    : statusError ? "No se pudo consultar el progreso. Reintentando la conexión…"
    : !statusLoaded ? "Consultando el estado de la exportación…"
    : loading ? "Cargando playlists…"
    : selected.size === 0 ? "Selecciona al menos una playlist para continuar."
    : !destinationPath ? "Elige una carpeta de destino."
    : !destinationInfo ? "Vuelve a elegir la carpeta para leer su contenido."
    : !destinationInfo.can_update ? "Este destino no está disponible para actualizar. Revisa el aviso del pendrive."
    : preview?.track_count === 0 ? "No hay temas disponibles para exportar. Corrige las rutas o conecta el disco y vuelve a revisar."
    : !preview ? "Pulsa «Revisar y continuar» para ver los temas, el tamaño estimado y los avisos."
    : preview.updating ? "Revisa el resumen y pulsa «Actualizar pendrive» para comenzar."
    : "Revisa el resumen y pulsa «Preparar exportación» para comenzar.";

  function formatBytes(bytes: number) {
    const units = ["B", "KiB", "MiB", "GiB", "TiB"];
    const unit = Math.min(units.length - 1, Math.max(0, Math.floor(Math.log(Math.max(1, bytes)) / Math.log(1024))));
    return `${new Intl.NumberFormat(locale, { maximumFractionDigits: 1 }).format(bytes / 1024 ** unit)} ${units[unit]}`;
  }

  function appendLog(level: TerminalLogEntry["level"], message: string, name?: string, time = new Date().toLocaleTimeString()) {
    const entry: TerminalLogEntry = { id: nextLog.current++, time, level, message, name };
    setTerminalLogs((current) => [...current, entry].slice(-1000));
  }

  async function inspectDestination(path: string, selectExisting = true) {
    const epoch = ++destinationEpoch.current;
    setInspecting(true);
    setPreview(null); setAcceptedOmissions(null); setUnavailableOpen(false); setError("");
    appendLog("info", `${t("Leyendo la biblioteca del pendrive…")} ${path}`);
    try {
      const info = await invoke<DestinationInfo>("usb_export_inspect_destination", { destinationPath: path });
      if (!mounted.current || epoch !== destinationEpoch.current) return;
      setDestinationPath(info.path); setDestinationInfo(info);
      if (selectExisting) {
        const currentIds = new Set(info.playlists.flatMap((p) => p.source_id ? [p.source_id] : []));
        const previousAuto = autoSelected.current;
        setSelected((current) => new Set([...current].filter((id) => !previousAuto.has(id)).concat([...currentIds])));
        autoSelected.current = currentIds;
      }
      appendLog("info", t("USB: {playlists} playlists y {tracks} pistas existentes.", { playlists: info.playlists.length, tracks: info.track_count }));
      if (info.reason) appendLog("warning", info.reason);
    } catch (cause) {
      if (mounted.current && epoch === destinationEpoch.current) { setDestinationInfo(null); setError(String(cause)); appendLog("error", String(cause)); }
    } finally { if (mounted.current && epoch === destinationEpoch.current) setInspecting(false); }
  }

  async function recoverDestination() {
    setInspecting(true); setError("");
    try {
      const info = await invoke<DestinationInfo>("usb_export_recover", { destinationPath });
      if (mounted.current) {
        setDestinationInfo(info); setPreview(null); setAcceptedOmissions(null);
        const currentIds = new Set(info.playlists.flatMap((p) => p.source_id ? [p.source_id] : []));
        setSelected((current) => new Set([...current, ...currentIds])); autoSelected.current = currentIds;
        appendLog("info", t("Biblioteca anterior restaurada."));
      }
    } catch (cause) { if (mounted.current) { setError(String(cause)); appendLog("error", String(cause)); } }
    finally { if (mounted.current) setInspecting(false); }
  }

  useEffect(() => {
    if (!job) return;
    if (job.status === "running") runningJobs.current.add(job.id);
    if (seenLogs.current.job !== job.id) seenLogs.current = { job: job.id, id: 0 };
    const unseen = job.logs.filter((line) => line.id > seenLogs.current.id);
    if (unseen.length > 0) {
      const entries: TerminalLogEntry[] = unseen.map((line) => ({ id: nextLog.current++, time: new Date(line.time).toLocaleTimeString(), level: line.level === "error" ? "error" : line.level === "warning" ? "warning" : "info", name: line.name ?? undefined, message: line.message }));
      setTerminalLogs((current) => [...current, ...entries].slice(-1000));
      seenLogs.current.id = unseen[unseen.length - 1].id;
    }
    if (job.status !== "running" && runningJobs.current.has(job.id) && refreshedJob.current !== job.id) {
      refreshedJob.current = job.id;
      void inspectDestination(job.output_path, false);
    }
  }, [job]);

  useEffect(() => { if (terminalExpanded && terminalRef.current) terminalRef.current.scrollTop = terminalRef.current.scrollHeight; }, [terminalLogs, terminalExpanded]);

  async function loadOptions() {
    const epoch = ++loadEpoch.current;
    setLoading(true);
    setError("");
    setPreview(null);
    setAcceptedOmissions(null);
    setUnavailableOpen(false);
    try {
      const rows = await invoke<PlaylistOption[]>("playlist_index_playlist_targets");
      if (!mounted.current || epoch !== loadEpoch.current) return;
      setOptions(rows);
      setSelected((current) => {
        const initial = initialSelection.current;
        const next = new Set([...current].filter((id) => rows.some((p) => p.id === id && (p.track_count > 0 || destinationInfo?.existing))));
        for (const row of rows) {
          if (!restoredJob.current && row.track_count > 0 && ((initial.draft && row.id === initial.draft)
            || (initial.path && row.library_id === initial.library && row.playlist_path === initial.path))) next.add(row.id);
        }
        initialSelection.current = { library: null, path: null, draft: null };
        return next;
      });
    } catch (cause) {
      if (mounted.current && epoch === loadEpoch.current) setError(String(cause));
    } finally {
      if (mounted.current && epoch === loadEpoch.current) setLoading(false);
    }
  }

  useEffect(() => {
    mounted.current = true;
    void loadOptions();
    let active = true;
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      const epoch = actionEpoch.current;
      try {
        const current = await invoke<ExportJob | null>("usb_export_status");
        if (!active || epoch !== actionEpoch.current) return;
        setJob(current);
        setStatusLoaded(true);
        setStatusError(false);
        if (current?.status === "running" && restoredJob.current !== current.id) {
          restoredJob.current = current.id;
          setSelected(new Set(current.preview.playlist_ids));
          setDestinationPath(current.output_path);
        }
      } catch {
        if (active && epoch === actionEpoch.current) setStatusError(true);
      } finally {
        if (active) timer = setTimeout(() => void poll(), 1000);
      }
    }
    void poll();
    return () => { mounted.current = false; active = false; clearTimeout(timer); };
  }, []);

  function changeSelection(next: Set<string>) {
    setSelected(next);
    setPreview(null);
    setAcceptedOmissions(null);
    setUnavailableOpen(false);
    setError("");
  }

  async function reviewSelection() {
    if (locked || selected.size === 0 || !destinationPath) return;
    setPreparing(true);
    setPreview(null);
    setAcceptedOmissions(null);
    setUnavailableOpen(false);
    setError("");
    try {
      // The backend holds this exact snapshot until export starts.
      appendLog("info", t("Revisando archivos, análisis y contenido existente del USB…"));
      const result = await invoke<ExportPreview>("usb_export_prepare", { playlistIds: [...selected], destinationPath, workers });
      if (mounted.current) {
        setPreview(result); setDestinationPath(result.destination_path); setUnavailableOpen(result.unavailable_tracks.length > 0);
        appendLog("info", t("Revisión lista: {tracks} pistas, {workers} temas en paralelo.", { tracks: result.track_count, workers: result.workers }));
        result.unavailable_tracks.forEach((track) => appendLog("warning", `${track.source_path ?? ""} · ${track.problem.kind} · ${track.problem.detail ?? ""}`, track.title));
        if (result.sync_counts) appendLog("info", t("{reused} pistas reutilizadas, {new} nuevas y {updated} actualizadas.", { reused: result.sync_counts.reused_tracks, new: result.sync_counts.new_tracks, updated: result.sync_counts.updated_tracks }));
      }
    } catch (cause) {
      if (mounted.current) { setError(String(cause)); appendLog("error", String(cause)); }
    } finally {
      if (mounted.current) setPreparing(false);
    }
  }

  async function chooseFolder() {
    setChoosing(true);
    setError("");
    try {
      const path = await open({ directory: true, multiple: false, title: t("Elegir carpeta de destino") });
      if (mounted.current && typeof path === "string") {
        setDestinationPath(path);
        setPreview(null);
        setAcceptedOmissions(null);
        setUnavailableOpen(false);
        await inspectDestination(path);
      }
    } catch (cause) {
      if (mounted.current) setError(String(cause));
    } finally {
      if (mounted.current) setChoosing(false);
    }
  }

  async function startExport() {
    if (locked || !preview || !destinationPath) return;
    if (preview.unavailable_tracks.length > 0 && acceptedOmissions !== preview.plan_id) {
      setUnavailableOpen(true);
      return;
    }
    setStarting(true);
    setError("");
    actionEpoch.current += 1;
    try {
      const result = await invoke<StartResult>("usb_export_start", { planId: preview.plan_id, skipUnavailable: acceptedOmissions === preview.plan_id });
      if (mounted.current) {
        if (result.status === "review_required") {
          setPreview(result.preview); setAcceptedOmissions(null); setUnavailableOpen(true);
        } else {
          restoredJob.current = result.job.id; setJob(result.job); setPreview(null); setUnavailableOpen(false); setAcceptedOmissions(null);
        }
      }
    } catch (cause) {
      if (mounted.current) { setError(String(cause)); setPreview(null); setAcceptedOmissions(null); appendLog("error", String(cause)); }
    } finally {
      actionEpoch.current += 1;
      if (mounted.current) setStarting(false);
    }
  }

  function cancelReview() {
    setUnavailableOpen(false);
    setAcceptedOmissions(null);
    setPreview(null);
    setError("");
    appendLog("info", t("Revisión cancelada; no se inició la exportación."));
  }

  async function reveal(path: string) {
    try { await invoke("reveal_path", { path }); }
    catch (cause) { if (mounted.current) setError(String(cause)); }
  }

  return (
    <main className={cn("min-w-0 p-4", terminalExpanded ? "pb-72" : "pb-20")}>
      <header className="mb-5 flex flex-wrap items-start justify-between gap-3 border-b border-border pb-4">
        <div>
          <Link to="/playlists" className="mb-3 inline-flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground"><ArrowLeft className="h-3.5 w-3.5" />{t("Volver a playlists")}</Link>
          <h1 className="flex items-center gap-2 text-2xl font-semibold"><Usb className="h-6 w-6" />{t("Exportar playlists")}</h1>
          <p className="mt-2 max-w-2xl text-sm text-muted-foreground">{t("Exporta tus playlists y su música directamente a la carpeta o al pendrive que elijas.")}</p>
        </div>
        <span className="rounded-full border border-amber-500/30 bg-amber-500/10 px-3 py-1 text-xs font-medium text-amber-700 dark:text-amber-300">{t("Experimental")}</span>
      </header>

      <div className="mb-4 rounded-lg border border-border bg-muted/40 p-3 text-xs leading-relaxed text-muted-foreground">
        {t("Genera Device Library y OneLibrary directamente desde Rau. Prueba funcional en XDJ-1000MK2; la validación en equipos OneLibrary está pendiente.")}
      </div>
      {error && <div role="alert" className="mb-4 break-words rounded-lg border border-red-500/30 bg-red-500/10 p-3 text-sm text-red-700 dark:text-red-300">{translateBackendMessage(locale, error)}</div>}
      {statusError && <div role="alert" className="mb-4 rounded-lg border border-amber-500/30 p-3 text-sm">{t("No se pudo consultar el progreso. Reintentando la conexión…")}</div>}

      {job && <Card className="mb-5" aria-label={t("Estado de la exportación")}>
        <CardContent className="space-y-3 p-4">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div className="flex min-w-0 items-center gap-3">
              {job.status === "running" ? <LoaderCircle className="h-5 w-5 shrink-0 animate-spin" /> : job.status === "completed" ? <CheckCircle2 className="h-5 w-5 shrink-0 text-emerald-600" /> : <AlertTriangle className="h-5 w-5 shrink-0 text-amber-600" />}
              <div className="min-w-0" role="status" aria-live="polite">
                <h2 className="text-sm font-semibold">{t(job.status === "completed" ? "Exportación completada" : job.status === "failed" ? "Exportación interrumpida" : job.progress.stage === "analysis" ? "Preparando waveforms y cues" : job.progress.stage === "library" ? "Guardando playlists e informe" : "Preparando audio")}</h2>
                <p className="mt-1 truncate text-xs text-muted-foreground">{job.progress.current_track && job.status !== "completed" ? job.progress.current_track : t("{count} playlists · {tracks} temas únicos", { count: job.preview.playlist_names.length, tracks: job.preview.track_count })}</p>
              </div>
            </div>
            <Button variant="secondary" size="sm" onClick={() => void reveal(job.output_path)}><FolderOpen className="h-4 w-4" />{t("Mostrar carpeta")}</Button>
          </div>
          {job.status === "running" && <>
            <progress className="h-2 w-full accent-primary" max={100} value={percent} aria-label={t("Progreso de exportación")} />
            <p className="text-xs text-muted-foreground">{t("{done} de {total} temas preparados", { done: job.progress.completed_tracks, total: job.progress.total_tracks })} · {t("Puedes cambiar de vista y volver mientras la app siga abierta.")}</p>
          </>}
          <p className="break-all font-mono text-xs text-muted-foreground">{job.output_path}</p>
          {job.status === "completed" && <div className="space-y-2 text-sm">
            <p>{t("PIONEER y Contents quedaron en el destino elegido. Si exportaste al pendrive, ya puedes expulsarlo y probarlo en el equipo.")}</p>
            <div className="flex flex-wrap items-center gap-3">
              {job.preview.warning_count > 0 && <span className="text-xs text-muted-foreground">{t("{count} avisos en el informe", { count: job.preview.warning_count })}</span>}
              {job.report_path && <Button variant="secondary" size="sm" onClick={() => void reveal(job.report_path!)}>{t("Mostrar informe")}</Button>}
            </div>
            {job.preview.unavailable_tracks.length > 0 && <details className="text-xs"><summary className="cursor-pointer font-medium text-amber-700 dark:text-amber-300">{t("Se omitieron {count} temas no disponibles.", { count: job.preview.unavailable_tracks.length })}</summary><div className="mt-2 max-h-64 overflow-y-auto"><UnavailableTrackList tracks={job.preview.unavailable_tracks} /></div></details>}
          </div>}
          {job.status === "failed" && <div role="alert" className="space-y-2 text-sm">
            <p className="break-words text-red-700 dark:text-red-300">{translateBackendMessage(locale, job.error ?? "")}</p>
            <p className="text-muted-foreground">{t(job.preview.updating ? "Revisa el estado del pendrive. La actualización conserva los archivos anteriores y permite restaurar las bases si se interrumpió su publicación." : "El destino puede contener archivos incompletos. Revisa el error y usa un destino sin una exportación anterior para reintentar.")}</p>
          </div>}
        </CardContent>
      </Card>}

      <div className="grid gap-5 xl:grid-cols-[minmax(0,1.3fr)_minmax(340px,1fr)]">
        <Card>
          <CardHeader className="flex-col items-stretch gap-3 py-4">
            <div className="flex items-center justify-between gap-2">
              <CardTitle className="flex items-center gap-2 text-base"><ListMusic className="h-4 w-4" />{t("1. Elige tus playlists")}</CardTitle>
              <Button variant="ghost" size="sm" disabled={locked} onClick={() => void loadOptions()} aria-label={t("Actualizar playlists")}><RefreshCcw className="h-4 w-4" /></Button>
            </div>
            <label className="relative block">
              <Search className="pointer-events-none absolute left-3 top-2.5 h-4 w-4 text-muted-foreground" />
              <input value={query} onChange={(e) => setQuery(e.target.value)} placeholder={t("Buscar playlist o biblioteca")} aria-label={t("Buscar playlist o biblioteca")} className="h-9 w-full rounded-md border border-input bg-background pl-9 pr-3 text-sm" />
            </label>
            <div className="flex items-center justify-between gap-2 text-xs">
              <label className="flex items-center gap-2"><input type="checkbox" checked={allVisibleSelected} disabled={locked || selectable.length === 0} onChange={() => {
                const next = new Set(selected);
                selectable.forEach((p) => allVisibleSelected ? next.delete(p.id) : next.add(p.id));
                changeSelection(next);
              }} />{t("Seleccionar visibles")}</label>
              <span className="text-muted-foreground">{t("{count} seleccionadas", { count: selected.size })}</span>
            </div>
          </CardHeader>
          <CardContent className="p-4">
            {destinationInfo && destinationInfo.playlists.length > 0 && <section className="mb-4 space-y-2 rounded-md border border-border bg-muted/30 p-3">
              <h3 className="text-sm font-semibold">{t("Playlists del pendrive")}</h3>
              <p className="text-xs text-muted-foreground">{t("Las playlists no seleccionadas se conservan en el USB. Selecciona las que quieras actualizar desde Rau.")}</p>
              <div className="max-h-64 divide-y divide-border overflow-auto">
                {destinationInfo.playlists.map((playlist) => <label key={playlist.id} className="flex items-start gap-2 py-2 text-xs">
                  <input type="checkbox" checked={playlist.source_id ? selected.has(playlist.source_id) : true} disabled={locked || !playlist.source_id || !destinationInfo.can_update} onChange={() => {
                    if (!playlist.source_id) return;
                    const next = new Set(selected); next.has(playlist.source_id) ? next.delete(playlist.source_id) : next.add(playlist.source_id); changeSelection(next);
                  }} />
                  <span className="min-w-0 flex-1 break-words">{playlist.path}{!playlist.source_id && <span className="mt-1 block text-muted-foreground">{t("Solo en el pendrive · se conserva")}</span>}</span>
                  <span className="shrink-0 text-muted-foreground">{playlist.track_count}</span>
                </label>)}
              </div>
            </section>}
            {loading ? <p role="status" className="py-8 text-center text-sm text-muted-foreground">{t("Cargando playlists…")}</p> : options.length === 0 ? <div className="space-y-3 py-8 text-center">
              <p className="text-sm text-muted-foreground">{t("Importa o crea una playlist en tu biblioteca para empezar.")}</p>
              <Button asChild variant="secondary"><Link to="/playlists">{t("Ir a la biblioteca")}</Link></Button>
            </div> : visible.length === 0 ? <p className="py-8 text-center text-sm text-muted-foreground">{t("No hay playlists que coincidan con la búsqueda.")}</p> : <div className="max-h-[55vh] divide-y divide-border overflow-auto rounded-md border border-border">
              {visible.map((p) => <label key={p.id} className={cn("flex cursor-pointer items-start gap-3 p-3 hover:bg-muted/50", selected.has(p.id) && "bg-muted/50", (locked || (p.track_count === 0 && !destinationInfo?.existing)) && "cursor-default opacity-60")}>
                <input type="checkbox" className="mt-1 shrink-0" checked={selected.has(p.id)} disabled={locked || (p.track_count === 0 && !destinationInfo?.existing)} onChange={() => {
                  const next = new Set(selected); next.has(p.id) ? next.delete(p.id) : next.add(p.id); changeSelection(next);
                }} />
                <div className="min-w-0 flex-1"><div className="truncate text-sm font-medium" title={p.name}>{p.name}</div><div className="mt-1 truncate text-xs text-muted-foreground" title={p.playlist_path ?? p.library_name}>{p.library_name} · {p.target_kind === "draft" ? t("Creada en Rau") : p.playlist_path}</div></div>
                <span className="shrink-0 text-right text-xs text-muted-foreground">{t("{count} temas", { count: p.track_count })}{existingSources.has(p.id) && <span className="mt-1 block text-emerald-600">{t("En el pendrive")}</span>}</span>
              </label>)}
            </div>}
          </CardContent>
        </Card>

        <div className="space-y-5">
          <Card>
            <CardHeader><CardTitle className="flex items-center gap-2 text-base"><HardDrive className="h-4 w-4" />{t("2. Destino y resumen")}</CardTitle></CardHeader>
            <CardContent className="space-y-4 p-4">
              <div>
                <span className="mb-2 block text-xs font-medium">{t("Exportar directamente a")}</span>
                <Button variant="secondary" className="w-full justify-start" disabled={locked} onClick={() => void chooseFolder()}><FolderOpen className="h-4 w-4 shrink-0" /><span className="truncate">{destinationPath || t("Elegir carpeta de destino")}</span></Button>
              </div>
              <p className="text-xs text-muted-foreground">{t("Elige la raíz del pendrive para usarlo en el equipo. PIONEER y Contents se crearán directamente ahí.")}</p>
              <p className="text-xs text-muted-foreground">{t("Los exports de Rau se actualizan conservando las pistas y playlists existentes. Solo se prepara el audio nuevo o modificado.")}</p>
              <p className="text-xs text-muted-foreground">{t("Las pistas sin cambios reutilizan el análisis guardado.")}</p>
              {inspecting && <p role="status" className="flex items-center gap-2 text-xs text-muted-foreground"><LoaderCircle className="h-3 w-3 animate-spin" />{t("Leyendo la biblioteca del pendrive…")}</p>}
              {destinationInfo && <div className="space-y-2 rounded-md bg-muted/50 p-3 text-xs">
                <p>{t("USB: {playlists} playlists y {tracks} pistas existentes.", { playlists: destinationInfo.playlists.length, tracks: destinationInfo.track_count })}</p>
                {destinationInfo.reason && <p role="status" className="break-words text-amber-700 dark:text-amber-300">{translateBackendMessage(locale, destinationInfo.reason)}</p>}
                <Button size="sm" variant="secondary" disabled={locked} onClick={() => void inspectDestination(destinationPath)}>{t("Actualizar contenido del pendrive")}</Button>
                {destinationInfo.recovery_available && <Button size="sm" disabled={locked} onClick={() => void recoverDestination()}>{t("Restaurar biblioteca anterior")}</Button>}
              </div>}
              <label className="block text-xs font-medium">{t("Temas en paralelo")}<select className="mt-2 h-9 w-full rounded-md border border-input bg-background px-3 text-sm" value={workers} disabled={locked} onChange={(event) => { setWorkers(Number(event.target.value)); setPreview(null); setAcceptedOmissions(null); }}>
                {[1, 2, 4].map((value) => <option key={value} value={value}>{value}{value === 2 ? ` · ${t("Recomendado")}` : ""}</option>)}
              </select></label>
              {destinationPath && <pre className="whitespace-pre-wrap break-all rounded-md bg-muted p-2 font-mono text-xs">{`${destinationPath}\n├── PIONEER/\n└── Contents/`}</pre>}
              <p className="text-xs text-muted-foreground">{t("Los temas se organizan en Contents por artista y conservan su nombre. Los nombres repetidos reciben un sufijo; sin artista, se usa «Artista desconocido».")}</p>
              <p className="text-xs text-muted-foreground">{t("MP3 y AIFF se copian sin cambios. FLAC, WAV y los demás formatos se convierten a AIFF estéreo de 16 bits y 44,1 kHz.")}</p>

              {summary ? <div className="space-y-3 border-t border-border pt-4">
                <div className="grid grid-cols-2 gap-3">
                  <div className="rounded-md bg-muted/60 p-3"><div className="text-xl font-semibold">{summary.playlist_names.length}</div><div className="text-xs text-muted-foreground">{t("Playlists")}</div></div>
                  <div className="rounded-md bg-muted/60 p-3"><div className="text-xl font-semibold">{summary.track_count}</div><div className="text-xs text-muted-foreground">{t("Temas únicos")}</div></div>
                </div>
                <dl className="space-y-2 text-xs">
                  <div className="flex justify-between gap-3"><dt className="text-muted-foreground">{t("Archivos originales")}</dt><dd>{formatBytes(summary.source_bytes)}</dd></div>
                  <div className="flex justify-between gap-3"><dt className="text-muted-foreground">{t("Audio exportado estimado")}</dt><dd>{summary.estimated_audio_bytes === null ? t("No disponible") : formatBytes(summary.estimated_audio_bytes)}</dd></div>
                  <div className="flex justify-between gap-3"><dt className="text-muted-foreground">{t("Temas con cues importados")}</dt><dd>{summary.with_cues}</dd></div>
                  <div className="flex justify-between gap-3"><dt className="text-muted-foreground">{t("Copiar sin convertir")}</dt><dd>{summary.copied_tracks}</dd></div>
                  <div className="flex justify-between gap-3"><dt className="text-muted-foreground">{t("Convertir a AIFF")}</dt><dd>{summary.converted_tracks}</dd></div>
                </dl>
                {summary.sync_counts && <dl className="space-y-2 rounded-md bg-emerald-500/10 p-3 text-xs">
                  <div className="flex justify-between"><dt>{t("Pistas nuevas")}</dt><dd>{summary.sync_counts.new_tracks}</dd></div>
                  <div className="flex justify-between"><dt>{t("Audio y pistas reutilizados")}</dt><dd>{summary.sync_counts.reused_tracks}</dd></div>
                  <div className="flex justify-between"><dt>{t("Audio modificado")}</dt><dd>{summary.sync_counts.updated_tracks}</dd></div>
                  <div className="flex justify-between"><dt>{t("Análisis por actualizar")}</dt><dd>{summary.sync_counts.analysis_tracks}</dd></div>
                  <div className="flex justify-between"><dt>{t("Otras playlists conservadas")}</dt><dd>{summary.sync_counts.preserved_playlists}</dd></div>
                </dl>}
                {summary.unavailable_tracks.length > 0 && <div className="space-y-2 rounded-md border border-amber-500/30 bg-amber-500/10 p-3 text-xs">
                  <p>{t("{available} temas disponibles · {missing} no disponibles", { available: summary.track_count, missing: summary.unavailable_tracks.length })}</p>
                  {acceptedOmissions === summary.plan_id && <p>{t("Se omitirán {count} temas. El informe incluirá sus rutas y errores.", { count: summary.unavailable_tracks.length })}</p>}
                  <Button size="sm" variant="secondary" disabled={locked} onClick={() => setUnavailableOpen(true)}>{t("Revisar archivos no disponibles")}</Button>
                </div>}
                {summary.empty_playlists.length > 0 && <p className="text-xs text-amber-700 dark:text-amber-300">{t("Estas playlists quedarán vacías: {names}", { names: summary.empty_playlists.join(", ") })}</p>}
                <p className="text-xs text-muted-foreground">{t("El tamaño combina los archivos originales y una estimación de los AIFF convertidos. El análisis usa espacio temporal en el ordenador; no se ha comprobado el espacio libre.")}</p>
                {summary.without_grid > 0 && <p className="rounded-md bg-amber-500/10 p-3 text-xs text-amber-800 dark:text-amber-200">{t("{count} temas sin grilla. Abre el detalle de la pista para analizar y guardar su preparación antes de exportar.", { count: summary.without_grid })}</p>}
                <details className="text-xs"><summary className="cursor-pointer font-medium">{t("Playlists incluidas")}</summary><ul className="mt-2 list-inside list-disc space-y-1 text-muted-foreground">{summary.playlist_names.map((name, i) => <li key={i}>{name}</li>)}</ul></details>
                {summary.warnings.length > 0 && <details className="text-xs"><summary className="cursor-pointer font-medium">{t("Revisar avisos ({count})", { count: summary.warning_count })}</summary><ul className="mt-2 max-h-48 space-y-2 overflow-auto">{summary.warnings.map((w, i) => <li key={i}><strong>{w.track_title}</strong><p className="mt-0.5 text-muted-foreground">{t(w.message)}</p></li>)}</ul>{summary.warning_count > summary.warnings.length && <p className="mt-2 text-muted-foreground">{t("El informe final incluirá todos los avisos.")}</p>}</details>}
              </div> : null}
              <p id="usb-export-action-hint" role="status" className="rounded-md bg-muted/50 p-3 text-xs text-muted-foreground">{t(actionHint)}</p>
              <Button className="w-full" disabled={actionDisabled} aria-describedby="usb-export-action-hint" onClick={() => void (preview ? startExport() : reviewSelection())}>
                {running || preparing ? <LoaderCircle className="h-4 w-4 animate-spin" /> : preview ? <Usb className="h-4 w-4" /> : <ListMusic className="h-4 w-4" />}{t(actionLabel)}
              </Button>
              {preview && <div className="flex flex-wrap gap-2"><Button size="sm" variant="ghost" disabled={locked} onClick={() => void reviewSelection()}>{t("Volver a revisar archivos")}</Button><Button size="sm" variant="ghost" disabled={locked} onClick={cancelReview}>{t("Cancelar exportación")}</Button></div>}
            </CardContent>
          </Card>
        </div>
      </div>
      <UnavailableTracksDialog open={unavailableOpen && Boolean(preview)} tracks={preview?.unavailable_tracks ?? []} availableCount={preview?.track_count ?? 0} destination={preview?.destination_path ?? ""} emptyPlaylists={preview?.empty_playlists ?? []} busy={starting || preparing}
        onCancel={cancelReview} onContinue={() => { if (preview && preview.track_count > 0) { setAcceptedOmissions(preview.plan_id); setUnavailableOpen(false); } }} />
      <TerminalDrawer logs={terminalLogs} expanded={terminalExpanded} terminalRef={terminalRef} subtitle={t("Exportación y actualización USB")} emptyMessage="Los detalles de revisión, preparación y actualización aparecerán aquí."
        onToggle={() => setTerminalExpanded((current) => !current)} onClear={() => setTerminalLogs([])} />
    </main>
  );
}
