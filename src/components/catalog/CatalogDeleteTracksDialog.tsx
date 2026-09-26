import { LoaderCircle, Trash2 } from "lucide-react";
import { useEffect, useRef } from "react";
import { useI18n } from "../../i18n";
import type { TrackListItem } from "../tracks/types";
import { Button } from "../ui/button";

export type CatalogTrackDeletion = {
  libraryId: string;
  libraryName: string;
  tracks: TrackListItem[];
};

export function CatalogDeleteTracksDialog({ request, busy, error, onClose, onConfirm }: {
  request: CatalogTrackDeletion | null;
  busy: boolean;
  error: string;
  onClose: () => void;
  onConfirm: () => void;
}) {
  const { t } = useI18n();
  const dialogRef = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    const dialog = dialogRef.current;
    if (!request || !dialog) return;
    dialog.showModal();
    return () => dialog.close();
  }, [request]);

  if (!request) return null;
  const preview = request.tracks.slice(0, 6);

  return (
    <dialog
      ref={dialogRef}
      aria-labelledby="delete-catalog-tracks-title"
      aria-describedby="delete-catalog-tracks-description"
      className="m-auto w-full max-w-md rounded-md border border-border bg-background p-0 text-foreground shadow-2xl backdrop:bg-black/40"
      onCancel={(event) => { event.preventDefault(); if (!busy) onClose(); }}
    >
      <header className="border-b border-border px-4 py-4">
        <h2 id="delete-catalog-tracks-title" className="text-base font-semibold">{request.tracks.length === 1 ? t("Borrar track") : t("Borrar {count} tracks", { count: request.tracks.length })}</h2>
        <p className="mt-1 text-sm font-semibold">{request.libraryName}</p>
        <p id="delete-catalog-tracks-description" className="mt-2 text-sm text-muted-foreground">
          {t("Se borraran los tracks seleccionados del catalogo, sus datos asociados y sus referencias en playlists. Los archivos de audio y el XML original se conservan.")}
        </p>
      </header>
      <div className="px-4 pt-3">
        <ul className="grid gap-1 text-sm">
          {preview.map((track) => <li key={track.track_id} className="truncate" title={track.name ?? track.track_id}>{track.name ?? track.track_id}{track.artist ? ` · ${track.artist}` : ""}</li>)}
        </ul>
        {request.tracks.length > preview.length ? <p className="mt-1 text-xs text-muted-foreground">{t("+ {count} mas", { count: request.tracks.length - preview.length })}</p> : null}
        {error ? <p className="mt-3 text-sm text-destructive" role="alert">{error}</p> : null}
      </div>
      <footer className="flex justify-end gap-2 p-4">
        <Button type="button" variant="secondary" autoFocus disabled={busy} onClick={onClose}>{t("Cancelar")}</Button>
        <Button type="button" variant="destructive" disabled={busy} onClick={onConfirm}>
          {busy ? <LoaderCircle className="h-4 w-4 animate-spin" /> : <Trash2 className="h-4 w-4" />}
          {busy ? t("Eliminando") : t("Borrar")}
        </Button>
      </footer>
    </dialog>
  );
}
