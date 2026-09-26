import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { CatalogFilters, CatalogResponse } from "./types";

type CatalogQuery = {
  libraryId: string;
  query: string;
  filters: CatalogFilters;
  sort: string;
  pageSize: number;
};
type Snapshot = { key: string; response: CatalogResponse };
type Failure = { key: string; append: boolean; message: string };

export function useCatalogTracks(query: CatalogQuery, refreshToken: number) {
  const key = JSON.stringify(query);
  const request = useMemo<CatalogQuery>(() => JSON.parse(key), [key]);
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const currentSnapshot = useRef<Snapshot | null>(null);
  const [loading, setLoading] = useState(false);
  const [failure, setFailure] = useState<Failure | null>(null);
  const sequence = useRef(0);
  const activeRequest = useRef<number | null>(null);

  const invalidate = useCallback(() => {
    ++sequence.current;
    activeRequest.current = null;
  }, []);

  const load = useCallback(async (append: boolean) => {
    if (!request.libraryId || activeRequest.current !== null) return;
    const previous = currentSnapshot.current?.key === key ? currentSnapshot.current.response : null;
    if (append && (!previous || previous.page >= previous.total_pages)) return;
    const requestId = ++sequence.current;
    activeRequest.current = requestId;
    setLoading(true);
    setFailure(null);
    try {
      let page = append ? previous!.page + 1 : 1;
      // Refresh the whole loaded range after edits/deletions so offsets stay correct.
      let lastPage = append ? page : previous?.page ?? 1;
      let items = append ? previous!.items : [];
      let result: CatalogResponse | null = null;
      for (; page <= lastPage; page++) {
        const next = await invoke<CatalogResponse>("playlist_catalog_search", { request: { ...request, page } });
        if (requestId !== sequence.current) return;
        if (append && next.page !== page) {
          // A smaller catalog may clamp an obsolete page. Rebuild the loaded range.
          page = 0;
          lastPage = Math.min(previous!.page, next.total_pages);
          items = [];
          append = false;
          continue;
        }
        lastPage = Math.min(lastPage, next.total_pages);
        items = Array.from(new Map([...items, ...next.items].map((track) => [track.track_id, track])).values());
        result = { ...next, items };
      }
      if (result) {
        const nextSnapshot = { key, response: result };
        currentSnapshot.current = nextSnapshot;
        setSnapshot(nextSnapshot);
      }
    } catch (error) {
      if (requestId === sequence.current) setFailure({ key, append, message: String(error) });
    } finally {
      if (requestId === sequence.current) {
        activeRequest.current = null;
        setLoading(false);
      }
    }
  }, [key, request]);

  useEffect(() => {
    invalidate();
    if (currentSnapshot.current?.key !== key) {
      currentSnapshot.current = null;
      setSnapshot(null);
      setFailure(null);
    }
    if (request.libraryId) void load(false);
    else setLoading(false);
    return invalidate;
  }, [invalidate, key, load, refreshToken, request.libraryId]);

  const response = snapshot?.key === key ? snapshot.response : null;
  const error = failure?.key === key ? failure : null;
  const loadMore = useCallback(() => {
    if (!error) void load(true);
  }, [error, load]);
  const retry = useCallback(() => void load(error?.append ?? false), [error, load]);
  const setResponse = useCallback((update: (current: CatalogResponse) => CatalogResponse) => {
    const current = currentSnapshot.current;
    if (current?.key !== key) return;
    const next = { key, response: update(current.response) };
    currentSnapshot.current = next;
    setSnapshot(next);
  }, [key]);

  return {
    response,
    loading,
    error: error?.message ?? "",
    hasMore: Boolean(response && response.page < response.total_pages),
    loadMore,
    retry,
    setResponse,
    invalidate
  };
}
