import { AlertTriangle, Search } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useI18n } from "../../i18n";
import { Button } from "../ui/button";

export type UnavailableTrack = {
  source_key: string;
  title: string;
  artist: string;
  source_path: string | null;
  playlist_names: string[];
  problem: {
    kind: "no_local_path" | "not_found" | "permission_denied" | "not_a_file" | "empty_file" | "unreadable";
    detail: string | null;
  };
};

const problemLabels: Record<UnavailableTrack["problem"]["kind"], string> = {
  no_local_path: "La pista no tiene una ruta de audio local.",
  not_found: "Archivo no encontrado. Revisa si el disco está conectado o si el archivo se movió.",
  permission_denied: "No hay permiso para leer el archivo.",
  not_a_file: "La ruta no corresponde a un archivo de audio.",
  empty_file: "El archivo está vacío.",
  unreadable: "No se pudo leer el archivo.",
};

export function UnavailableTrackList({ tracks }: { tracks: UnavailableTrack[] }) {
  const { t } = useI18n();
  return <ul className="divide-y divide-border rounded-md border border-border text-xs">
    {tracks.map((track) => <li key={track.source_key} className="space-y-1.5 p-3">
      <p className="break-words font-semibold">{track.title}{track.artist ? ` · ${track.artist}` : ""}</p>
      <p className="text-amber-700 dark:text-amber-300">{t(problemLabels[track.problem.kind])}</p>
      <p className="break-all font-mono text-[11px] text-muted-foreground">{track.source_path || t("Sin ruta local")}</p>
      <p className="break-words text-muted-foreground">{t("Playlists afectadas")}: {track.playlist_names.join(", ")}</p>
      {track.problem.detail && <details className="text-muted-foreground"><summary className="cursor-pointer">{t("Detalle del error")}</summary><p className="mt-1 break-words">{track.problem.detail}</p></details>}
    </li>)}
  </ul>;
}

export function UnavailableTracksDialog({ open, tracks, availableCount, destination, emptyPlaylists, busy, onCancel, onContinue }: {
  open: boolean;
  tracks: UnavailableTrack[];
  availableCount: number;
  destination: string;
  emptyPlaylists: string[];
  busy: boolean;
  onCancel: () => void;
  onContinue: () => void;
}) {
  const { t } = useI18n();
  const dialog = useRef<HTMLDialogElement>(null);
  const [query, setQuery] = useState("");
  const visible = useMemo(() => {
    const value = query.trim().toLocaleLowerCase();
    return tracks.filter((track) => `${track.title} ${track.artist} ${track.source_path ?? ""} ${track.playlist_names.join(" ")}`.toLocaleLowerCase().includes(value));
  }, [tracks, query]);
  useEffect(() => {
    if (!open || !dialog.current) return;
    const element = dialog.current;
    setQuery("");
    element.showModal();
    return () => element.close();
  }, [open]);

  return <dialog ref={dialog} aria-labelledby="usb-unavailable-title" aria-describedby="usb-unavailable-description"
    className="m-auto max-h-[85vh] w-[min(640px,calc(100vw-32px))] overflow-y-auto rounded-lg border border-border bg-background p-0 text-foreground shadow-2xl backdrop:bg-black/40"
    onCancel={(event) => { event.preventDefault(); if (!busy) onCancel(); }}>
    <header className="space-y-3 border-b border-border p-4">
      <h2 id="usb-unavailable-title" className="flex items-center gap-2 text-base font-semibold"><AlertTriangle className="h-5 w-5 shrink-0 text-amber-600" />{t("Archivos no disponibles")}</h2>
      <p id="usb-unavailable-description" className="text-sm">{t("{missing} temas no están disponibles. Puedes omitirlos y exportar los {available} temas restantes, o cancelar para corregir las rutas.", { missing: tracks.length, available: availableCount })}</p>
      <p className="text-xs text-muted-foreground">{t("Los temas omitidos no aparecerán en las playlists exportadas. Sus detalles quedarán en el informe.")}</p>
      <p className="break-all font-mono text-xs text-muted-foreground">{destination}</p>
      {emptyPlaylists.length > 0 && <p className="text-xs text-amber-700 dark:text-amber-300">{t("Estas playlists quedarán vacías: {names}", { names: emptyPlaylists.join(", ") })}</p>}
      {availableCount === 0 && <p role="alert" className="text-sm font-medium text-destructive">{t("No hay temas disponibles para exportar. Corrige las rutas o conecta el disco y vuelve a revisar.")}</p>}
    </header>
    <div className="space-y-3 p-4">
      <label className="relative block"><Search className="pointer-events-none absolute left-3 top-2.5 h-4 w-4 text-muted-foreground" /><input className="h-9 w-full rounded border border-input bg-background pl-9 pr-3 text-sm" value={query} onChange={(event) => setQuery(event.target.value)} placeholder={t("Buscar entre los archivos no disponibles")} aria-label={t("Buscar entre los archivos no disponibles")} /></label>
      <div className="max-h-[38vh] overflow-y-auto"><UnavailableTrackList tracks={visible} /></div>
      {visible.length === 0 && <p role="status" className="text-xs text-muted-foreground">{t("No hay archivos que coincidan con la búsqueda.")}</p>}
    </div>
    <footer className="sticky bottom-0 flex flex-wrap justify-end gap-2 border-t border-border bg-background p-4">
      <Button type="button" variant="secondary" autoFocus disabled={busy} onClick={onCancel}>{t("Cancelar exportación")}</Button>
      <Button type="button" disabled={busy || availableCount === 0} onClick={onContinue}>{t("Omitir y continuar")}</Button>
    </footer>
  </dialog>;
}
