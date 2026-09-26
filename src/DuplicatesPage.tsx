import { invoke } from "@tauri-apps/api/core";
import { Copy, FolderSearch, GitMerge, LoaderCircle, SkipForward, Square } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { useSearchParams } from "react-router-dom";
import { Button } from "./components/ui/button";
import { Card, CardContent } from "./components/ui/card";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "./components/ui/select";
import { useI18n } from "./i18n";

type Library = { id: string; source_name: string; track_count: number };
type Progress = {
  library_id: string; status: string; total: number; prepared: number; processed: number;
  hashed: number; cached: number; unique_size: number; missing: number; failed: number; skipped: number;
  bytes_total: number; bytes_done: number; bytes_read: number; current_path: string | null;
  elapsed_seconds: number; remaining_seconds: number | null; message: string | null;
};
type Track = { track_id: string; name: string | null; artist: string | null; source_path: string | null; playlist_count: number; status: string; message: string | null };
type Group = { checksum: string; size_bytes: number; tracks: Track[] };
type Report = { groups: Group[]; omitted: Track[]; unchecked: number };
type MergeSelection = { group: Group; survivor: Track };
type MergeRequest = { all: boolean; selections: MergeSelection[] };
type MergeBatch = {
  id: string; library_id: string; status: string; total: number; processed: number;
  merged_groups: number; merged_records: number; removed_track_ids: string[];
  current_group: string | null; errors: Array<{ label: string; message: string }>; message: string | null;
};
function removeMergedTracks(report: Report, ids: string[]): Report {
  const removed = new Set(ids);
  return { ...report, groups: report.groups.map((group) => ({ ...group, tracks: group.tracks.filter((track) => !removed.has(track.track_id)) })).filter((group) => group.tracks.length > 1) };
}
const emptyReport: Report = { groups: [], omitted: [], unchecked: 0 };
const running = (p: Progress | null) => p?.status === "preparing" || p?.status === "hashing";
const size = (bytes: number) => bytes >= 1024 ** 3 ? `${(bytes / 1024 ** 3).toFixed(1)} GiB` : `${(bytes / 1024 ** 2).toFixed(1)} MiB`;
const time = (seconds: number) => seconds < 60 ? `${Math.ceil(seconds)} s` : `${Math.ceil(seconds / 60)} min`;

export function DuplicatesPage() {
  const { locale, t } = useI18n();
  const [params] = useSearchParams();
  const [libraries, setLibraries] = useState<Library[]>([]);
  const [libraryId, setLibraryId] = useState("");
  const [progress, setProgress] = useState<Progress | null>(null);
  const [report, setReport] = useState<Report>(emptyReport);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");
  const [survivors, setSurvivors] = useState<Record<string, string>>({});
  const [request, setRequest] = useState<MergeRequest | null>(null);
  const [groupPage, setGroupPage] = useState(0);
  const [omittedPage, setOmittedPage] = useState(0);
  const dialogRef = useRef<HTMLDialogElement>(null);
  const reportVersion = useRef(0);
  const mutationInFlight = useRef(false);
  const activeLibraryRef = useRef(libraryId);
  const [batch, setBatch] = useState<MergeBatch | null>(null);
  const mergingAll = batch?.status === "running";
  const currentBatch = batch?.library_id === libraryId ? batch : null;
  const scanning = running(progress);
  const current = progress?.library_id === libraryId ? progress : null;
  const locked = busy || scanning || mergingAll;

  useEffect(() => {
    let disposed = false;
    invoke<Library[]>("playlist_index_libraries").then((items) => {
      if (disposed) return;
      setLibraries(items);
      setLibraryId(items.find((item) => item.id === params.get("library"))?.id ?? items[0]?.id ?? "");
      if (!items.length) setLoading(false);
    }).catch((e) => { if (!disposed) { setError(String(e)); setLoading(false); } });
    return () => { disposed = true; };
  }, []);

  useEffect(() => {
    if (!libraryId) return;
    let disposed = false;
    let inFlight = false;
    let lastReportKey = "";
    let lastBatchKey = "";
    activeLibraryRef.current = libraryId;
    reportVersion.current += 1;
    setBatch(null);
    setReport(emptyReport); setProgress(null); setLoading(true); setSurvivors({}); setGroupPage(0); setOmittedPage(0);
    async function poll() {
      if (inFlight || mutationInFlight.current) return;
      inFlight = true;
      const version = reportVersion.current;
      try {
        const [next, nextBatch] = await Promise.all([
          invoke<Progress | null>("playlist_duplicates_status", { libraryId }),
          invoke<MergeBatch | null>("playlist_duplicates_merge_batch_status", { libraryId })
        ]);
        if (disposed || version !== reportVersion.current || mutationInFlight.current) return;
        setProgress(next); setBatch(nextBatch);
        const batchKey = `${nextBatch?.id}:${nextBatch?.status}:${nextBatch?.processed}`;
        if (batchKey !== lastBatchKey && nextBatch?.library_id === libraryId) {
          setReport((previous) => removeMergedTracks(previous, nextBatch.removed_track_ids));
        }
        lastBatchKey = batchKey;
        const key = `${next?.library_id}:${next?.status}:${next?.processed}:${batchKey}:${version}`;
        if (lastReportKey === "" || (!running(next) && key !== lastReportKey)) {
          const [data, items] = await Promise.all([
            invoke<Report>("playlist_duplicates_report", { libraryId }),
            invoke<Library[]>("playlist_index_libraries")
          ]);
          // A response started before a merge must never restore its old rows.
          if (disposed || version !== reportVersion.current || mutationInFlight.current) return;
          setReport(data); setLibraries(items); setLoading(false); lastReportKey = key;
        }
      } catch (e) { if (!disposed && version === reportVersion.current && !mutationInFlight.current) { setError(String(e)); setLoading(false); } }
      finally { inFlight = false; }
    }
    void poll();
    const timer = window.setInterval(() => void poll(), 1000);
    return () => { disposed = true; reportVersion.current += 1; window.clearInterval(timer); };
  }, [libraryId]);

  useEffect(() => {
    if (!request || !dialogRef.current) return;
    const dialog = dialogRef.current;
    dialog.showModal();
    return () => dialog.close();
  }, [request]);

  const reload = useCallback(async () => {
    const version = ++reportVersion.current;
    const data = await invoke<Report>("playlist_duplicates_report", { libraryId });
    if (version === reportVersion.current && activeLibraryRef.current === libraryId) setReport(data);
  }, [libraryId]);

  async function start() {
    if (mutationInFlight.current) return;
    mutationInFlight.current = true;
    reportVersion.current += 1;
    setBusy(true); setError(""); setMessage("");
    try {
      await invoke("playlist_duplicates_start", { libraryId });
      setBatch(null);
      setProgress(await invoke<Progress>("playlist_duplicates_status", { libraryId }));
    } catch (e) { setError(String(e)); }
    finally { mutationInFlight.current = false; reportVersion.current += 1; setBusy(false); }
  }

  async function control(command: "playlist_duplicates_cancel" | "playlist_duplicates_skip") {
    try {
      await invoke(command, { sourcePath: progress?.current_path });
      setMessage(t(command === "playlist_duplicates_cancel" ? "Cancelando; se conservan los checksums terminados." : "Se omitirá el archivo actual al terminar el bloque de lectura."));
    } catch (e) { setError(String(e)); }
  }

  function selection(group: Group): MergeSelection {
    return { group, survivor: group.tracks.find((track) => track.track_id === survivors[group.checksum]) ?? group.tracks[0] };
  }

  async function merge() {
    if (!request || mutationInFlight.current) return;
    const approved = request;
    mutationInFlight.current = true;
    reportVersion.current += 1;
    setBusy(true); setError(""); setMessage("");
    try {
      if (approved.all) {
        await invoke("playlist_duplicates_merge_batch_start", {
          libraryId,
          groups: approved.selections.map(({ group, survivor }) => ({
            checksum: group.checksum, survivor_id: survivor.track_id,
            track_ids: group.tracks.map((track) => track.track_id), label: survivor.name ?? survivor.track_id
          }))
        });
        setRequest(null);
        setBatch({ id: "starting", library_id: libraryId, status: "running", total: approved.selections.length, processed: 0, merged_groups: 0, merged_records: 0, removed_track_ids: [], current_group: null, errors: [], message: null });
      } else {
        const { group, survivor } = approved.selections[0];
        const result = await invoke<{ merged: number; warning: string | null }>("playlist_duplicates_merge", {
          libraryId, survivorId: survivor.track_id,
          trackIds: group.tracks.map((track) => track.track_id), checksum: group.checksum
        });
        // Remove committed records immediately, even if refreshing the report fails.
        reportVersion.current += 1;
        setReport((previous) => removeMergedTracks(previous, group.tracks.filter((track) => track.track_id !== survivor.track_id).map((track) => track.track_id)));
        setRequest(null);
        setLibraries((items) => items.map((item) => item.id === libraryId ? { ...item, track_count: Math.max(0, item.track_count - result.merged) } : item));
        setMessage(t("Fusión completada: {count} registros reunidos.", { count: result.merged + 1 }));
        if (result.warning) setError(result.warning);
        await reload();
      }
    } catch (e) { setError(String(e)); }
    finally { mutationInFlight.current = false; reportVersion.current += 1; setBusy(false); }
  }

  async function stopBatch() {
    try {
      await invoke("playlist_duplicates_merge_batch_cancel");
      setMessage(t("Se detendrá al terminar el grupo actual. Las fusiones terminadas se conservan."));
    } catch (e) { setError(String(e)); }
  }

  const percent = !current ? 0 : current.status === "preparing"
    ? (current.total ? current.prepared / current.total * 100 : 0)
    : current.bytes_total ? Math.min(100, current.bytes_done / current.bytes_total * 100)
    : current.total ? current.processed / current.total * 100 : 0;
  const groupPages = Math.max(1, Math.ceil(report.groups.length / 20));
  const safeGroupPage = Math.min(groupPage, groupPages - 1);
  const omittedPages = Math.max(1, Math.ceil(report.omitted.length / 20));
  const safeOmittedPage = Math.min(omittedPage, omittedPages - 1);
  const statuses: Record<string, string> = {
    preparing: "Revisando archivos", hashing: "Comparando contenido", completed: "Escaneo completado",
    cancelled: "Escaneo cancelado", interrupted: "Escaneo interrumpido", failed: "No se pudo completar el escaneo"
  };
  const reasons: Record<string, string> = { missing: "Archivo no encontrado", unreadable: "No se pudo leer", skipped: "Omitido manualmente" };

  return <main className="mx-auto flex w-full max-w-6xl flex-col gap-6 p-4 md:p-6">
    <header className="flex flex-wrap items-start justify-between gap-4">
      <div>
        <h1 className="flex items-center gap-2 text-2xl font-semibold"><Copy className="h-6 w-6" />{t("Duplicados")}</h1>
        <p className="mt-2 max-w-2xl text-sm text-muted-foreground">{t("Encuentra copias exactas y reúne sus playlists en un solo registro. Los archivos de audio se conservan en el disco.")}</p>
      </div>
      <Button disabled={!libraryId || locked || loading} onClick={() => void start()}>
        {scanning ? <LoaderCircle className="h-4 w-4 animate-spin" /> : <FolderSearch className="h-4 w-4" />}
        {t(scanning ? "Escaneando" : current ? "Volver a escanear" : "Buscar duplicados")}
      </Button>
    </header>
    <div className="max-w-md">
      <Select value={libraryId} onValueChange={(id) => { setLibraryId(id); setError(""); setMessage(""); }} disabled={locked}>
        <SelectTrigger aria-label={t("Biblioteca")}><SelectValue placeholder={t("Biblioteca")} /></SelectTrigger>
        <SelectContent>{libraries.map((library) => <SelectItem key={library.id} value={library.id}>{library.source_name} · {library.track_count.toLocaleString(locale)}</SelectItem>)}</SelectContent>
      </Select>
    </div>
    {error && !request ? <p role="alert" className="text-sm text-destructive">{error}</p> : null}
    {message ? <p role="status" className="text-sm text-muted-foreground">{message}</p> : null}
    {progress && scanning && !current ? <p role="status">{t("Hay un escaneo en curso en otra biblioteca.")}</p> : null}
    {mergingAll && !currentBatch ? <p role="status">{t("Hay una fusión en curso en otra biblioteca.")}</p> : null}
    {currentBatch ? <Card><CardContent className="space-y-3 p-5">
      <div className="flex flex-wrap items-center justify-between gap-3"><h2 className="font-semibold">{t(currentBatch.status === "running" ? "Fusionando grupos" : currentBatch.status === "cancelled" ? "Fusión detenida" : currentBatch.status === "failed" ? "Fusión interrumpida" : "Fusión de grupos terminada")}</h2>
        {mergingAll ? <Button variant="secondary" onClick={() => void stopBatch()}><Square className="h-4 w-4" />{t("Detener")}</Button> : null}
      </div>
      <p className="text-sm">{t("{done} de {total} grupos revisados", { done: currentBatch.processed, total: currentBatch.total })} · {t("Grupos fusionados: {count}", { count: currentBatch.merged_groups })} · {t("Grupos con errores: {count}", { count: currentBatch.errors.length })}</p>
      <div role="progressbar" aria-label={t("Fusión de grupos")} aria-valuemin={0} aria-valuemax={currentBatch.total} aria-valuenow={currentBatch.processed} className="h-2 overflow-hidden rounded-full bg-muted"><div className="h-full bg-primary transition-all" style={{ width: `${currentBatch.total ? currentBatch.processed / currentBatch.total * 100 : 0}%` }} /></div>
      {currentBatch.current_group ? <p className="truncate text-sm text-muted-foreground">{currentBatch.current_group}</p> : null}
      {currentBatch.message ? <p role="alert" className="text-sm text-destructive">{currentBatch.message}</p> : null}
      {currentBatch.errors.length ? <details><summary className="cursor-pointer text-sm">{t("Ver errores de fusión")}</summary><ul className="mt-2 max-h-60 space-y-2 overflow-y-auto text-sm">{currentBatch.errors.map((entry, index) => <li key={index}><strong>{entry.label}</strong>: {entry.message}</li>)}</ul></details> : null}
      {mergingAll ? <p className="text-xs text-muted-foreground">{t("Puedes salir de esta vista. Cada grupo se verifica y se fusiona por separado.")}</p> : null}
    </CardContent></Card> : null}
    {current ? <Card><CardContent className="space-y-4 p-5">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div><h2 className="font-semibold">{t(statuses[current.status] ?? current.status)}</h2><p className="mt-1 text-sm text-muted-foreground">{t("{done} de {total} archivos", { done: current.status === "preparing" ? current.prepared : current.processed, total: current.total })} · {time(current.elapsed_seconds)}</p></div>
        {scanning ? <div className="flex gap-2">
          <Button variant="secondary" disabled={current.status !== "hashing" || !current.current_path} onClick={() => void control("playlist_duplicates_skip")}><SkipForward className="h-4 w-4" />{t("Omitir archivo")}</Button>
          <Button variant="secondary" onClick={() => void control("playlist_duplicates_cancel")}><Square className="h-4 w-4" />{t("Cancelar")}</Button>
        </div> : null}
      </div>
      <div role="progressbar" aria-label={t(statuses[current.status] ?? current.status)} aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(percent)} className="h-2 overflow-hidden rounded-full bg-muted"><div className="h-full bg-primary transition-all" style={{ width: `${percent}%` }} /></div>
      {current.current_path ? <p className="truncate text-xs text-muted-foreground" title={current.current_path}>{current.current_path}</p> : null}
      <div className="flex flex-wrap gap-x-5 gap-y-2 text-sm">
        <span>{t("{count} calculados", { count: current.hashed })}</span><span>{t("{count} reutilizados", { count: current.cached })}</span>
        <span>{t("{count} descartados por tamaño", { count: current.unique_size })}</span><span>{t("{count} no encontrados", { count: current.missing })}</span>
        <span>{t("{count} ilegibles u omitidos", { count: current.failed + current.skipped })}</span>
      </div>
      {current.bytes_total > 0 ? <p className="text-xs text-muted-foreground">{t("Lectura: {done} de {total}", { done: size(current.bytes_read), total: size(current.bytes_total) })}{scanning && current.remaining_seconds !== null ? ` · ${t("Tiempo restante aproximado: {time}", { time: time(current.remaining_seconds) })}` : ""}</p> : null}
      {current.message ? <p className="text-sm text-destructive" role="alert">{current.message}</p> : null}
    </CardContent></Card> : null}
    <p className="text-sm text-muted-foreground">{t("La primera pasada depende del tamaño y la velocidad del disco. Se comparan tamaños primero y se reutilizan los checksums de archivos sin cambios. Puedes salir de esta vista mientras trabaja.")}</p>
    {loading ? <p className="flex items-center gap-2" role="status"><LoaderCircle className="h-4 w-4 animate-spin" />{t("Cargando")}</p> : !libraryId ? <p>{t("Importa una biblioteca para buscar duplicados.")}</p> : <>
      <section aria-labelledby="duplicate-groups-title" className="space-y-4">
        <div className="flex flex-wrap items-center justify-between gap-3"><div><h2 id="duplicate-groups-title" className="text-lg font-semibold">{t("Copias exactas")} · {report.groups.length}</h2><p className="text-sm text-muted-foreground">{t("Cambios en etiquetas, portada o formato producen un archivo distinto y no se agrupan aquí.")}</p></div>
          <Button disabled={locked || loading || !report.groups.length} onClick={() => { setError(""); setRequest({ all: true, selections: report.groups.map(selection) }); }}><GitMerge className="h-4 w-4" />{t("Aprobar todo")}</Button>
        </div>
        {scanning ? <p className="text-sm text-muted-foreground">{t("Los resultados se actualizarán cuando termine el escaneo.")}</p> : null}
        {!report.groups.length ? <div className="rounded-lg border border-dashed border-border p-8 text-center text-muted-foreground">{t(current || currentBatch || message ? "No quedan grupos de duplicados para revisar." : "Inicia un escaneo para comprobar los archivos de esta biblioteca.")}</div> : null}
        {report.groups.slice(safeGroupPage * 20, (safeGroupPage + 1) * 20).map((group) => {
          const survivor = group.tracks.find((track) => track.track_id === survivors[group.checksum]) ?? group.tracks[0];
          return <Card key={group.checksum}><CardContent className="p-0">
            <div className="flex flex-wrap items-center justify-between gap-3 border-b border-border p-4"><div><h3 className="font-medium">{group.tracks[0].name ?? t("Sin título")}</h3><p className="text-xs text-muted-foreground">{t("{count} registros idénticos", { count: group.tracks.length })} · {size(group.size_bytes)}</p></div><Button variant="secondary" disabled={locked} onClick={() => { setError(""); setRequest({ all: false, selections: [{ group, survivor }] }); }}><GitMerge className="h-4 w-4" />{t("Revisar fusión")}</Button></div>
            <fieldset disabled={locked} className="max-h-96 overflow-y-auto p-4"><legend className="sr-only">{t("Elige el registro a conservar")}</legend>
              {group.tracks.map((track) => <label key={track.track_id} className="flex cursor-pointer items-start gap-3 rounded-md p-3 hover:bg-muted/50">
                <input type="radio" name={`survivor-${group.checksum}`} checked={survivor.track_id === track.track_id} onChange={() => setSurvivors((values) => ({ ...values, [group.checksum]: track.track_id }))} className="mt-1 accent-primary" />
                <div className="min-w-0 flex-1"><div className="flex flex-wrap gap-2 text-sm"><span className="font-medium">{track.name ?? t("Sin título")}</span><span className="text-muted-foreground">{track.artist}</span>{survivor.track_id === track.track_id ? <span className="text-primary">· {t("Conservar")}</span> : null}</div><p className="break-all text-xs text-muted-foreground">{track.source_path}</p><p className="mt-1 text-xs text-muted-foreground">{t("{count} playlists", { count: track.playlist_count })}</p></div>
              </label>)}
            </fieldset>
          </CardContent></Card>;
        })}
        {groupPages > 1 ? <div className="flex items-center justify-end gap-3"><Button variant="secondary" disabled={!safeGroupPage} onClick={() => setGroupPage(safeGroupPage - 1)}>{t("Anterior")}</Button><span className="text-sm">{safeGroupPage + 1} / {groupPages}</span><Button variant="secondary" disabled={safeGroupPage + 1 >= groupPages} onClick={() => setGroupPage(safeGroupPage + 1)}>{t("Siguiente")}</Button></div> : null}
      </section>
      {report.unchecked > 0 ? <p className="text-sm text-muted-foreground">{t("{count} registros pendientes de comprobar. Un checksum vacío nunca se considera duplicado.", { count: report.unchecked })}</p> : null}
      {report.omitted.length > 0 ? <details className="rounded-lg border border-border p-4"><summary className="cursor-pointer font-medium">{t("Archivos omitidos")} · {report.omitted.length}</summary><p className="my-3 text-sm text-muted-foreground">{t("No participan en las coincidencias. Conecta el disco o corrige el acceso y vuelve a escanear para reintentarlos.")}</p>
        <ul className="divide-y divide-border">{report.omitted.slice(safeOmittedPage * 20, (safeOmittedPage + 1) * 20).map((track) => <li key={track.track_id} className="py-3 text-sm"><p className="font-medium">{track.name ?? track.track_id} · {t(reasons[track.status] ?? track.status)}</p><p className="break-all text-xs text-muted-foreground">{track.source_path ?? t("Sin ruta de archivo")}</p>{track.message ? <p className="break-all text-xs text-muted-foreground">{track.message}</p> : null}</li>)}</ul>
        {omittedPages > 1 ? <div className="mt-3 flex items-center justify-end gap-3"><Button variant="secondary" disabled={!safeOmittedPage} onClick={() => setOmittedPage(safeOmittedPage - 1)}>{t("Anterior")}</Button><span className="text-sm">{safeOmittedPage + 1} / {omittedPages}</span><Button variant="secondary" disabled={safeOmittedPage + 1 >= omittedPages} onClick={() => setOmittedPage(safeOmittedPage + 1)}>{t("Siguiente")}</Button></div> : null}
      </details> : null}
    </>}
    {request ? <dialog ref={dialogRef} aria-labelledby="merge-title" aria-describedby="merge-description" onCancel={(event) => { event.preventDefault(); if (!busy) setRequest(null); }} className="m-auto w-full max-w-lg rounded-lg border border-border bg-background p-0 text-foreground shadow-2xl backdrop:bg-black/40">
      <div className="space-y-4 p-5"><h2 id="merge-title" className="text-lg font-semibold">{request.all ? t("Aprobar {count} grupos de duplicados", { count: request.selections.length }) : t("Fusionar {count} registros", { count: request.selections[0].group.tracks.length })}</h2><p id="merge-description" className="text-sm text-muted-foreground">{t("Se conservará el registro elegido, se completarán sus etiquetas vacías y se reunirán las referencias de playlists. Los archivos físicos y el XML original se conservan. Esta fusión no tiene opción de deshacer en la interfaz.")}</p>
        {request.all ? <p className="text-sm">{t("Incluye todos los grupos de todas las páginas. Se conservará el registro marcado en cada grupo; por defecto, el primero. Si un grupo falla, continuará con el resto.")}</p> : null}
        <div className="max-h-48 space-y-2 overflow-y-auto rounded-md bg-muted p-3">{request.selections.slice(0, 10).map(({ group, survivor }) => <div key={group.checksum}><p className="text-sm font-medium">{survivor.name ?? survivor.track_id}</p><p className="break-all text-xs text-muted-foreground">{survivor.source_path}</p></div>)}{request.selections.length > 10 ? <p className="text-xs">{t("Y {count} grupos más", { count: request.selections.length - 10 })}</p> : null}</div>
        <p className="text-sm text-muted-foreground">{t("Antes de fusionar se verificará otra vez el contenido de todos los archivos del grupo.")}</p>
        {error ? <p role="alert" className="text-sm text-destructive">{error}</p> : null}
        <div className="flex justify-end gap-2"><Button variant="secondary" autoFocus disabled={busy} onClick={() => setRequest(null)}>{t("Cancelar")}</Button><Button disabled={busy} onClick={() => void merge()}>{busy ? <LoaderCircle className="h-4 w-4 animate-spin" /> : <GitMerge className="h-4 w-4" />}{t(busy ? "Verificando y fusionando" : request.all ? "Aprobar todo" : "Confirmar fusión")}</Button></div>
      </div>
    </dialog> : null}
  </main>;
}
