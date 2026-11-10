import { ArrowUp, ChevronLeft, ChevronRight, Download, FileAudio, Folder, Save } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useI18n } from "../../i18n";
import { Button } from "../ui/button";
import { buildRemoteTree, type RemoteDirectory, type RemoteFile } from "./remoteCatalogue";

type Props = {
  directories: RemoteDirectory[];
  username: string;
  path: string;
  disabled: boolean;
  hasDefaultDirectory: boolean;
  onNavigate: (path: string) => void;
  onDownload: (directory: string, file: RemoteFile, saveAs?: boolean) => void;
};
const pageSize = 100;
const formatSize = (value: number) => value < 1024 ? `${value} B` : `${(value / 1024 ** (value < 1024 ** 2 ? 1 : 2)).toFixed(1)} ${value < 1024 ** 2 ? "KB" : "MB"}`;

export function RemoteFileBrowser({ directories, username, path, disabled, hasDefaultDirectory, onNavigate, onDownload }: Props) {
  const { t } = useI18n();
  const [filter, setFilter] = useState("");
  const [page, setPage] = useState(0);
  const tree = useMemo(() => buildRemoteTree(directories), [directories]);
  const current = tree.get(path) || tree.get("")!;
  const crumbs = current.path.split("\\").filter(Boolean);
  useEffect(() => { setFilter(""); setPage(0); }, [path, directories]);
  useEffect(() => { if (!tree.has(path)) onNavigate(""); }, [tree, path, onNavigate]);

  const entries = useMemo(() => {
    const query = filter.trim().toLowerCase();
    // Filtering can jump to any folder; with no query only immediate children
    // appear, so similarly named sibling albums never become descendants.
    const folders = query
      ? [...tree.values()].filter((node) => node.path && node.path.toLowerCase().includes(query))
      : current.children.map((child) => tree.get(child)!);
    const files = current.files.filter((file) => file.path.toLowerCase().includes(query)).sort((a, b) => a.path.localeCompare(b.path, undefined, { numeric: true }));
    return [
      ...folders.map((folder) => ({ kind: "folder" as const, folder })),
      ...files.map((file) => ({ kind: "file" as const, file }))
    ];
  }, [tree, current, filter]);
  const pages = Math.max(1, Math.ceil(entries.length / pageSize));
  const visiblePage = Math.min(page, pages - 1);

  function navigate(next: string) { setFilter(""); setPage(0); onNavigate(next); }

  if (!username) return <p className="rounded-md border border-border p-6 text-center text-xs text-muted-foreground">{t("Explora un usuario para ver sus carpetas.")}</p>;

  return (
    <div className="grid min-w-0 gap-2">
      <div className="flex min-w-0 items-center gap-1">
        <Button size="icon" variant="ghost" disabled={!current.path} aria-label={t("Subir un nivel")} title={t("Subir un nivel")} onClick={() => navigate(current.parent)}><ArrowUp className="h-4 w-4" /></Button>
        <nav aria-label={t("Ruta de la carpeta")} className="flex min-w-0 flex-1 items-center gap-1 overflow-x-auto text-xs">
          <button type="button" className="shrink-0 rounded px-1 py-1 font-medium hover:bg-muted" onClick={() => navigate("")}>{username}</button>
          {crumbs.map((part, index) => {
            const target = crumbs.slice(0, index + 1).join("\\");
            return <span key={target} className="flex shrink-0 items-center gap-1"><ChevronRight className="h-3 w-3 text-muted-foreground" /><button type="button" className="rounded px-1 py-1 hover:bg-muted" aria-current={target === current.path ? "location" : undefined} onClick={() => navigate(target)}>{part}</button></span>;
          })}
        </nav>
      </div>
      <input className="h-9 w-full rounded-md border border-border bg-background px-3 text-sm outline-none focus:ring-2 focus:ring-ring" aria-label={t("Buscar en el catálogo remoto")} placeholder={t("Buscar carpetas o filtrar archivos")} value={filter} onChange={(e) => { setFilter(e.target.value); setPage(0); }} />
      <p className="text-xs text-muted-foreground">{t("{folders} carpetas · {files} archivos", { folders: current.children.length, files: current.files.length })}</p>
      <div className="max-h-[380px] overflow-auto rounded-md border border-border">
        <table className="w-full text-left text-xs">
          <thead className="sticky top-0 bg-card text-muted-foreground"><tr><th className="p-2">{t("Nombre")}</th><th className="p-2">{t("Tamaño")}</th><th className="p-2"><span className="sr-only">{t("Acciones")}</span></th></tr></thead>
          <tbody>{entries.slice(visiblePage * pageSize, (visiblePage + 1) * pageSize).map((entry) => entry.kind === "folder" ? (
            <tr key={`folder:${entry.folder.path}`} className="border-t border-border hover:bg-muted/30">
              <td colSpan={3} className="p-0"><button type="button" className="flex w-full items-center gap-2 px-2 py-2.5 text-left" onClick={() => navigate(entry.folder.path)}><Folder className="h-4 w-4 shrink-0 text-amber-500" /><span className="min-w-0 flex-1"><span className="block truncate font-medium" title={entry.folder.path}>{filter ? entry.folder.path : entry.folder.name}</span><span className="block text-muted-foreground">{t("{folders} carpetas · {files} archivos", { folders: entry.folder.children.length, files: entry.folder.files.length })}</span></span><ChevronRight className="h-4 w-4 shrink-0 text-muted-foreground" /></button></td>
            </tr>
          ) : (
            <tr key={`file:${entry.file.path}`} className="border-t border-border hover:bg-muted/30">
              <td className="max-w-[220px] p-2"><span className="flex items-center gap-2"><FileAudio className="h-4 w-4 shrink-0 text-muted-foreground" /><span className="truncate" title={entry.file.path}>{entry.file.path}</span></span><span className="ml-6 uppercase text-muted-foreground">{entry.file.extension}</span></td>
              <td className="whitespace-nowrap p-2 text-muted-foreground">{formatSize(entry.file.size)}</td>
              <td className="p-1"><div className="flex justify-end gap-1"><Button size="icon" variant="ghost" disabled={disabled} title={t("Descargar archivo")} aria-label={`${t("Descargar archivo")}: ${entry.file.path}`} onClick={() => onDownload(current.remotePath, entry.file)}><Download className="h-4 w-4" /></Button>{hasDefaultDirectory && <Button size="icon" variant="ghost" disabled={disabled} title={t("Guardar como…")} aria-label={`${t("Guardar como…")}: ${entry.file.path}`} onClick={() => onDownload(current.remotePath, entry.file, true)}><Save className="h-4 w-4" /></Button>}</div></td>
            </tr>
          ))}</tbody>
        </table>
        {!entries.length && <p className="p-6 text-center text-xs text-muted-foreground">{t(filter ? "No hay coincidencias en el catálogo." : "Esta carpeta está vacía.")}</p>}
      </div>
      {pages > 1 && <div className="flex items-center justify-between text-xs text-muted-foreground"><span>{t("Página {page} de {pages}", { page: visiblePage + 1, pages })}</span><span className="flex gap-1"><Button size="icon" variant="ghost" disabled={visiblePage === 0} aria-label={t("Página anterior")} onClick={() => setPage(visiblePage - 1)}><ChevronLeft className="h-4 w-4" /></Button><Button size="icon" variant="ghost" disabled={visiblePage + 1 >= pages} aria-label={t("Página siguiente")} onClick={() => setPage(visiblePage + 1)}><ChevronRight className="h-4 w-4" /></Button></span></div>}
    </div>
  );
}
