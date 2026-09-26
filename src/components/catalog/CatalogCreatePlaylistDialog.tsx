import { invoke, isTauri } from "@tauri-apps/api/core";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import { FileAudio, FolderOpen, ListPlus, LoaderCircle, Upload, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { translateBackendMessage, useI18n } from "../../i18n";
import { cn } from "../../lib/utils";
import type { PlaylistDraftOption } from "../tracks/PlaylistAddDialog";
import { Button } from "../ui/button";

const audioExtensions = ["wav", "wave", "aif", "aiff", "flac", "mp3", "m4a", "aac", "alac"];

function fileName(path: string) {
  return path.split(/[\\/]/).pop() ?? path;
}

export function CatalogCreatePlaylistDialog({ libraryId, onClose, onCreated }: {
  libraryId: string;
  onClose: () => void;
  onCreated: (playlist: PlaylistDraftOption & { library_id: string }) => void;
}) {
  const { locale, t } = useI18n();
  const dialogRef = useRef<HTMLDialogElement>(null);
  const nameRef = useRef<HTMLInputElement>(null);
  const busyRef = useRef(false);
  const mountedRef = useRef(false);
  const [name, setName] = useState("");
  const [paths, setPaths] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [choosingFiles, setChoosingFiles] = useState(false);
  const [dragActive, setDragActive] = useState(false);
  const [error, setError] = useState("");
  const [fileNotice, setFileNotice] = useState("");
  const addPathsRef = useRef(addPaths);
  addPathsRef.current = addPaths;

  useEffect(() => {
    mountedRef.current = true;
    const dialog = dialogRef.current;
    dialog?.showModal();
    nameRef.current?.focus();
    return () => {
      mountedRef.current = false;
      dialog?.close();
    };
  }, []);

  useEffect(() => {
    if (!isTauri()) return;
    let disposed = false;
    let unlisten: UnlistenFn | undefined;
    getCurrentWebview().onDragDropEvent(({ payload }) => {
      if (disposed || busyRef.current) return;
      if (payload.type === "leave") {
        setDragActive(false);
        return;
      }
      const bounds = dialogRef.current?.getBoundingClientRect();
      const x = payload.position.x / window.devicePixelRatio;
      const y = payload.position.y / window.devicePixelRatio;
      const inside = Boolean(bounds && x >= bounds.left && x <= bounds.right && y >= bounds.top && y <= bounds.bottom);
      setDragActive(inside && payload.type !== "drop");
      if (inside && payload.type === "drop") addPathsRef.current(payload.paths);
    }).then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    }).catch(() => {
      if (!disposed) setError(t("No se pudo activar el arrastre. Usa Seleccionar archivos."));
    });
    return () => { disposed = true; unlisten?.(); };
  }, [t]);

  function addPaths(incoming: string[]) {
    if (busyRef.current || incoming.length === 0) return;
    const accepted = incoming.filter((path) => {
      const base = fileName(path);
      return !base.startsWith("._") && audioExtensions.includes(base.split(".").pop()?.toLowerCase() ?? "");
    });
    setPaths((current) => Array.from(new Set([...current, ...accepted])));
    setError("");
    setFileNotice(accepted.length < incoming.length ? t("Archivos no compatibles omitidos: {count}.", { count: incoming.length - accepted.length }) : "");
  }

  async function chooseFiles() {
    if (busyRef.current || choosingFiles) return;
    setChoosingFiles(true);
    setError("");
    try {
      const selected = await open({ multiple: true, directory: false, filters: [{ name: "Audio", extensions: audioExtensions }] });
      if (mountedRef.current) addPaths(Array.isArray(selected) ? selected : selected ? [selected] : []);
    } catch (cause) {
      if (mountedRef.current) setError(translateBackendMessage(locale, String(cause)));
    } finally {
      if (mountedRef.current) setChoosingFiles(false);
    }
  }

  async function createPlaylist() {
    if (busyRef.current || choosingFiles || !name.trim() || paths.length === 0) return;
    busyRef.current = true;
    setBusy(true);
    setDragActive(false);
    setError("");
    try {
      const playlist = await invoke<PlaylistDraftOption & { library_id: string }>("playlist_catalog_create_playlist_from_files", {
        libraryId: libraryId || null, name: name.trim(), paths
      });
      if (mountedRef.current) onCreated(playlist);
    } catch (cause) {
      if (mountedRef.current) setError(translateBackendMessage(locale, String(cause)));
    } finally {
      busyRef.current = false;
      if (mountedRef.current) setBusy(false);
    }
  }

  return (
    <dialog
      ref={dialogRef}
      aria-labelledby="create-catalog-playlist-title"
      aria-describedby="create-catalog-playlist-description"
      className="m-auto max-h-[calc(100dvh-2rem)] w-[calc(100%-2rem)] max-w-xl overflow-y-auto rounded-md border border-border bg-background p-0 text-foreground shadow-2xl backdrop:bg-black/40"
      onCancel={(event) => { event.preventDefault(); if (!busyRef.current && !choosingFiles) onClose(); }}
      onKeyDown={(event) => event.stopPropagation()}
    >
      <form onSubmit={(event) => { event.preventDefault(); void createPlaylist(); }} aria-busy={busy}>
        <header className="flex items-start justify-between gap-3 border-b border-border p-4">
          <div>
            <h2 id="create-catalog-playlist-title" className="flex items-center gap-2 text-base font-semibold"><ListPlus className="h-5 w-5 text-primary" />{t("Crear playlist")}</h2>
            <p id="create-catalog-playlist-description" className="mt-1 text-sm text-muted-foreground">{t("Elige un nombre y agrega los archivos de audio de tu playlist.")}</p>
          </div>
          <Button type="button" variant="ghost" size="icon" aria-label={t("Cerrar")} disabled={busy || choosingFiles} onClick={onClose}><X className="h-4 w-4" /></Button>
        </header>
        <div className="grid gap-4 p-4">
          <label className="grid gap-1 text-sm">
            <span className="font-semibold">{t("Nombre")}</span>
            <input ref={nameRef} required disabled={busy} value={name} onChange={(event) => setName(event.currentTarget.value)} className="h-10 min-w-0 rounded-md border border-input bg-background px-3 outline-none focus-visible:ring-2 focus-visible:ring-ring" placeholder={t("Nombre de la playlist")} />
          </label>
          <div className={cn("grid justify-items-center gap-2 rounded-md border-2 border-dashed px-4 py-6 text-center transition-colors", dragActive ? "border-primary bg-primary/10" : "border-border bg-muted/30")}>
            <Upload className="h-7 w-7 text-muted-foreground" />
            <p className="text-sm font-semibold">{dragActive ? t("Suelta los archivos aquí") : t("Arrastra archivos de audio aquí")}</p>
            <p className="text-xs text-muted-foreground">WAV, AIFF, FLAC, MP3, M4A, AAC, ALAC</p>
            <Button type="button" variant="secondary" disabled={busy || choosingFiles} onClick={() => void chooseFiles()}><FolderOpen className="h-4 w-4" />{t("Seleccionar archivos")}</Button>
          </div>
          <div>
            <p className="mb-2 text-sm font-semibold" role="status">{t("{count} archivos seleccionados", { count: paths.length })}</p>
            {paths.length > 0 ? <ul className="max-h-52 overflow-y-auto rounded-md border border-border divide-y divide-border">
              {paths.map((path) => <li key={path} className="flex min-w-0 items-center gap-2 px-3 py-2">
                <FileAudio className="h-4 w-4 shrink-0 text-muted-foreground" />
                <span className="min-w-0 flex-1 truncate text-sm" title={path}>{fileName(path)}</span>
                <Button type="button" variant="ghost" size="icon" disabled={busy} aria-label={t("Quitar {name}", { name: fileName(path) })} onClick={() => { setPaths((current) => current.filter((value) => value !== path)); setError(""); }}><X className="h-4 w-4" /></Button>
              </li>)}
            </ul> : null}
          </div>
          {fileNotice ? <p className="text-sm text-muted-foreground" role="status">{fileNotice}</p> : null}
          {error ? <p className="text-sm text-destructive" role="alert">{error}</p> : null}
          {busy ? <p className="text-sm text-muted-foreground" role="status">{t("Leyendo metadata y creando playlist...")}</p> : null}
        </div>
        <footer className="flex justify-end gap-2 border-t border-border p-4">
          <Button type="button" variant="secondary" disabled={busy || choosingFiles} onClick={onClose}>{t("Cancelar")}</Button>
          <Button type="submit" disabled={busy || choosingFiles || !name.trim() || paths.length === 0}>{busy ? <LoaderCircle className="h-4 w-4 animate-spin" /> : <ListPlus className="h-4 w-4" />}{busy ? t("Creando playlist...") : t("Crear")}</Button>
        </footer>
      </form>
    </dialog>
  );
}
