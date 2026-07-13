import type { TranslationValues } from "./i18n";

type Translate = (key: string, values?: TranslationValues) => string;

export function playbackErrorMessage(
  t: Translate,
  label: string,
  path?: string | null,
  error?: unknown
) {
  const detail = error ? `: ${String(error)}` : "";
  const hint = path?.startsWith("/Volumes/")
    ? t("Si el archivo esta en un disco externo, permite a Rau Studio acceder a Volumenes extraibles o agregalo a Acceso total al disco en macOS. Tambien revisa que el disco no este en solo lectura.")
    : t("Revisa permisos del archivo o carpeta.");

  return `${t("No se pudo reproducir")} ${label}${detail}. ${hint}`;
}
