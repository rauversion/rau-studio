import { invoke } from "@tauri-apps/api/core";
import { Activity, Check, LoaderCircle, Pause, Play, Save } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useI18n } from "../../i18n";
import { useAudioTimeline } from "../audio/GlobalAudioPlayer";
import { Button } from "../ui/button";
import type { TrackListItem } from "./types";

type Grid = { bpm: number; anchor_seconds: number; beat_in_bar: number };
type MusicalAnalysis = {
  version: string; duration_seconds: number; bpm: number | null; anchor_seconds: number | null;
  beat_in_bar: number; tempo_confidence: number; downbeat_confidence: number;
  key: string | null; key_confidence: number; waveform_hz: number; waveform: number[]; warnings: string[];
};
type Preparation = { analysis: MusicalAnalysis; grid: Grid | null; key: string | null; revision: string; saved_at: string | null };
type AnalysisState = { proposal: Preparation | null; prepared: Preparation | null; stale: boolean };
const roots = ["C", "Db", "D", "Eb", "E", "F", "Gb", "G", "Ab", "A", "Bb", "B"];
const keys = roots.flatMap((root) => [root, `${root}m`]);
const inputClass = "h-8 w-full rounded border border-input bg-background px-2 text-xs disabled:opacity-50";

export function MusicAnalysisPanel({ track, onSaved }: { track: TrackListItem; onSaved?: (bpm: string | null, key: string | null) => void }) {
  // Identity-keyed mounting also discards pending UI callbacks when switching
  // tracks. Backend proposals remain available when the sheet is reopened.
  return track.library_id && track.source_path ? <AnalysisEditor key={`${track.library_id}/${track.track_id}/${track.source_path}`} track={track} onSaved={onSaved} /> : null;
}

function AnalysisEditor({ track, onSaved }: { track: TrackListItem; onSaved?: (bpm: string | null, key: string | null) => void }) {
  const { t } = useI18n();
  const timeline = useAudioTimeline();
  const [state, setState] = useState<AnalysisState>({ proposal: null, prepared: null, stale: false });
  const [current, setCurrent] = useState<Preparation | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState<"analysis" | "save" | null>(null);
  const [error, setError] = useState("");
  const [saved, setSaved] = useState(false);
  const [bpm, setBpm] = useState("");
  const [anchor, setAnchor] = useState("0");
  const [beat, setBeat] = useState(1);
  const [key, setKey] = useState("");
  const [applyGrid, setApplyGrid] = useState(false);
  const [range, setRange] = useState("60-200");
  const [cursor, setCursor] = useState(0);
  const [viewStart, setViewStart] = useState(0);
  const [viewLength, setViewLength] = useState(8);
  const [pendingSeek, setPendingSeek] = useState<number | null>(null);
  const taps = useRef<number[]>([]);
  const mounted = useRef(true);
  const activeTrack = timeline.player?.path === track.source_path;
  const playhead = activeTrack ? timeline.currentTime : cursor;
  const duration = current?.analysis.duration_seconds ?? 0;
  const bpmValue = Number(bpm);
  const anchorValue = Number(anchor);
  const gridValid = bpm.trim() !== "" && Number.isFinite(bpmValue) && bpmValue >= 30 && bpmValue <= 300
    && anchor.trim() !== "" && Number.isFinite(anchorValue) && anchorValue >= 0 && anchorValue < duration;
  const dirty = () => setSaved(false);

  function choose(preparation: Preparation) {
    setCurrent(preparation);
    setBpm(preparation.grid?.bpm.toFixed(2) ?? "");
    setAnchor(preparation.grid?.anchor_seconds.toFixed(4) ?? "0");
    setBeat(preparation.grid?.beat_in_bar ?? 1);
    setApplyGrid(Boolean(preparation.grid));
    setKey(preparation.key ?? "");
    setSaved(false);
    taps.current = [];
  }

  useEffect(() => {
    mounted.current = true;
    let active = true;
    void invoke<AnalysisState>("track_analysis_load", { libraryId: track.library_id, trackId: track.track_id })
      .then((loaded) => {
        if (!active) return;
        setState(loaded);
        const value = loaded.proposal ?? loaded.prepared;
        if (value) choose(value);
      }).catch((cause) => { if (active) setError(String(cause)); })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; mounted.current = false; };
  }, [track.library_id, track.track_id]);

  useEffect(() => {
    if (activeTrack && timeline.duration > 0 && pendingSeek !== null) {
      timeline.seek(pendingSeek);
      setPendingSeek(null);
    }
  }, [activeTrack, timeline.duration, timeline.seek, pendingSeek]);

  async function analyze() {
    setBusy("analysis"); setError(""); setSaved(false);
    const [minBpm, maxBpm] = range.split("-").map(Number);
    try {
      const proposal = await invoke<Preparation>("track_analysis_run", { libraryId: track.library_id, trackId: track.track_id, minBpm, maxBpm });
      if (!mounted.current) return;
      setState((previous) => ({ ...previous, proposal, stale: false }));
      choose(proposal);
    } catch (cause) { if (mounted.current) setError(String(cause)); }
    finally { if (mounted.current) setBusy(null); }
  }

  async function save() {
    if (!current || (applyGrid && !gridValid)) return;
    setBusy("save"); setError("");
    try {
      const prepared = await invoke<Preparation>("track_analysis_save", {
        libraryId: track.library_id, trackId: track.track_id, revision: current.revision,
        grid: applyGrid ? { bpm: bpmValue, anchor_seconds: anchorValue, beat_in_bar: beat } : null,
        key: key || null
      });
      if (!mounted.current) return;
      setState({ proposal: null, prepared, stale: false }); choose(prepared); setSaved(true);
      onSaved?.(prepared.grid?.bpm.toFixed(2) ?? null, prepared.key);
    } catch (cause) { if (mounted.current) setError(String(cause)); }
    finally { if (mounted.current) setBusy(null); }
  }

  function seek(seconds: number) {
    const time = Math.max(0, Math.min(duration, seconds));
    setCursor(time);
    if (activeTrack) timeline.seek(time);
  }

  function shift(seconds: number) {
    if (!gridValid) return;
    let next = anchorValue + seconds;
    let nextBeat = beat;
    if (next < 0) { next += 60 / bpmValue; nextBeat = beat % 4 + 1; }
    if (next >= duration) return;
    setAnchor(next.toFixed(4)); setBeat(nextBeat); dirty();
  }

  function tap() {
    const now = performance.now();
    taps.current = taps.current.filter((value) => now - value < 6000).slice(-7);
    taps.current.push(now);
    if (taps.current.length < 3) return;
    const intervals = taps.current.slice(1).map((value, index) => value - taps.current[index]).sort((a, b) => a - b);
    const median = intervals[Math.floor(intervals.length / 2)];
    const result = 60000 / median;
    if (result >= 30 && result <= 300) { setBpm(result.toFixed(2)); setApplyGrid(true); dirty(); }
  }

  const confidence = (value: number) => t(value >= 0.65 ? "Alta" : value >= 0.35 ? "Media" : "Baja");
  return <section className="my-4 rounded-md border border-border bg-card p-3" aria-label={t("Preparación musical")}>
    <div className="flex items-center justify-between gap-2">
      <h3 className="flex items-center gap-2 text-sm font-semibold"><Activity className="h-4 w-4" />{t("Preparación musical")}</h3>
      {state.prepared && <span className="text-[10px] text-emerald-600">{t("Preparación guardada")}</span>}
    </div>
    <p className="mt-2 text-xs text-muted-foreground">{t("Detecta BPM, grilla y tonalidad desde el audio. Revisa el resultado y guárdalo para la exportación USB.")}</p>
    <div className="mt-3 flex items-end gap-2">
      <label className="min-w-0 flex-1 text-xs">{t("Rango de BPM")}
        <select className={`${inputClass} mt-1`} value={range} disabled={Boolean(busy) || loading} onChange={(event) => setRange(event.target.value)}>
          {["40-100", "60-200", "100-220", "140-300"].map((value) => <option key={value} value={value}>{value.replace("-", "–")}</option>)}
        </select>
      </label>
      <Button size="sm" disabled={Boolean(busy) || loading || !track.source_exists} onClick={() => void analyze()}>
        {busy === "analysis" || loading ? <LoaderCircle className="h-4 w-4 animate-spin" /> : <Activity className="h-4 w-4" />}
        {t(busy === "analysis" ? "Analizando audio…" : current ? "Volver a analizar" : "Analizar pista")}
      </Button>
    </div>
    {state.stale && <p role="status" className="mt-2 text-xs text-amber-600">{t("El audio cambió. Vuelve a analizarlo antes de usar la preparación guardada.")}</p>}
    {busy === "analysis" && <p role="status" className="mt-2 text-xs text-muted-foreground">{t("Puedes cerrar el detalle y volver; la propuesta quedará guardada cuando termine.")}</p>}
    {current && <fieldset className="mt-4 min-w-0 space-y-3" disabled={Boolean(busy)}>
      <p className="text-[11px] text-muted-foreground">{t("Claridad del pulso")}: {confidence(current.analysis.tempo_confidence)} · {t("Primer tiempo")}: {confidence(current.analysis.downbeat_confidence)} · {t("Tonalidad")}: {confidence(current.analysis.key_confidence)}</p>
      <Waveform analysis={current.analysis} start={0} length={duration} cursor={playhead} onSeek={seek} />
      <input type="range" className="w-full" min={0} max={duration} step={0.01} value={Math.min(playhead, duration)} onChange={(event) => seek(Number(event.target.value))} aria-label={t("Posición de escucha")} />
      <div className="flex items-center justify-between gap-2 text-xs">
        <Button size="sm" variant="secondary" onClick={() => {
          if (!activeTrack) setPendingSeek(cursor);
          void timeline.toggleTrackPlayback(track, (message) => setError(message));
        }}>{activeTrack && timeline.playing ? <Pause className="h-3 w-3" /> : <Play className="h-3 w-3" />}{t("Escuchar")}</Button>
        <span className="font-mono">{playhead.toFixed(3)} / {duration.toFixed(1)} s</span>
        <Button size="sm" variant="ghost" onClick={() => setViewStart(Math.max(0, Math.min(duration - viewLength, playhead - viewLength / 2)))}>{t("Ver posición")}</Button>
      </div>
      <Waveform analysis={current.analysis} start={viewStart} length={Math.min(viewLength, duration - viewStart)} cursor={playhead}
        grid={applyGrid && gridValid ? { bpm: bpmValue, anchor_seconds: anchorValue, beat_in_bar: beat } : undefined} onSeek={seek} />
      <div className="flex items-center gap-2">
        <input type="range" className="min-w-0 flex-1" min={0} max={Math.max(0, duration - viewLength)} step={0.1} value={viewStart} onChange={(event) => setViewStart(Number(event.target.value))} aria-label={t("Inicio de la vista ampliada")} />
        <select className="h-7 rounded border border-input bg-background px-1 text-xs" aria-label={t("Ampliación")} value={viewLength} onChange={(event) => { setViewLength(Number(event.target.value)); setViewStart(0); }}>
          {[2, 4, 8, 16, 32].map((value) => <option key={value} value={value}>{value} s</option>)}
        </select>
      </div>
      <label className="flex items-center gap-2 text-xs font-semibold"><input type="checkbox" checked={applyGrid} onChange={(event) => { setApplyGrid(event.target.checked); dirty(); }} />{t("Aplicar grilla de Rau (4/4)")}</label>
      <div className="grid grid-cols-3 gap-2">
        <label className="text-xs">BPM<input className={`${inputClass} mt-1`} type="number" min={30} max={300} step={0.01} value={bpm} onChange={(event) => { setBpm(event.target.value); dirty(); }} /></label>
        <label className="text-xs">{t("Beat en segundos")}<input className={`${inputClass} mt-1`} type="number" min={0} max={duration} step={0.001} value={anchor} onChange={(event) => { setAnchor(event.target.value); dirty(); }} /></label>
        <label className="text-xs">{t("Beat del compás")}<select className={`${inputClass} mt-1`} value={beat} onChange={(event) => { setBeat(Number(event.target.value)); dirty(); }}>{[1, 2, 3, 4].map((value) => <option key={value}>{value}</option>)}</select></label>
      </div>
      <div className="flex flex-wrap gap-1">
        <Button size="sm" variant="secondary" onClick={tap}>{t("Marcar tempo")}</Button>
        {[0.5, 2].map((factor) => <Button key={factor} size="sm" variant="ghost" disabled={!gridValid || bpmValue * factor < 30 || bpmValue * factor > 300} onClick={() => { setBpm((bpmValue * factor).toFixed(2)); dirty(); }}>{factor === 0.5 ? "÷2" : "×2"}</Button>)}
        <Button size="sm" variant="ghost" disabled={!gridValid} onClick={() => shift(-0.01)}>−10 ms</Button>
        <Button size="sm" variant="ghost" disabled={!gridValid} onClick={() => shift(0.01)}>+10 ms</Button>
      </div>
      <Button className="w-full" size="sm" variant="secondary" disabled={playhead >= duration} onClick={() => { setAnchor(playhead.toFixed(4)); setBeat(1); setApplyGrid(true); dirty(); }}>{t("Marcar esta posición como primer tiempo")}</Button>
      <label className="block text-xs">{t("Tonalidad")}<select className={`${inputClass} mt-1`} value={key} onChange={(event) => { setKey(event.target.value); dirty(); }}>
        <option value="">{t("Mantener tonalidad actual")}</option>
        {!keys.includes(key) && key && <option value={key}>{key}</option>}
        {keys.map((value) => <option key={value}>{value}</option>)}
      </select></label>
      {current.analysis.warnings.map((warning) => <p key={warning} className="text-[11px] text-muted-foreground">{t(warning)}</p>)}
      {!applyGrid && <p className="text-[11px] text-muted-foreground">{t("Se mantiene la grilla guardada o importada que ya tenga la pista.")}</p>}
      <p className="text-[11px] text-muted-foreground">{t("Los cues existentes se conservan. Guardar actualiza la preparación en Rau y las próximas exportaciones USB.")}</p>
      {applyGrid && !gridValid && <p role="status" className="text-xs text-amber-600">{t("Introduce BPM entre 30 y 300 y una posición dentro de la pista.")}</p>}
      <div className="flex flex-wrap items-center gap-2">
        <Button size="sm" disabled={Boolean(busy) || (applyGrid && !gridValid) || (!applyGrid && !key)} onClick={() => void save()}>{busy === "save" ? <LoaderCircle className="h-4 w-4 animate-spin" /> : <Save className="h-4 w-4" />}{t("Guardar preparación")}</Button>
        {saved && <span role="status" className="flex items-center gap-1 text-xs text-emerald-600"><Check className="h-3 w-3" />{t("Guardada para exportar")}</span>}
        {state.proposal && state.prepared && <Button size="sm" variant="ghost" onClick={() => choose(state.prepared!)}>{t("Ver preparación anterior")}</Button>}
      </div>
    </fieldset>}
    {error && <p role="alert" className="mt-3 text-xs text-destructive">{t(error)}</p>}
  </section>;
}

function Waveform({ analysis, start, length, cursor, grid, onSeek }: { analysis: MusicalAnalysis; start: number; length: number; cursor: number; grid?: Grid; onSeek: (seconds: number) => void }) {
  const { t } = useI18n();
  const columns = 500;
  const path: string[] = [];
  for (let x = 0; x < columns; x++) {
    const from = Math.floor((start + length * x / columns) * analysis.waveform_hz);
    const to = Math.max(from + 1, Math.ceil((start + length * (x + 1) / columns) * analysis.waveform_hz));
    let peak = 0;
    for (let index = from; index < Math.min(to, analysis.waveform.length); index++) peak = Math.max(peak, analysis.waveform[index]);
    const height = peak / 255 * 28;
    path.push(`M${x},${34 - height}V${34 + height}`);
  }
  const beats: { x: number; number: number }[] = [];
  if (grid) {
    const period = 60 / grid.bpm;
    const first = Math.ceil((start - grid.anchor_seconds) / period);
    for (let index = first; index < first + 200; index++) {
      const seconds = grid.anchor_seconds + index * period;
      if (seconds >= start + length) break;
      beats.push({ x: (seconds - start) / length * columns, number: ((grid.beat_in_bar - 1 + index) % 4 + 4) % 4 + 1 });
    }
  }
  return <svg viewBox="0 0 500 72" className="w-full cursor-crosshair rounded bg-secondary/60" role="img" aria-label={t("Forma de onda y grilla")}
    onPointerDown={(event) => { const bounds = event.currentTarget.getBoundingClientRect(); onSeek(start + Math.max(0, Math.min(1, (event.clientX - bounds.left) / bounds.width)) * length); }}>
    <path d={path.join("")} className="stroke-sky-500" strokeWidth="1" />
    {beats.map((beat, index) => <g key={index}><line x1={beat.x} x2={beat.x} y1={14} y2={68} stroke={beat.number === 1 ? "#f97316" : "currentColor"} opacity={beat.number === 1 ? 0.9 : 0.35} /><text x={beat.x + 2} y={10} fill="currentColor" fontSize={9}>{beat.number}</text></g>)}
    {cursor >= start && cursor <= start + length && <line x1={(cursor - start) / length * columns} x2={(cursor - start) / length * columns} y1={0} y2={72} stroke="#22c55e" strokeWidth={2} />}
  </svg>;
}
