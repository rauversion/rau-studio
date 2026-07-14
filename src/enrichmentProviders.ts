export type EnrichmentProviderCredential = {
  id: string;
  label: string;
  required: boolean;
  secret: boolean;
  configured: boolean;
  preview?: string | null;
};

export type EnrichmentProviderDescriptor = {
  id: string;
  label: string;
  description: string;
  capabilities: string[];
  accepted_identifiers: string[];
  produced_identifiers: string[];
  credentials: EnrichmentProviderCredential[];
  ready: boolean;
  min_interval_ms: number;
};

export type EnrichmentProviderTestResult = {
  provider_id: string;
  ok: boolean;
  message: string;
};
