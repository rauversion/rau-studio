import type { TrackListItem } from "../tracks/types";

export type CatalogFacetValue = {
  value: string;
  name: string;
  count: number;
};

export type CatalogFilters = {
  genres: string[];
  artists: string[];
  playlists: string[];
  albums: string[];
  keys: string[];
  years: string[];
  formats: string[];
  bpmMin?: number;
  bpmMax?: number;
  ratingMin?: number;
  metadataGaps: string[];
  availability: string[];
};
export type CatalogResponse = {
  items: TrackListItem[];
  total: number;
  page: number;
  page_size: number;
  total_pages: number;
  facets: Record<"genres" | "artists" | "albums" | "keys" | "years" | "formats" | "ratings" | "metadata_gaps" | "availability", CatalogFacetValue[]>;
  query_terms: string[];
};
