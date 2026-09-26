import { invoke } from "@tauri-apps/api/core";
import { LoaderCircle, Search } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { translateBackendMessage, useI18n } from "../../i18n";
import { Button } from "../ui/button";
import type { CatalogFacetValue, CatalogFilters } from "./types";

type CatalogFacetPage = {
  items: CatalogFacetValue[];
  selected: CatalogFacetValue[];
  total: number;
  page: number;
  page_size: number;
  total_pages: number;
};

const facetLabels = {
  artists: { title: "Artista", search: "Buscar artistas", placeholder: "Buscar artistas...", empty: "No hay artistas que coincidan.", count: "{count} de {total} artistas", command: "playlist_catalog_artist_facets", pageSize: 12 },
  playlists: { title: "Playlists", search: "Buscar playlists", placeholder: "Buscar playlists...", empty: "No hay playlists que coincidan.", count: "{count} de {total} playlists", command: "playlist_catalog_playlist_facets", pageSize: 200 }
};

export function CatalogValueFacet({ facet, libraryId, catalogQuery, filters, refreshToken, labels, onToggle }: {
  facet: "artists" | "playlists";
  libraryId: string;
  catalogQuery: string;
  filters: CatalogFilters;
  refreshToken: number;
  labels?: Record<string, string>;
  onToggle: (value: string) => void;
}) {
  const { locale, t } = useI18n();
  const text = facetLabels[facet];
  const selectedValues = filters[facet];
  const [open, setOpen] = useState(selectedValues.length > 0);
  const [search, setSearch] = useState("");
  const [result, setResult] = useState<(CatalogFacetPage & { contextKey: string }) | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const requestSequence = useRef(0);
  const knownLabels = useRef(new Map<string, string>());
  // Selections in this facet filter tracks without changing its candidates or pages.
  const contextKey = JSON.stringify([facet, libraryId, catalogQuery, { ...filters, [facet]: [] }, refreshToken, search.trim()]);
  const current = result?.contextKey === contextKey ? result : null;
  const pinnedValues = selectedValues.filter((value) => !current?.items.some((item) => item.value === value));

  useEffect(() => {
    if (selectedValues.length > 0) setOpen(true);
  }, [selectedValues.length]);

  useEffect(() => {
    const requestId = ++requestSequence.current;
    setError("");
    setLoading(false);
    if (!open || current) return () => { ++requestSequence.current; };

    // Invalidate old pages immediately, then debounce this facet's lookup only.
    setResult(null);
    setLoading(true);
    const timer = window.setTimeout(() => void loadPage(1, false, requestId), 250);
    return () => {
      window.clearTimeout(timer);
      ++requestSequence.current;
    };
  }, [contextKey, open]);

  async function loadPage(page: number, append: boolean, requestId: number) {
    setLoading(true);
    setError("");
    try {
      const next = await invoke<CatalogFacetPage>(text.command, {
        request: { libraryId, query: catalogQuery, filters, page, pageSize: text.pageSize },
        search: search.trim()
      });
      if (requestId !== requestSequence.current) return;
      for (const item of [...next.items, ...next.selected]) {
        if (facet !== "playlists" || item.name !== item.value) knownLabels.current.set(item.value, item.name);
      }
      setResult((previous) => ({
        ...next,
        contextKey,
        items: append && previous?.contextKey === contextKey
          ? Array.from(new Map([...previous.items, ...next.items].map((item) => [item.value, item])).values())
          : next.items
      }));
    } catch (failure) {
      if (requestId === requestSequence.current) setError(translateBackendMessage(locale, String(failure)));
    } finally {
      if (requestId === requestSequence.current) setLoading(false);
    }
  }

  function loadMore() {
    if (loading || !current || current.page >= current.total_pages) return;
    void loadPage(current.page + 1, true, ++requestSequence.current);
  }

  return (
    <details className="group px-3 py-3" data-catalog-facet={facet} open={open} onToggle={(event) => setOpen(event.currentTarget.open)}>
      <summary className="flex cursor-pointer list-none items-center justify-between text-xs font-semibold">
        <span>{t(text.title)}</span>
        <span className="text-muted-foreground transition group-open:rotate-90">›</span>
      </summary>
      <div className="mt-2 grid gap-2">
        <div className="relative">
          <Search className="pointer-events-none absolute left-2 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
          <input
            type="search"
            aria-label={t(text.search)}
            placeholder={t(text.placeholder)}
            className="h-8 w-full rounded-md border border-input bg-background pl-7 pr-2 text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring"
            value={search}
            onChange={(event) => setSearch(event.currentTarget.value)}
          />
        </div>
        {pinnedValues.length > 0 ? (
          <div className="grid gap-0.5 border-b border-border pb-2">
            <span className="px-1.5 text-[10px] font-semibold text-muted-foreground">{t("Seleccionados")}</span>
            <div className="grid max-h-32 gap-0.5 overflow-y-auto">
              {pinnedValues.map((value) => (
                <FacetOption key={value} name={labels?.[value] ?? knownLabels.current.get(value) ?? (facet === "playlists" ? t("Playlist no disponible") : value)} count={current?.selected.find((item) => item.value === value)?.count} checked onToggle={() => onToggle(value)} />
              ))}
            </div>
          </div>
        ) : null}
        <div className="grid max-h-48 gap-0.5 overflow-y-auto pr-1" aria-busy={loading}>
          {current?.items.map((item) => (
            <FacetOption key={item.value} name={item.name} count={item.count} checked={selectedValues.includes(item.value)} onToggle={() => onToggle(item.value)} />
          ))}
          {!loading && !error && current?.total === 0 ? <span className="py-2 text-xs text-muted-foreground">{t(text.empty)}</span> : null}
        </div>
        {error ? (
          <div className="grid gap-1 text-xs" role="alert">
            <span className="break-words text-destructive">{error}</span>
            <Button variant="ghost" size="sm" onClick={() => void loadPage(current ? current.page + 1 : 1, Boolean(current), ++requestSequence.current)}>{t("Reintentar")}</Button>
          </div>
        ) : null}
        <div className="flex items-center justify-between gap-2 text-[10px] text-muted-foreground" role="status">
          <span>{current ? t(text.count, { count: current.items.length, total: current.total }) : ""}</span>
          {loading ? <span className="inline-flex items-center gap-1"><LoaderCircle className="h-3 w-3 animate-spin" />{t("Cargando")}</span> : null}
        </div>
        {current && current.page < current.total_pages ? (
          <Button variant="secondary" size="sm" disabled={loading} onClick={loadMore}>{t("Ver más")}</Button>
        ) : null}
      </div>
    </details>
  );
}

function FacetOption({ name, count, checked, onToggle }: {
  name: string;
  count?: number;
  checked: boolean;
  onToggle: () => void;
}) {
  return (
    <label className="flex min-w-0 cursor-pointer items-center gap-2 rounded px-1.5 py-1 text-xs hover:bg-accent">
      <input type="checkbox" checked={checked} onChange={onToggle} />
      <span className="min-w-0 flex-1 truncate" title={name}>{name}</span>
      <span className="tabular-nums text-muted-foreground">{count ?? "—"}</span>
    </label>
  );
}
