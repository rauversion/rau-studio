<script lang="ts">
  import { convertFileSrc, invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { open, save } from "@tauri-apps/plugin-dialog";
  import { onDestroy, onMount, tick } from "svelte";

  type Issue = {
    severity: "info" | "warning" | "error";
    code: string;
    track_id?: string;
    playlist_path?: string;
    source_path?: string;
    message: string;
  };

  type Playlist = {
    path: string;
    name: string;
    node_type?: string;
    track_count: number;
    child_count: number;
    track_keys: string[];
  };

  type Validation = {
    tracks_total: number;
    playlists_total: number;
    convert_candidates: number;
    already_aiff: number;
    missing_files: number;
    unreadable_files: number;
    unsupported_tracks: number;
    duplicate_sources: number;
    playlist_reference_errors: number;
    format_counts: Record<string, number>;
    issues: Issue[];
  };

  type ImportResponse = {
    playlists: Playlist[];
    validation: Validation;
  };

  type PlanItem = {
    track_id: string;
    name?: string;
    artist?: string;
    kind?: string;
    source_path?: string;
    target_path?: string;
    action: "convert" | "reuse_existing" | "skip_already_aiff" | "blocked";
    issues: Issue[];
  };

  type Plan = {
    playlists_total: number;
    referenced_tracks_total: number;
    unique_tracks_total: number;
    convert_total: number;
    reuse_existing_total: number;
    skipped_total: number;
    blocked_total: number;
    items: PlanItem[];
    issues: Issue[];
  };

  type ConvertedFile = {
    track_id: string;
    name?: string;
    artist?: string;
    kind?: string;
    source_path: string;
    target_path: string;
    source_exists: boolean;
    target_exists: boolean;
  };

  type AudioFolderResponse = {
    root_path: string;
    recursive: boolean;
    files: AudioFile[];
    skipped_errors: string[];
  };

  type AudioFile = {
    name: string;
    extension: string;
    path: string;
    parent_path: string;
    size_bytes: number;
    modified_ms?: number;
  };

  type PlaylistTrackFile = {
    position: number;
    track_id: string;
    name?: string;
    artist?: string;
    kind?: string;
    source_path?: string;
    source_exists: boolean;
    target_path?: string;
    target_exists: boolean;
  };

  type ConversionStatus =
    | "queued"
    | "running"
    | "converted"
    | "already_converted"
    | "already_aiff"
    | "failed";

  type ConversionProgressEvent = {
    track_id: string;
    name?: string;
    source_path?: string;
    target_path?: string;
    status: ConversionStatus;
    message?: string;
    percent?: number;
    elapsed_seconds?: number;
    speed?: string;
  };

  type ConversionLogEvent = {
    level: "info" | "warning" | "error";
    track_id?: string;
    name?: string;
    message: string;
  };

  type TerminalLog = ConversionLogEvent & {
    id: number;
    time: string;
  };

  type ConversionItemResult = {
    track_id: string;
    name?: string;
    artist?: string;
    source_path?: string;
    target_path?: string;
    status: ConversionStatus;
    message?: string;
  };

  type ConversionBatchResult = {
    items: ConversionItemResult[];
    converted_total: number;
    already_converted_total: number;
    already_aiff_total: number;
    failed_total: number;
  };

  type ExportXmlResult = {
    output_path: string;
    selected_playlist_total: number;
    selected_track_total: number;
    replaced_track_total: number;
  };

  type PlayerState = {
    label: string;
    path: string;
    url: string;
  };

  const savedXmlPathKey = "aifficator.savedXmlPath";
  const recentXmlPathsKey = "aifficator.recentXmlPaths";

  let xmlPath = "";
  let recentXmlPaths: string[] = [];
  let importResult: ImportResponse | null = null;
  let convertedFiles: ConvertedFile[] = [];
  let folderPath = "";
  let folderRecursive = true;
  let audioFiles: AudioFile[] = [];
  let folderSkippedErrors: string[] = [];
  let activePlaylistPath = "";
  let playlistFiles: PlaylistTrackFile[] = [];
  let playlistLoading = false;
  let selectedPlaylists = new Set<string>();
  let plan: Plan | null = null;
  let conversionProgress = new Map<string, ConversionProgressEvent>();
  let conversionResults: ConversionItemResult[] = [];
  let conversionQueue: string[] = [];
  let conversionBusy = false;
  let conversionMessage = "";
  let maxConcurrency = 1;
  let terminalLogs: TerminalLog[] = [];
  let terminalProgressBuckets = new Map<string, number>();
  let terminalExpanded = true;
  let nextTerminalLogId = 1;
  let terminalElement: HTMLDivElement | null = null;
  let unlistenConversionProgress: UnlistenFn | null = null;
  let unlistenConversionLog: UnlistenFn | null = null;
  let player: PlayerState | null = null;
  let audioElement: HTMLAudioElement | null = null;
  let playerPlaying = false;
  let playerCurrentTime = 0;
  let playerDuration = 0;
  let busy = false;
  let errorMessage = "";

  $: playlistRows = importResult?.playlists.filter((playlist) => playlist.node_type === "1") ?? [];
  $: validation = importResult?.validation;
  $: sortedIssues = validation?.issues ?? [];
  $: plannedRows = plan?.items ?? [];
  $: activePlaylist = playlistRows.find((playlist) => playlist.path === activePlaylistPath);
  $: playerProgress = playerDuration > 0 ? Math.min(100, (playerCurrentTime / playerDuration) * 100) : 0;
  $: activeConvertibleTrackIds = playlistFiles
    .filter(canConvertPlaylistFile)
    .map((file) => file.track_id);

  onMount(() => {
    recentXmlPaths = readRecentXmlPaths();
    const savedXmlPath = localStorage.getItem(savedXmlPathKey);

    if (savedXmlPath) {
      xmlPath = savedXmlPath;
      void importXml();
    }

    void listen<ConversionProgressEvent>("conversion-progress", (event) => {
      const next = new Map(conversionProgress);
      next.set(event.payload.track_id, event.payload);
      conversionProgress = next;
      logProgressMilestone(event.payload);
    }).then((unlisten) => {
      unlistenConversionProgress = unlisten;
    });

    void listen<ConversionLogEvent>("conversion-log", (event) => {
      appendTerminalLog(event.payload);
    }).then((unlisten) => {
      unlistenConversionLog = unlisten;
    });
  });

  onDestroy(() => {
    unlistenConversionProgress?.();
    unlistenConversionLog?.();
  });

  async function chooseXml() {
    errorMessage = "";
    plan = null;
    player = null;

    const selected = await open({
      multiple: false,
      filters: [{ name: "Rekordbox XML", extensions: ["xml"] }]
    });

    if (typeof selected !== "string") return;

    xmlPath = selected;
    rememberXmlPath(xmlPath);
    await importXml();
  }

  async function importXml() {
    if (!xmlPath) return;

    busy = true;
    errorMessage = "";
    plan = null;
    player = null;
    convertedFiles = [];
    conversionProgress = new Map<string, ConversionProgressEvent>();
    conversionResults = [];
    conversionQueue = [];
    conversionBusy = false;
    conversionMessage = "";
    terminalLogs = [];
    terminalProgressBuckets = new Map<string, number>();
    activePlaylistPath = "";
    playlistFiles = [];
    selectedPlaylists = new Set<string>();

    try {
      const response = await invoke<ImportResponse>("import_rekordbox_xml", { path: xmlPath });
      importResult = response;
      const firstPlaylist = response.playlists.find((playlist) => playlist.node_type === "1");
      if (firstPlaylist) {
        await selectPlaylist(firstPlaylist.path);
      }
      await refreshConvertedFiles();
    } catch (error) {
      errorMessage = String(error);
    } finally {
      busy = false;
    }
  }

  async function loadRecentXml(path: string) {
    xmlPath = path;
    rememberXmlPath(path);
    await importXml();
  }

  function forgetSavedXml() {
    localStorage.removeItem(savedXmlPathKey);
    xmlPath = "";
    importResult = null;
    convertedFiles = [];
    conversionProgress = new Map<string, ConversionProgressEvent>();
    conversionResults = [];
    conversionQueue = [];
    conversionBusy = false;
    conversionMessage = "";
    terminalLogs = [];
    terminalProgressBuckets = new Map<string, number>();
    activePlaylistPath = "";
    playlistFiles = [];
    selectedPlaylists = new Set<string>();
    plan = null;
  }

  function rememberXmlPath(path: string) {
    localStorage.setItem(savedXmlPathKey, path);
    recentXmlPaths = [path, ...recentXmlPaths.filter((recentPath) => recentPath !== path)].slice(0, 8);
    localStorage.setItem(recentXmlPathsKey, JSON.stringify(recentXmlPaths));
  }

  function readRecentXmlPaths() {
    const raw = localStorage.getItem(recentXmlPathsKey);
    if (!raw) return [];

    try {
      const parsed = JSON.parse(raw);
      return Array.isArray(parsed)
        ? parsed.filter((path): path is string => typeof path === "string")
        : [];
    } catch {
      return [];
    }
  }

  async function chooseFolder() {
    errorMessage = "";

    const selected = await open({
      multiple: false,
      directory: true
    });

    if (typeof selected !== "string") return;

    folderPath = selected;
    await refreshAudioFiles();
  }

  async function refreshAudioFiles() {
    if (!folderPath) return;

    busy = true;
    errorMessage = "";

    try {
      const response = await invoke<AudioFolderResponse>("list_audio_files", {
        folderPath,
        recursive: folderRecursive
      });
      folderPath = response.root_path;
      audioFiles = response.files;
      folderSkippedErrors = response.skipped_errors;
    } catch (error) {
      errorMessage = String(error);
    } finally {
      busy = false;
    }
  }

  function formatSize(bytes: number) {
    if (bytes >= 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
    if (bytes >= 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    if (bytes >= 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${bytes} B`;
  }

  function togglePlaylist(path: string) {
    const next = new Set(selectedPlaylists);
    if (next.has(path)) {
      next.delete(path);
    } else {
      next.add(path);
    }
    selectedPlaylists = next;
  }

  async function selectPlaylist(path: string) {
    if (!xmlPath) return;

    activePlaylistPath = path;
    playlistLoading = true;
    errorMessage = "";

    try {
      playlistFiles = await invoke<PlaylistTrackFile[]>("playlist_tracks", {
        path: xmlPath,
        playlistPath: path
      });
    } catch (error) {
      playlistFiles = [];
      errorMessage = String(error);
    } finally {
      playlistLoading = false;
    }
  }

  async function createPlan() {
    if (!xmlPath) return;

    busy = true;
    errorMessage = "";

    try {
      plan = await invoke<Plan>("plan_conversion", {
        path: xmlPath,
        playlistPaths: Array.from(selectedPlaylists)
      });
    } catch (error) {
      errorMessage = String(error);
    } finally {
      busy = false;
    }
  }

  function convertTrackIds(trackIds: string[]) {
    if (!xmlPath || trackIds.length === 0) return;

    const queuedIds = new Set(conversionQueue);
    const uniqueTrackIds = Array.from(new Set(trackIds)).filter(
      (trackId) => !isTrackConverting(trackId) && !queuedIds.has(trackId)
    );
    if (uniqueTrackIds.length === 0) return;

    conversionQueue = [...conversionQueue, ...uniqueTrackIds];
    conversionMessage = conversionBusy
      ? `${uniqueTrackIds.length} archivo(s) agregados a la cola. ${conversionQueue.length} pendientes.`
      : `${uniqueTrackIds.length} archivo(s) en cola.`;
    errorMessage = "";

    const nextProgress = new Map(conversionProgress);
    for (const trackId of uniqueTrackIds) {
      nextProgress.set(trackId, {
        track_id: trackId,
        status: "queued",
        message: "En cola",
        percent: 0
      });
    }
    conversionProgress = nextProgress;

    void drainConversionQueue();
  }

  async function drainConversionQueue() {
    if (conversionBusy || !xmlPath) return;

    conversionBusy = true;
    errorMessage = "";

    let convertedTotal = 0;
    let alreadyConvertedTotal = 0;
    let alreadyAiffTotal = 0;
    let failedTotal = 0;

    try {
      while (conversionQueue.length > 0) {
        const batch = conversionQueue.slice(0, maxConcurrency);
        conversionQueue = conversionQueue.slice(batch.length);
        conversionMessage =
          `Convirtiendo ${batch.length} archivo(s). ` +
          `${conversionQueue.length} pendientes en cola.`;

        const result = await invoke<ConversionBatchResult>("convert_tracks", {
          path: xmlPath,
          trackIds: batch,
          maxConcurrency
        });

        conversionResults = [...conversionResults, ...result.items];
        convertedTotal += result.converted_total;
        alreadyConvertedTotal += result.already_converted_total;
        alreadyAiffTotal += result.already_aiff_total;
        failedTotal += result.failed_total;
      }

      conversionMessage =
        `${convertedTotal} convertidos, ` +
        `${alreadyConvertedTotal} ya existian, ` +
        `${alreadyAiffTotal} ya eran AIFF, ` +
        `${failedTotal} con error.`;
    } catch (error) {
      errorMessage = String(error);
    } finally {
      conversionBusy = false;
      await refreshConvertedFiles();
      if (activePlaylistPath) {
        await selectPlaylist(activePlaylistPath);
      }
      if (plan) {
        await createPlan();
      }
    }
  }

  async function exportXml() {
    if (!xmlPath) return;

    const playlistPaths =
      selectedPlaylists.size > 0
        ? Array.from(selectedPlaylists)
        : activePlaylistPath
          ? [activePlaylistPath]
          : [];

    if (playlistPaths.length === 0) {
      errorMessage = "Selecciona una playlist o haz click en una playlist activa antes de exportar.";
      return;
    }

    const outputPath = await save({
      defaultPath: defaultExportPath(xmlPath),
      filters: [{ name: "Rekordbox XML", extensions: ["xml"] }]
    });

    if (typeof outputPath !== "string") return;

    busy = true;
    errorMessage = "";
    appendTerminalLog({
      level: "info",
      message: `Exportando XML a ${outputPath}`
    });

    try {
      const result = await invoke<ExportXmlResult>("export_rekordbox_xml", {
        path: xmlPath,
        playlistPaths,
        outputPath
      });
      conversionMessage =
        `XML exportado: ${result.output_path}. ` +
        `${result.replaced_track_total}/${result.selected_track_total} tracks apuntan a AIFF.`;
      appendTerminalLog({
        level: "info",
        message:
          `XML exportado: ${result.output_path}. ` +
          `${result.replaced_track_total}/${result.selected_track_total} tracks reemplazados.`
      });
    } catch (error) {
      errorMessage = String(error);
      appendTerminalLog({
        level: "error",
        message: `Error exportando XML: ${String(error)}`
      });
    } finally {
      busy = false;
    }
  }

  function defaultExportPath(path: string) {
    return path.replace(/\.xml$/i, "") + ".aifficator.aiff.xml";
  }

  function appendTerminalLog(log: ConversionLogEvent) {
    const nextLog: TerminalLog = {
      ...log,
      id: nextTerminalLogId,
      time: new Date().toLocaleTimeString()
    };
    nextTerminalLogId += 1;
    terminalLogs = [...terminalLogs, nextLog].slice(-1000);

    void tick().then(() => {
      if (terminalElement) {
        terminalElement.scrollTop = terminalElement.scrollHeight;
      }
    });
  }

  function logProgressMilestone(progress: ConversionProgressEvent) {
    if (progress.status !== "running" || typeof progress.percent !== "number") return;

    const bucket = Math.floor(progress.percent / 10) * 10;
    if (bucket <= 0) return;

    const previousBucket = terminalProgressBuckets.get(progress.track_id) ?? 0;
    if (bucket <= previousBucket) return;

    const nextBuckets = new Map(terminalProgressBuckets);
    nextBuckets.set(progress.track_id, bucket);
    terminalProgressBuckets = nextBuckets;

    appendTerminalLog({
      level: "info",
      track_id: progress.track_id,
      name: progress.name,
      message: `Progreso ${bucket}%${progress.speed ? ` (${progress.speed})` : ""}`
    });
  }

  function clearTerminal() {
    terminalLogs = [];
  }

  function convertActivePlaylist() {
    void convertTrackIds(playlistFiles.filter(canConvertPlaylistFile).map((file) => file.track_id));
  }

  function canConvertPlaylistFile(file: PlaylistTrackFile) {
    return Boolean(
      file.source_exists &&
        file.source_path &&
        !file.target_exists &&
        !isTrackConverting(file.track_id)
    );
  }

  function trackProgress(trackId: string) {
    return conversionProgress.get(trackId);
  }

  function isTrackConverting(trackId: string) {
    const status = trackProgress(trackId)?.status;
    return status === "queued" || status === "running";
  }

  function isTrackConverted(file: PlaylistTrackFile) {
    const status = trackProgress(file.track_id)?.status;
    return file.target_exists || status === "converted" || status === "already_converted";
  }

  function conversionDotClass(file: PlaylistTrackFile) {
    const status = trackProgress(file.track_id)?.status;

    if (isTrackConverted(file)) return "converted";
    if (status === "failed" || !file.source_exists) return "failed";
    if (status === "queued" || status === "running") return "running";
    return "pending";
  }

  function conversionDotTitle(file: PlaylistTrackFile) {
    const progress = trackProgress(file.track_id);

    if (isTrackConverted(file)) return "Convertido";
    if (progress?.status === "failed") return progress.message ?? "Error de conversion";
    if (progress?.status === "queued") return "En cola";
    if (progress?.status === "running") return progress.message ?? "Convirtiendo";
    if (!file.source_exists) return "Archivo original no encontrado";
    return "Pendiente";
  }

  function conversionButtonLabel(file: PlaylistTrackFile) {
    const status = trackProgress(file.track_id)?.status;

    if (file.target_exists || status === "converted" || status === "already_converted") return "DONE";
    if (status === "failed") return "ERR";
    if (status === "queued" || status === "running") return "...";
    return "CONV";
  }

  function targetLabel(file: PlaylistTrackFile) {
    const progress = trackProgress(file.track_id);

    if (file.target_exists) return file.target_path ?? "Convertido";
    if (progress?.status === "converted" || progress?.status === "already_converted") {
      return progress.target_path ?? file.target_path ?? "Convertido";
    }
    if (progress?.status === "failed") return progress.message ?? "Error";
    if (progress?.status === "queued") return "En cola";
    if (progress?.status === "running") {
      const percent = progress.percent;
      const percentLabel = typeof percent === "number" ? ` ${Math.round(percent)}%` : "";
      const speedLabel = progress.speed ? ` ${progress.speed}` : "";
      return `Convirtiendo${percentLabel}${speedLabel}`;
    }
    return file.target_path ? "Pendiente" : "";
  }

  function progressPercent(trackId: string) {
    const percent = trackProgress(trackId)?.percent;
    return typeof percent === "number" && Number.isFinite(percent)
      ? Math.max(0, Math.min(100, percent))
      : 0;
  }

  function runPlaylistFileAction(file: PlaylistTrackFile, action: string) {
    switch (action) {
      case "aiff":
        if (file.target_path) {
          void togglePathPlayback(file.target_path, file.name ?? file.target_path);
        }
        break;
      case "convert":
        convertTrackIds([file.track_id]);
        break;
      case "find":
        if (file.source_path) {
          void reveal(file.source_path);
        }
        break;
      case "open":
        if (file.source_path) {
          void openFolder(file.source_path);
        }
        break;
    }
  }

  async function refreshConvertedFiles() {
    if (!xmlPath) return;

    try {
      convertedFiles = await invoke<ConvertedFile[]>("list_converted_files", { path: xmlPath });
    } catch (error) {
      errorMessage = String(error);
    }
  }

  async function playPath(path: string, label: string) {
    player = {
      label,
      path,
      url: convertFileSrc(path)
    };
    playerPlaying = false;
    playerCurrentTime = 0;
    playerDuration = 0;

    await tick();

    try {
      audioElement?.load();
      await audioElement?.play();
      playerPlaying = true;
    } catch (error) {
      errorMessage = `No se pudo reproducir ${label}: ${String(error)}`;
    }
  }

  async function togglePlayer() {
    if (!audioElement || !player) return;

    try {
      if (audioElement.paused) {
        await audioElement.play();
        playerPlaying = true;
      } else {
        audioElement.pause();
        playerPlaying = false;
      }
    } catch (error) {
      errorMessage = `No se pudo controlar el player: ${String(error)}`;
    }
  }

  async function togglePathPlayback(path: string, label: string) {
    if (player?.path === path && playerPlaying) {
      stopPlayer();
      return;
    }

    if (player?.path === path) {
      await togglePlayer();
      return;
    }

    await playPath(path, label);
  }

  function stopPlayer() {
    if (audioElement) {
      audioElement.pause();
      audioElement.currentTime = 0;
    }
    playerPlaying = false;
    playerCurrentTime = 0;
  }

  function playbackLabel(path?: string) {
    if (!path || player?.path !== path) return "PLAY";
    return playerPlaying ? "STOP" : "PLAY";
  }

  function playbackIcon(path?: string) {
    return path && player?.path === path && playerPlaying ? "stop" : "play";
  }

  function syncPlayerTime() {
    if (!audioElement) return;

    playerCurrentTime = audioElement.currentTime || 0;
    playerDuration = Number.isFinite(audioElement.duration) ? audioElement.duration : 0;
  }

  function finishPlayback() {
    playerPlaying = false;
    syncPlayerTime();
  }

  function formatTime(seconds: number) {
    if (!Number.isFinite(seconds) || seconds < 0) return "0:00";

    const minutes = Math.floor(seconds / 60);
    const remainingSeconds = Math.floor(seconds % 60)
      .toString()
      .padStart(2, "0");

    return `${minutes}:${remainingSeconds}`;
  }

  async function reveal(path: string) {
    try {
      await invoke("reveal_path", { path });
    } catch (error) {
      errorMessage = String(error);
    }
  }

  async function openFolder(path: string) {
    try {
      await invoke("open_parent_folder", { path });
    } catch (error) {
      errorMessage = String(error);
    }
  }
</script>

<main class="shell" class:terminal-expanded={terminalExpanded}>
  <header class="toolbar">
    <div>
      <h1>Aifficator</h1>
      <p>{xmlPath || "Sin XML cargado"}</p>
    </div>
    <div class="actions">
      <button type="button" on:click={chooseXml} disabled={busy}>Importar XML</button>
      <button type="button" class="secondary" on:click={chooseFolder} disabled={busy}>
        Explorar carpeta
      </button>
      <button type="button" on:click={createPlan} disabled={busy || !importResult}>
        Crear plan
      </button>
      <button
        type="button"
        on:click={exportXml}
        disabled={busy || conversionBusy || !importResult}
      >
        Exportar XML
      </button>
      <label class="select-control">
        <span>Concurrencia</span>
        <select
          value={maxConcurrency}
          disabled={conversionBusy}
          on:change={(event) => (maxConcurrency = Number((event.currentTarget as HTMLSelectElement).value))}
        >
          <option value="1">1</option>
          <option value="2">2</option>
          <option value="3">3</option>
          <option value="4">4</option>
        </select>
      </label>
      {#if xmlPath}
        <button type="button" class="secondary" on:click={forgetSavedXml} disabled={busy}>
          Olvidar XML
        </button>
      {/if}
    </div>
  </header>

  {#if recentXmlPaths.length > 0}
    <section class="recentbar">
      <span>XML recientes</span>
      {#each recentXmlPaths as recentPath}
        <button
          type="button"
          class:activeRecent={recentPath === xmlPath}
          title={recentPath}
          on:click={() => loadRecentXml(recentPath)}
          disabled={busy}
        >
          {recentPath}
        </button>
      {/each}
    </section>
  {/if}

  {#if errorMessage}
    <section class="alert">{errorMessage}</section>
  {/if}

  {#if conversionMessage}
    <section class="notice">{conversionMessage}</section>
  {/if}

  {#if validation}
    <section class="metrics">
      <div>
        <span>Tracks</span>
        <strong>{validation.tracks_total}</strong>
      </div>
      <div>
        <span>Convertibles</span>
        <strong>{validation.convert_candidates}</strong>
      </div>
      <div>
        <span>AIFF</span>
        <strong>{validation.already_aiff}</strong>
      </div>
      <div class:danger={validation.missing_files > 0}>
        <span>No encontrados</span>
        <strong>{validation.missing_files}</strong>
      </div>
      <div class:danger={validation.unsupported_tracks > 0}>
        <span>No soportados</span>
        <strong>{validation.unsupported_tracks}</strong>
      </div>
      <div class:danger={validation.playlist_reference_errors > 0}>
        <span>Refs rotas</span>
        <strong>{validation.playlist_reference_errors}</strong>
      </div>
    </section>
  {/if}

  {#if plan}
    <section class="planbar">
      <span>{plan.playlists_total} playlists</span>
      <span>{plan.unique_tracks_total} tracks unicos</span>
      <span>{plan.convert_total} conversiones</span>
      <span>{plan.reuse_existing_total} reutilizados</span>
      <span>{plan.skipped_total} omitidos</span>
      <span class:danger={plan.blocked_total > 0}>{plan.blocked_total} bloqueados</span>
    </section>
  {/if}

  <section class="playerbar" class:idle={!player}>
    <button type="button" class="play-toggle" disabled={!player} on:click={togglePlayer}>
      {playerPlaying ? "Pause" : "Play"}
    </button>
    <div>
      <span>Player</span>
      <strong title={player?.path ?? ""}>{player?.label ?? "Sin archivo cargado"}</strong>
    </div>
    <div class="player-progress">
      <div class="time-row">
        <span>{formatTime(playerCurrentTime)}</span>
        <span>{formatTime(playerDuration)}</span>
      </div>
      <div class="progress-track" aria-label="Progreso de reproduccion">
        <div class="progress-fill" style={`width: ${playerProgress}%`}></div>
      </div>
    </div>
    {#if player}
      <audio
        bind:this={audioElement}
        src={player.url}
        on:loadedmetadata={syncPlayerTime}
        on:timeupdate={syncPlayerTime}
        on:play={() => (playerPlaying = true)}
        on:pause={() => (playerPlaying = false)}
        on:ended={finishPlayback}
      ></audio>
    {/if}
    <button
      type="button"
      class="secondary"
      disabled={!player}
      on:click={() => player && reveal(player.path)}
    >
      Finder
    </button>
  </section>

  <section class="panel browser">
    <div class="panel-title">
      <div>
        <h2>Originales</h2>
        <span title={folderPath}>{folderPath || "Sin carpeta seleccionada"}</span>
      </div>
      <div class="title-actions">
        <label class="toggle">
          <input
            type="checkbox"
            checked={folderRecursive}
            on:change={(event) => {
              folderRecursive = (event.currentTarget as HTMLInputElement).checked;
              void refreshAudioFiles();
            }}
          />
          Recursivo
        </label>
        <span>{audioFiles.length} archivos</span>
        <button type="button" class="secondary compact" disabled={busy || !folderPath} on:click={refreshAudioFiles}>
          Refrescar
        </button>
        <button type="button" class="compact" disabled={busy} on:click={chooseFolder}>
          Elegir
        </button>
      </div>
    </div>

    <div class="file-table browser-table">
      <div class="file-header">
        <span>Archivo</span>
        <span>Formato</span>
        <span>Tamano</span>
        <span>Carpeta</span>
        <span>Acciones</span>
      </div>

      {#if !folderPath}
        <div class="empty-row">Elige una carpeta para navegar archivos FLAC, MP3, WAV, AIFF, ALAC, M4A o AAC.</div>
      {:else if audioFiles.length === 0}
        <div class="empty-row">No se encontraron archivos de audio originales en esta carpeta.</div>
      {/if}

      {#each audioFiles as file}
        <div class="file-row">
          <span title={file.path}>{file.name}</span>
          <span>{file.extension.toUpperCase()}</span>
          <span>{formatSize(file.size_bytes)}</span>
          <span title={file.parent_path}>{file.parent_path}</span>
          <div class="row-actions">
            <button
              type="button"
              class="icon-button"
              title="Escuchar archivo"
              on:click={() => togglePathPlayback(file.path, file.name)}
            >
              {playbackLabel(file.path)}
            </button>
            <button
              type="button"
              class="icon-button"
              title="Mostrar en Finder"
              on:click={() => reveal(file.path)}
            >
              FIND
            </button>
            <button
              type="button"
              class="icon-button"
              title="Abrir carpeta"
              on:click={() => openFolder(file.path)}
            >
              OPEN
            </button>
          </div>
        </div>
      {/each}
    </div>

    {#if folderSkippedErrors.length > 0}
      <div class="minor-alert">{folderSkippedErrors.length} carpetas o archivos no se pudieron leer.</div>
    {/if}
  </section>

  {#if importResult}
    <section class="workspace">
      <aside class="panel playlists">
        <div class="panel-title">
          <h2>Playlists</h2>
          <span>{selectedPlaylists.size} seleccionadas</span>
        </div>
        <div class="list">
          {#each playlistRows as playlist}
            <div class="playlist-row" class:active-playlist={playlist.path === activePlaylistPath}>
              <input
                type="checkbox"
                checked={selectedPlaylists.has(playlist.path)}
                on:change={() => togglePlaylist(playlist.path)}
              />
              <button
                type="button"
                class="playlist-name"
                title={playlist.path}
                on:click={() => selectPlaylist(playlist.path)}
              >
                {playlist.path}
              </button>
              <em>{playlist.track_count}</em>
            </div>
          {/each}
        </div>
      </aside>

      <section class="detail-stack">
        <section class="panel playlist-files">
          <div class="panel-title">
            <div>
              <h2>Playlist</h2>
              <span title={activePlaylistPath}>{activePlaylistPath || "Sin playlist seleccionada"}</span>
            </div>
            <div class="title-actions">
              <span>{playlistFiles.length} archivos</span>
              <button
                type="button"
                class="compact"
                disabled={playlistLoading || activeConvertibleTrackIds.length === 0}
                on:click={convertActivePlaylist}
              >
                Convertir playlist
              </button>
              <button
                type="button"
                class="secondary compact"
                disabled={playlistLoading || !activePlaylistPath}
                on:click={() => selectPlaylist(activePlaylistPath)}
              >
                Refrescar
              </button>
              {#if activePlaylist && !selectedPlaylists.has(activePlaylist.path)}
                <button
                  type="button"
                  class="compact"
                  on:click={() => togglePlaylist(activePlaylist.path)}
                >
                  Seleccionar
                </button>
              {/if}
            </div>
          </div>

          <div class="file-table">
            <div class="playlist-file-header">
              <span></span>
              <span>#</span>
              <span>Tema</span>
              <span>Artista</span>
              <span>Formato</span>
              <span>Original</span>
              <span>AIFF</span>
              <span>Acciones</span>
            </div>

            {#if playlistLoading}
              <div class="empty-row">Cargando playlist...</div>
            {:else if !activePlaylistPath}
              <div class="empty-row">Haz click en una playlist para ver sus archivos.</div>
            {:else if playlistFiles.length === 0}
              <div class="empty-row">Esta playlist no tiene archivos.</div>
            {/if}

            {#each playlistFiles as file}
              <div class="playlist-file-row" class:missing={!file.source_exists}>
                <span class="play-cell">
                  <button
                    type="button"
                    class="row-play-button"
                    class:active={file.source_path && player?.path === file.source_path && playerPlaying}
                    title={playbackIcon(file.source_path) === "stop" ? "Detener original" : "Escuchar original"}
                    disabled={!file.source_exists || !file.source_path}
                    on:click={() =>
                      file.source_path &&
                      togglePathPlayback(file.source_path, file.name ?? file.source_path)}
                  >
                    <span class={`playback-glyph ${playbackIcon(file.source_path)}`}></span>
                  </button>
                </span>
                <span>{file.position}</span>
                <span class="track-cell" title={file.name ?? file.track_id}>
                  <span
                    class={`status-dot ${conversionDotClass(file)}`}
                    title={conversionDotTitle(file)}
                  ></span>
                  <span class="clip-text">{file.name ?? file.track_id}</span>
                </span>
                <span title={file.artist ?? ""}>{file.artist ?? ""}</span>
                <span>{file.kind ?? ""}</span>
                <span title={file.source_path ?? ""}>{file.source_path ?? "No encontrado"}</span>
                <span class="target-cell" title={trackProgress(file.track_id)?.message ?? file.target_path ?? ""}>
                  <span class="clip-text">{targetLabel(file)}</span>
                  {#if isTrackConverting(file.track_id)}
                    <span class="inline-progress" aria-label="Progreso de conversion">
                      <span style={`width: ${progressPercent(file.track_id)}%`}></span>
                    </span>
                  {/if}
                </span>
                <select
                  class="action-select"
                  title={trackProgress(file.track_id)?.message ?? "Acciones"}
                  value=""
                  on:change={(event) => {
                    const select = event.currentTarget as HTMLSelectElement;
                    runPlaylistFileAction(file, select.value);
                    select.value = "";
                  }}
                >
                  <option value="">Acciones</option>
                  <option value="aiff" disabled={!file.target_exists || !file.target_path}>
                    AIFF
                  </option>
                  <option value="convert" disabled={!canConvertPlaylistFile(file)}>
                    {conversionButtonLabel(file)}
                  </option>
                  <option value="find" disabled={!file.source_exists || !file.source_path}>
                    Finder
                  </option>
                  <option value="open" disabled={!file.source_path}>Abrir carpeta</option>
                </select>
              </div>
            {/each}
          </div>
        </section>

        <section class="panel converted">
          <div class="panel-title">
            <h2>Convertidos</h2>
            <div class="title-actions">
              <span>{convertedFiles.length} AIFF detectados</span>
              <button
                type="button"
                class="secondary compact"
                disabled={busy}
                on:click={refreshConvertedFiles}
              >
                Refrescar
              </button>
            </div>
          </div>
          <div class="file-table">
            <div class="file-header">
              <span>Tema</span>
              <span>Artista</span>
              <span>Formato</span>
              <span>AIFF</span>
              <span>Acciones</span>
            </div>

            {#if convertedFiles.length === 0}
              <div class="empty-row">No hay AIFF convertidos detectados para este XML.</div>
            {/if}

            {#each convertedFiles as file}
              <div class="file-row">
                <span class="track-cell" title={file.name ?? file.track_id}>
                  <span class="status-dot converted" title="Convertido"></span>
                  <span class="clip-text">{file.name ?? file.track_id}</span>
                </span>
                <span title={file.artist ?? ""}>{file.artist ?? ""}</span>
                <span title={file.source_path}>{file.kind ?? ""}</span>
                <span title={file.target_path}>{file.target_path}</span>
                <div class="row-actions">
                  <button
                    type="button"
                    class="icon-button"
                    title="Escuchar AIFF"
                    on:click={() => togglePathPlayback(file.target_path, file.name ?? file.target_path)}
                  >
                    {file.target_path && player?.path === file.target_path ? playbackLabel(file.target_path) : "AIFF"}
                  </button>
                  <button
                    type="button"
                    class="icon-button"
                    title="Escuchar original"
                    disabled={!file.source_exists}
                    on:click={() => togglePathPlayback(file.source_path, file.name ?? file.source_path)}
                  >
                    {playbackLabel(file.source_path)}
                  </button>
                  <button
                    type="button"
                    class="icon-button"
                    title="Mostrar en Finder"
                    on:click={() => reveal(file.target_path)}
                  >
                    FIND
                  </button>
                  <button
                    type="button"
                    class="icon-button"
                    title="Abrir carpeta"
                    on:click={() => openFolder(file.target_path)}
                  >
                    OPEN
                  </button>
                </div>
              </div>
            {/each}
          </div>
        </section>

        {#if plan}
          <section class="panel plan">
            <div class="panel-title">
              <h2>Plan seleccionado</h2>
              <span>{plannedRows.length} tracks</span>
            </div>
            <div class="file-table">
              <div class="plan-header">
                <span>Tema</span>
                <span>Estado</span>
                <span>Destino</span>
                <span>Acciones</span>
              </div>
              {#each plannedRows as item}
                <div class="plan-row {item.action}">
                  <span title={item.name ?? item.track_id}>{item.name ?? item.track_id}</span>
                  <span>{item.action}</span>
                  <span title={item.target_path ?? item.source_path ?? ""}>
                    {item.target_path ?? item.source_path ?? ""}
                  </span>
                  <div class="row-actions">
                    <button
                      type="button"
                      class="icon-button"
                      title="Escuchar original"
                      disabled={!item.source_path}
                      on:click={() =>
                        item.source_path && togglePathPlayback(item.source_path, item.name ?? item.source_path)}
                    >
                      {playbackLabel(item.source_path)}
                    </button>
                    <button
                      type="button"
                      class="icon-button"
                      title="Escuchar AIFF existente"
                      disabled={item.action !== "reuse_existing" || !item.target_path}
                      on:click={() =>
                        item.target_path && togglePathPlayback(item.target_path, item.name ?? item.target_path)}
                    >
                      {item.target_path && player?.path === item.target_path ? playbackLabel(item.target_path) : "AIFF"}
                    </button>
                    <button
                      type="button"
                      class="icon-button"
                      title="Abrir carpeta de destino"
                      disabled={!item.target_path}
                      on:click={() => item.target_path && openFolder(item.target_path)}
                    >
                      OPEN
                    </button>
                  </div>
                </div>
              {/each}
            </div>
          </section>
        {/if}

        <section class="panel issues">
          <div class="panel-title">
            <h2>Reporte</h2>
            <span>{sortedIssues.length} hallazgos</span>
          </div>
          <div class="issue-table">
            <div class="issue-header">
              <span>Severidad</span>
              <span>Codigo</span>
              <span>Track</span>
              <span>Mensaje</span>
            </div>
            {#each sortedIssues as issue}
              <div class="issue-row {issue.severity}">
                <span>{issue.severity}</span>
                <span>{issue.code}</span>
                <span>{issue.track_id ?? ""}</span>
                <span title={issue.message}>{issue.message}</span>
              </div>
            {/each}
          </div>
        </section>
      </section>
    </section>
  {/if}

  <section class="panel terminal-panel" class:expanded={terminalExpanded}>
    <div class="panel-title">
      <div>
        <h2>Terminal</h2>
        <span>{terminalLogs.length} eventos</span>
      </div>
      <div class="title-actions">
        <span>ffmpeg / conversion / export</span>
        <button
          type="button"
          class="secondary compact"
          on:click={() => (terminalExpanded = !terminalExpanded)}
        >
          {terminalExpanded ? "Contraer" : "Expandir"}
        </button>
        <button type="button" class="secondary compact" on:click={clearTerminal}>
          Limpiar
        </button>
      </div>
    </div>
    <div class="terminal-output" bind:this={terminalElement}>
      {#if terminalLogs.length === 0}
        <div class="terminal-empty">Sin eventos todavia.</div>
      {/if}
      {#each terminalLogs as log}
        <div class={`terminal-line ${log.level}`}>
          <span class="terminal-time">{log.time}</span>
          <span class="terminal-level">{log.level.toUpperCase()}</span>
          <span class="terminal-track" title={log.track_id ?? ""}>
            {log.name ?? log.track_id ?? "system"}
          </span>
          <span class="terminal-message">{log.message}</span>
        </div>
      {/each}
    </div>
  </section>
</main>
