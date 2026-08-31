import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  AlertTriangle,
  ArrowDown,
  ArrowUp,
  AudioLines,
  Calendar,
  Camera,
  Check,
  ChevronsUpDown,
  GripVertical,
  Info,
  Library,
  LoaderCircle,
  Mic,
  MicOff,
  Monitor,
  Music2,
  Play,
  Plus,
  Radio,
  RefreshCcw,
  Save,
  SlidersHorizontal,
  SkipForward,
  Square,
  Trash2,
  Wifi
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState, type FormEvent, type ReactNode } from "react";
import { Button, type ButtonProps } from "./components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "./components/ui/card";
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList
} from "./components/ui/command";
import { Popover, PopoverContent, PopoverTrigger } from "./components/ui/popover";
import { TerminalDrawer, type TerminalLogEntry } from "./components/terminal-drawer";
import { translateBackendMessage, useI18n } from "./i18n";
import { cn } from "./lib/utils";

const SYSTEM_AUDIO_TARGET_ID = "__system_audio__";

type BroadcastProfile = {
  id: string;
  name: string;
  active: boolean;
  output_kind: "icecast" | "rtmp" | string;
  host: string;
  port: number;
  mount: string;
  username: string;
  station_name: string;
  description: string;
  bitrate_kbps: number;
  tls: boolean;
  public: boolean;
  microphone_enabled: boolean;
  microphone_device: string;
  microphone_gain_percent: number;
  line_input_enabled: boolean;
  line_input_device: string;
  line_input_channel: number;
  line_input_stereo: boolean;
  line_input_gain_percent: number;
  application_audio_enabled: boolean;
  application_audio_bundle_id: string;
  application_audio_gain_percent: number;
  rtmp_platform: "instagram" | "custom" | string;
  rtmp_server_url: string;
  rtmp_video_bitrate_kbps: number;
  rtmp_audio_bitrate_kbps: number;
  video_compositor: BroadcastVideoCompositor;
  password_configured: boolean;
  listener_url: string;
  updated_at: string;
};

type BroadcastControlSettings = {
  microphone_enabled: boolean;
  microphone_device: string;
  microphone_gain_percent: number;
  line_input_enabled: boolean;
  line_input_device: string;
  line_input_channel: number;
  line_input_stereo: boolean;
  line_input_gain_percent: number;
  application_audio_enabled: boolean;
  application_audio_bundle_id: string;
  application_audio_gain_percent: number;
  updated_at: string;
};

type BroadcastVideoCompositor = {
  enabled: boolean;
  graphicTemplate: "signal_grid" | "transmission" | "mono_paper" | string;
  captureMode: "native" | "browser" | string;
  cameraEnabled: boolean;
  cameraDevice: string;
  cameraPosition: "top_left" | "top_right" | "center" | "bottom_left" | "bottom_right" | string;
  cameraSize: "small" | "medium" | "large" | string;
  cameraEffect: "clean" | "mono" | "contrast" | "dream" | string;
  cameraMirror: boolean;
  cameraRotationDegrees: 0 | 90 | 180 | 270 | number;
  cameraFraming: "contain" | "cover" | string;
  cameraLayout: "card" | "wide" | "background" | "free" | string;
  cameraX: number;
  cameraY: number;
  cameraWidth: number;
  cameraHeight: number;
  cameraZIndex: number;
  cameraOpacityPercent: number;
  screenEnabled: boolean;
  screenLabel: string;
  screenPosition: "top_left" | "top_right" | "center" | "bottom_left" | "bottom_right" | string;
  screenSize: "small" | "medium" | "large" | string;
  screenEffect: "clean" | "mono" | "contrast" | "dream" | string;
  screenMirror: boolean;
  screenRotationDegrees: number;
  screenFraming: "contain" | "cover" | string;
  screenLayout: "card" | "wide" | "background" | "free" | string;
  screenX: number;
  screenY: number;
  screenWidth: number;
  screenHeight: number;
  screenZIndex: number;
  screenOpacityPercent: number;
  transitionMillis: number;
};

type BroadcastPreflight = {
  ffmpeg_available: boolean;
  mp3_encoder_available: boolean;
  icecast_protocol_available: boolean;
  tls_protocol_available: boolean;
  h264_encoder_available: boolean;
  aac_encoder_available: boolean;
  rtmp_protocol_available: boolean;
  rtmps_protocol_available: boolean;
  flv_muxer_available: boolean;
  visualizer_filter_available: boolean;
  overlay_filter_available: boolean;
  camera_input_available: boolean;
  camera_filter_available: boolean;
  microphone_input_available: boolean;
  ready: boolean;
  message: string;
};

type BroadcastMicrophoneDevice = {
  id: string;
  label: string;
  is_default: boolean;
  input_channels: number;
};

type BroadcastApplicationAudioDevice = {
  id: string;
  label: string;
  process_id: number;
};

type BroadcastApplicationAudioSupport = {
  supported: boolean;
  minimum_macos_version: string;
  current_macos_version?: string | null;
  message: string;
};

type BroadcastCameraDevice = {
  id: string;
  label: string;
  kind: "camera" | "screen" | string;
};

type BroadcastMicrophoneStatus = {
  configured: boolean;
  ready: boolean;
  live: boolean;
  receiving_audio: boolean;
  level_percent: number;
  device?: string | null;
  gain_percent: number;
  message: string;
};

type BroadcastLineInputStatus = {
  configured: boolean;
  ready: boolean;
  live: boolean;
  receiving_audio: boolean;
  level_percent: number;
  device?: string | null;
  channel: number;
  stereo: boolean;
  gain_percent: number;
  message: string;
};

type BroadcastLineInputPreview = {
  active: boolean;
  receiving_audio: boolean;
  level_percent: number;
  device: string;
  channel: number;
  stereo: boolean;
  message: string;
};

type BroadcastApplicationAudioStatus = {
  configured: boolean;
  ready: boolean;
  live: boolean;
  receiving_audio: boolean;
  level_percent: number;
  application?: string | null;
  label?: string | null;
  gain_percent: number;
  message: string;
};

type BroadcastQueueEntry = {
  id: string;
  library_id: string;
  track_id: string;
  playlist_path: string;
  playlist_name: string;
  source_path: string;
  title: string;
  artist?: string | null;
  duration_seconds?: number | null;
  position: number;
  status: "queued" | "playing" | "played" | "skipped" | "failed" | string;
  error?: string | null;
  scheduled: boolean;
  inserted_at: string;
  updated_at: string;
};

type BroadcastStatus = {
  status: "idle" | "connecting" | "live" | "reconnecting" | "stopping" | "error" | string;
  message: string;
  now_playing?: BroadcastQueueEntry | null;
  started_at?: string | null;
  source_mode: "playlist" | "line_input" | "application_audio" | string;
  microphone: BroadcastMicrophoneStatus;
  line_input: BroadcastLineInputStatus;
  application_audio: BroadcastApplicationAudioStatus;
  camera: {
    configured: boolean;
    ready: boolean;
    live: boolean;
    mix_percent: number;
    device?: string | null;
    label?: string | null;
    transition_millis: number;
    message: string;
  };
  updated_at: string;
};

type BroadcastProgressEvent = {
  level: "info" | "warning" | "error" | string;
  event: string;
  message: string;
  status: BroadcastStatus;
  timestamp: string;
};

type PlaylistIndexLibrary = {
  id: string;
  source_name: string;
  track_count: number;
  playlist_count: number;
};

type PlaylistIndexPlaylist = {
  library_id: string;
  path: string;
  name: string;
  track_count: number;
  position: number;
};

type PlaylistDraft = {
  id: string;
  library_id: string;
  name: string;
  description?: string | null;
  track_count: number;
};

type BroadcastPlaylistSource = {
  key: string;
  kind: "local" | "rekordbox";
  id: string;
  library_id: string;
  library_name: string;
  name: string;
  track_count: number;
};

type QueueAppendResult = {
  appended_total: number;
  skipped_missing_total: number;
  queue: BroadcastQueueEntry[];
};

type BroadcastScheduleItem = {
  id: string;
  start_at: string;
  policy: "soft" | "exact" | string;
  source_kind: "playlist" | "draft" | "track" | string;
  library_id: string;
  source_id: string;
  source_name: string;
  track_count: number;
  duration_seconds?: number | null;
  status: "pending" | "activated" | "skipped" | "failed" | string;
  error?: string | null;
  activated_at?: string | null;
  created_at: string;
  updated_at: string;
  tracks: BroadcastScheduleTrack[];
};

type BroadcastScheduleTrack = {
  id: string;
  library_id: string;
  track_id: string;
  source_path: string;
  title: string;
  artist?: string | null;
  duration_seconds?: number | null;
  position: number;
};

type BroadcastAutomationSettings = {
  mode: "immediate" | "scheduled" | string;
  bed_enabled: boolean;
  bed_library_id?: string | null;
  bed_track_id?: string | null;
  bed_source_path?: string | null;
  bed_title?: string | null;
  bed_artist?: string | null;
  bed_gain_percent: number;
  bed_ducking: boolean;
  updated_at: string;
};

type BroadcastSourceTrack = {
  library_id: string;
  track_id: string;
  name?: string | null;
  artist?: string | null;
  source_path?: string | null;
  source_exists: boolean;
  total_time?: number | null;
};

type BusyAction = "loading" | "saving" | "starting" | "stopping" | "skipping" | "appending" | "clearing" | string | null;
type BroadcastSourceTab = "microphone" | "line_input" | "system_audio";
type BroadcastOutputKind = "icecast" | "rtmp";
type RtmpPlatform = "instagram" | "custom";
type BroadcastWorkspaceTab = "destinations" | "control" | "schedule";

const defaultVideoCompositor: BroadcastVideoCompositor = {
  enabled: false,
  graphicTemplate: "signal_grid",
  captureMode: "browser",
  cameraEnabled: true,
  cameraDevice: "default",
  cameraPosition: "top_right",
  cameraSize: "medium",
  cameraEffect: "mono",
  cameraMirror: true,
  cameraRotationDegrees: 180,
  cameraFraming: "contain",
  cameraLayout: "wide",
  cameraX: 0,
  cameraY: 120,
  cameraWidth: 360,
  cameraHeight: 225,
  cameraZIndex: 2,
  cameraOpacityPercent: 100,
  screenEnabled: false,
  screenLabel: "",
  screenPosition: "top_left",
  screenSize: "large",
  screenEffect: "clean",
  screenMirror: false,
  screenRotationDegrees: 0,
  screenFraming: "contain",
  screenLayout: "background",
  screenX: 0,
  screenY: 110,
  screenWidth: 360,
  screenHeight: 340,
  screenZIndex: 1,
  screenOpacityPercent: 100,
  transitionMillis: 800
};

const defaultAutomationSettings: BroadcastAutomationSettings = {
  mode: "immediate",
  bed_enabled: false,
  bed_library_id: null,
  bed_track_id: null,
  bed_source_path: null,
  bed_title: null,
  bed_artist: null,
  bed_gain_percent: 25,
  bed_ducking: true,
  updated_at: ""
};

const broadcastGraphicTemplates = [
  {
    id: "signal_grid",
    name: "Signal Grid",
    description: "Monocromo técnico",
    swatch: "linear-gradient(135deg,#060807 0 46%,#737773 46% 60%,#f4f4ef 60%)"
  },
  {
    id: "transmission",
    name: "Transmission",
    description: "Marfil · acid · rojo",
    swatch: "linear-gradient(180deg,#f1efe6 0 26%,#d7ff00 26% 38%,#ff4b2b 38% 67%,#0b0b0b 67%)"
  },
  {
    id: "mono_paper",
    name: "Mono Paper",
    description: "Editorial mínimo",
    swatch: "linear-gradient(135deg,#0b0b0b 0 38%,#eeece3 38% 76%,#ff4b2b 76%)"
  }
] as const;

const fieldClass =
  "h-10 w-full rounded-md border border-border bg-background px-3 text-sm text-foreground outline-none transition focus:border-foreground/35 focus:ring-2 focus:ring-ring/30 disabled:cursor-not-allowed disabled:opacity-60";

async function loadBroadcastPlaylistSources(): Promise<BroadcastPlaylistSource[]> {
  const [libraries, drafts] = await Promise.all([
    invoke<PlaylistIndexLibrary[]>("playlist_index_libraries"),
    invoke<PlaylistDraft[]>("playlist_index_drafts", { libraryId: null })
  ]);
  const indexedPlaylists = await Promise.all(
    libraries.map((library) => invoke<PlaylistIndexPlaylist[]>("playlist_index_library_playlists", { libraryId: library.id }))
  );
  const libraryNames = new Map(libraries.map((library) => [library.id, library.source_name]));
  const localSources = drafts
    .filter((draft) => draft.track_count > 0)
    .map((draft): BroadcastPlaylistSource => ({
      key: `local:${draft.id}`,
      kind: "local",
      id: draft.id,
      library_id: draft.library_id,
      library_name: libraryNames.get(draft.library_id) ?? draft.library_id,
      name: draft.name,
      track_count: draft.track_count
    }));
  const rekordboxSources = libraries.flatMap((library, libraryIndex) =>
    indexedPlaylists[libraryIndex]
      .filter((playlist) => playlist.track_count > 0)
      .map((playlist): BroadcastPlaylistSource => ({
        key: `rekordbox:${library.id}:${playlist.path}`,
        kind: "rekordbox",
        id: playlist.path,
        library_id: library.id,
        library_name: library.source_name,
        name: playlist.name,
        track_count: playlist.track_count
      }))
  );
  return [...localSources, ...rekordboxSources];
}

export function BroadcastPage() {
  const { locale, t } = useI18n();
  const [profile, setProfile] = useState<BroadcastProfile | null>(null);
  const [profiles, setProfiles] = useState<BroadcastProfile[]>([]);
  const [workspaceTab, setWorkspaceTab] = useState<BroadcastWorkspaceTab>("control");
  const [profileName, setProfileName] = useState("Destino principal");
  const [creatingDestination, setCreatingDestination] = useState(false);
  const [newDestinationName, setNewDestinationName] = useState("");
  const [preflight, setPreflight] = useState<BroadcastPreflight | null>(null);
  const [status, setStatus] = useState<BroadcastStatus | null>(null);
  const [queue, setQueue] = useState<BroadcastQueueEntry[]>([]);
  const [scheduleItems, setScheduleItems] = useState<BroadcastScheduleItem[]>([]);
  const [automation, setAutomation] = useState<BroadcastAutomationSettings>(defaultAutomationSettings);
  const [playlistSources, setPlaylistSources] = useState<BroadcastPlaylistSource[]>([]);
  const [microphoneDevices, setMicrophoneDevices] = useState<BroadcastMicrophoneDevice[]>([]);
  const [applicationAudioDevices, setApplicationAudioDevices] = useState<BroadcastApplicationAudioDevice[]>([]);
  const [applicationAudioSupport, setApplicationAudioSupport] = useState<BroadcastApplicationAudioSupport | null>(null);
  const [cameraDevices, setCameraDevices] = useState<BroadcastCameraDevice[]>([]);
  const [playlistSourceKey, setPlaylistSourceKey] = useState("");
  const [playlistComboboxOpen, setPlaylistComboboxOpen] = useState(false);
  const [terminalLogs, setTerminalLogs] = useState<TerminalLogEntry[]>([]);
  const [terminalExpanded, setTerminalExpanded] = useState(false);
  const [busy, setBusy] = useState<BusyAction>("loading");
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [draggedQueueEntryId, setDraggedQueueEntryId] = useState<string | null>(null);

  const [outputKind, setOutputKind] = useState<BroadcastOutputKind>("icecast");
  const [host, setHost] = useState("");
  const [port, setPort] = useState("8000");
  const [mount, setMount] = useState("/live.mp3");
  const [username, setUsername] = useState("source");
  const [stationName, setStationName] = useState("Rau Studio Radio");
  const [description, setDescription] = useState("");
  const [bitrate, setBitrate] = useState("128");
  const [tls, setTls] = useState(false);
  const [isPublic, setIsPublic] = useState(false);
  const [password, setPassword] = useState("");
  const [clearPassword, setClearPassword] = useState(false);
  const [rtmpPlatform, setRtmpPlatform] = useState<RtmpPlatform>("instagram");
  const [rtmpServerUrl, setRtmpServerUrl] = useState("");
  const [rtmpVideoBitrate, setRtmpVideoBitrate] = useState("3500");
  const [rtmpAudioBitrate, setRtmpAudioBitrate] = useState("128");
  const [streamKey, setStreamKey] = useState("");
  const [microphoneEnabled, setMicrophoneEnabled] = useState(false);
  const [microphoneDevice, setMicrophoneDevice] = useState("default");
  const [microphoneGain, setMicrophoneGain] = useState("100");
  const [lineInputEnabled, setLineInputEnabled] = useState(false);
  const [lineInputDevice, setLineInputDevice] = useState("default");
  const [lineInputChannel, setLineInputChannel] = useState("1");
  const [lineInputStereo, setLineInputStereo] = useState(true);
  const [lineInputGain, setLineInputGain] = useState("100");
  const [lineInputPreview, setLineInputPreview] = useState<BroadcastLineInputPreview | null>(null);
  const [applicationAudioEnabled, setApplicationAudioEnabled] = useState(false);
  const [applicationAudioBundleId, setApplicationAudioBundleId] = useState("");
  const [applicationAudioGain, setApplicationAudioGain] = useState("100");
  const [videoCompositor, setVideoCompositor] = useState<BroadcastVideoCompositor>(defaultVideoCompositor);
  const [videoStudioOpen, setVideoStudioOpen] = useState(false);
  const [cameraMix, setCameraMix] = useState(0);
  const [sourceTab, setSourceTab] = useState<BroadcastSourceTab>("microphone");
  const terminalElement = useRef<HTMLDivElement | null>(null);
  const queueScrollElement = useRef<HTMLDivElement | null>(null);
  const playingQueueEntryElement = useRef<HTMLDivElement | null>(null);
  const lastAutoScrolledQueueEntryId = useRef<string | null>(null);
  const nextTerminalLogId = useRef(1);

  const running = status ? ["connecting", "live", "reconnecting", "stopping"].includes(status.status) : false;
  const runningRef = useRef(running);
  const compositorSaveTimer = useRef<number | null>(null);
  const compositorSaveRevision = useRef(0);
  runningRef.current = running;
  const destinationNeedsSave = !profile
    || profileName.trim() !== profile.name
    || outputKind !== (profile.output_kind === "rtmp" ? "rtmp" : "icecast")
    || stationName.trim() !== profile.station_name
    || description.trim() !== profile.description
    || Boolean(password)
    || clearPassword
    || (outputKind === "icecast" && (
      host.trim() !== profile.host
      || Number(port) !== profile.port
      || mount.trim() !== profile.mount
      || username.trim() !== profile.username
      || Number(bitrate) !== profile.bitrate_kbps
      || tls !== profile.tls
      || isPublic !== profile.public
    ))
    || (outputKind === "rtmp" && (
      rtmpPlatform !== (profile.rtmp_platform === "custom" ? "custom" : "instagram")
      || rtmpServerUrl.trim() !== profile.rtmp_server_url
      || Number(rtmpVideoBitrate) !== profile.rtmp_video_bitrate_kbps
      || Number(rtmpAudioBitrate) !== profile.rtmp_audio_bitrate_kbps
      || JSON.stringify(videoCompositor) !== JSON.stringify(profile.video_compositor)
    ));
  const controlNeedsSave = !profile
    || microphoneEnabled !== profile.microphone_enabled
    || microphoneDevice !== (profile.microphone_device || "default")
    || Number(microphoneGain) !== profile.microphone_gain_percent
    || lineInputEnabled !== profile.line_input_enabled
    || lineInputDevice !== (profile.line_input_device || "default")
    || Number(lineInputChannel) !== profile.line_input_channel
    || lineInputStereo !== profile.line_input_stereo
    || Number(lineInputGain) !== profile.line_input_gain_percent
    || applicationAudioEnabled !== profile.application_audio_enabled
    || applicationAudioBundleId !== (profile.application_audio_bundle_id || SYSTEM_AUDIO_TARGET_ID)
    || Number(applicationAudioGain) !== profile.application_audio_gain_percent;
  const modeQueue = queue.filter((entry) => automation.mode === "scheduled" ? entry.scheduled : !entry.scheduled);
  const queuedEntries = modeQueue.filter((entry) => entry.status === "queued");
  const playingQueueEntryId = status?.now_playing?.id
    ?? modeQueue.find((entry) => entry.status === "playing")?.id
    ?? null;
  const playingQueueEntryVisible = Boolean(
    playingQueueEntryId && modeQueue.some((entry) => entry.id === playingQueueEntryId)
  );
  const queuedTotal = queuedEntries.length;
  const completedTotal = modeQueue.filter((entry) => entry.status === "played").length;
  const failedTotal = modeQueue.filter((entry) => entry.status === "failed").length;
  const applicationAudioSupported = applicationAudioSupport?.supported ?? false;
  const applicationAudioDetail = translateBackendMessage(
    locale,
    status?.application_audio?.message ?? t("Audio del Mac esperando inicio.")
  );
  const applicationAudioNeedsAttention = Boolean(
    running &&
    profile?.application_audio_enabled &&
    status?.application_audio?.configured &&
    !status.application_audio.ready
  );
  const applicationAudioPermissionMissing = /autoriz|permiso|tcc|capture/i.test(
    status?.application_audio?.message ?? ""
  );

  useEffect(() => {
    if (workspaceTab !== "control" || !playingQueueEntryId || !playingQueueEntryVisible) return;
    const frame = window.requestAnimationFrame(() => {
      const container = queueScrollElement.current;
      const entry = playingQueueEntryElement.current;
      if (!container || !entry) return;
      const containerRect = container.getBoundingClientRect();
      const entryRect = entry.getBoundingClientRect();
      const centeredTop = container.scrollTop
        + entryRect.top
        - containerRect.top
        - (container.clientHeight - entryRect.height) / 2;
      container.scrollTo({
        top: Math.max(0, centeredTop),
        behavior: lastAutoScrolledQueueEntryId.current ? "smooth" : "auto"
      });
      lastAutoScrolledQueueEntryId.current = playingQueueEntryId;
    });
    return () => window.cancelAnimationFrame(frame);
  }, [automation.mode, playingQueueEntryId, playingQueueEntryVisible, workspaceTab]);

  const hydrateProfile = useCallback((next: BroadcastProfile, hydrateControl = true) => {
    setProfile(next);
    setProfileName(next.name);
    setOutputKind(next.output_kind === "rtmp" ? "rtmp" : "icecast");
    setHost(next.host);
    setPort(String(next.port));
    setMount(next.mount);
    setUsername(next.username);
    setStationName(next.station_name);
    setDescription(next.description);
    setBitrate(String(next.bitrate_kbps));
    setTls(next.tls);
    setIsPublic(next.public);
    if (hydrateControl) {
      setMicrophoneEnabled(next.microphone_enabled);
      setMicrophoneDevice(next.microphone_device || "default");
      setMicrophoneGain(String(next.microphone_gain_percent));
      setLineInputEnabled(next.line_input_enabled);
      setLineInputDevice(next.line_input_device || "default");
      setLineInputChannel(String(next.line_input_channel || 1));
      setLineInputStereo(next.line_input_stereo);
      setLineInputGain(String(next.line_input_gain_percent));
      setApplicationAudioEnabled(next.application_audio_enabled);
      setApplicationAudioBundleId(next.application_audio_bundle_id || SYSTEM_AUDIO_TARGET_ID);
      setApplicationAudioGain(String(next.application_audio_gain_percent));
      setSourceTab(next.application_audio_enabled ? "system_audio" : next.line_input_enabled ? "line_input" : "microphone");
    }
    setRtmpPlatform(next.rtmp_platform === "custom" ? "custom" : "instagram");
    setRtmpServerUrl(next.rtmp_server_url);
    setRtmpVideoBitrate(String(next.rtmp_video_bitrate_kbps));
    setRtmpAudioBitrate(String(next.rtmp_audio_bitrate_kbps));
    setVideoCompositor(next.video_compositor ?? defaultVideoCompositor);
    setPassword("");
    setClearPassword(false);
  }, []);

  const refreshRuntime = useCallback(async () => {
    const [nextStatus, nextQueue, nextPreflight] = await Promise.all([
      invoke<BroadcastStatus>("broadcast_status"),
      invoke<BroadcastQueueEntry[]>("broadcast_queue"),
      invoke<BroadcastPreflight>("broadcast_preflight")
    ]);
    setStatus(nextStatus);
    setCameraMix(nextStatus.camera?.mix_percent ?? 0);
    setQueue(nextQueue);
    setPreflight(nextPreflight);
  }, []);

  useEffect(() => {
    let disposed = false;
    let unlisten: UnlistenFn | undefined;
    let unlistenCalled = false;
    const stopListeningOnce = (candidate = unlisten) => {
      if (!candidate || unlistenCalled) return;
      unlistenCalled = true;
      safelyUnlisten(candidate);
    };
    void Promise.all([
      invoke<BroadcastProfile>("broadcast_profile"),
      invoke<BroadcastProfile[]>("broadcast_profiles"),
      invoke<BroadcastStatus>("broadcast_status"),
      invoke<BroadcastQueueEntry[]>("broadcast_queue"),
      invoke<BroadcastScheduleItem[]>("broadcast_schedule_items"),
      invoke<BroadcastAutomationSettings>("broadcast_automation_settings"),
      invoke<BroadcastPreflight>("broadcast_preflight"),
      invoke<BroadcastApplicationAudioSupport>("broadcast_application_audio_support"),
      loadBroadcastPlaylistSources(),
      invoke<BroadcastMicrophoneDevice[]>("broadcast_microphone_devices"),
      invoke<BroadcastCameraDevice[]>("broadcast_camera_devices").catch(() => [])
    ])
      .then(([nextProfile, nextProfiles, nextStatus, nextQueue, nextScheduleItems, nextAutomation, nextPreflight, nextApplicationAudioSupport, nextPlaylistSources, nextMicrophones, nextCameras]) => {
        if (disposed) return;
        hydrateProfile(nextProfile);
        setProfiles(nextProfiles);
        setStatus(nextStatus);
        setQueue(nextQueue);
        setScheduleItems(nextScheduleItems);
        setAutomation(nextAutomation);
        setPreflight(nextPreflight);
        setApplicationAudioSupport(nextApplicationAudioSupport);
        setPlaylistSources(nextPlaylistSources);
        setMicrophoneDevices(nextMicrophones);
        setCameraDevices(nextCameras);
        setCameraMix(nextStatus.camera?.mix_percent ?? 0);
        if (!nextMicrophones.some((device) => device.id === nextProfile.microphone_device)) {
          setMicrophoneDevice(nextMicrophones[0]?.id ?? "default");
        }
        if (!nextMicrophones.some((device) => device.id === nextProfile.line_input_device)) {
          setLineInputDevice(nextMicrophones[0]?.id ?? "default");
        }
      })
      .catch((cause) => setError(errorMessage(cause, locale)))
      .finally(() => setBusy(null));

    void listen<BroadcastProgressEvent>("broadcast-progress", ({ payload }) => {
      setStatus(payload.status);
      setCameraMix(payload.status.camera?.mix_percent ?? 0);
      if (!payload.event.endsWith("_level")) {
        const level: TerminalLogEntry["level"] = payload.level === "error"
          ? "error"
          : payload.level === "warning"
            ? "warning"
            : "info";
        setTerminalLogs((current) => [...current, {
          id: nextTerminalLogId.current++,
          time: new Date(payload.timestamp).toLocaleTimeString(),
          level,
          name: payload.event,
          message: translateBackendMessage(locale, payload.message)
        }].slice(-1200));
        window.requestAnimationFrame(() => {
          if (terminalElement.current) {
            terminalElement.current.scrollTop = terminalElement.current.scrollHeight;
          }
        });
      }
      void invoke<BroadcastQueueEntry[]>("broadcast_queue").then(setQueue).catch(() => undefined);
    })
      .then((stopListening) => {
        if (disposed) stopListeningOnce(stopListening);
        else unlisten = stopListening;
      })
      .catch(() => undefined);

    const timer = window.setInterval(() => {
      void Promise.all([
        invoke<BroadcastStatus>("broadcast_status").then((nextStatus) => {
          setStatus(nextStatus);
          setCameraMix(nextStatus.camera?.mix_percent ?? 0);
        }),
        invoke<BroadcastQueueEntry[]>("broadcast_queue").then(setQueue),
        invoke<BroadcastScheduleItem[]>("broadcast_schedule_items").then(setScheduleItems)
      ]).catch(() => undefined);
    }, 2500);

    return () => {
      disposed = true;
      window.clearInterval(timer);
      stopListeningOnce();
    };
  }, [hydrateProfile, locale]);

  useEffect(() => {
    let disposed = false;
    let unlisten: UnlistenFn | undefined;
    void listen<BroadcastLineInputPreview>("broadcast-line-input-preview", ({ payload }) => {
      if (!disposed) setLineInputPreview(payload);
    }).then((stopListening) => {
      if (disposed) safelyUnlisten(stopListening);
      else unlisten = stopListening;
    }).catch(() => undefined);
    return () => {
      disposed = true;
      if (unlisten) safelyUnlisten(unlisten);
      void invoke("broadcast_stop_line_input_preview").catch(() => undefined);
    };
  }, []);

  useEffect(() => () => {
    if (compositorSaveTimer.current !== null) {
      window.clearTimeout(compositorSaveTimer.current);
    }
  }, []);

  const selectedPlaylistSource = useMemo(
    () => playlistSources.find((source) => source.key === playlistSourceKey) ?? null,
    [playlistSourceKey, playlistSources]
  );
  const localPlaylistSources = useMemo(
    () => playlistSources.filter((source) => source.kind === "local"),
    [playlistSources]
  );
  const rekordboxPlaylistSources = useMemo(
    () => playlistSources.filter((source) => source.kind === "rekordbox"),
    [playlistSources]
  );
  const selectedLineInputDevice = useMemo(
    () => microphoneDevices.find((device) => device.id === lineInputDevice) ?? null,
    [lineInputDevice, microphoneDevices]
  );
  const lineInputChannels = Math.max(1, selectedLineInputDevice?.input_channels ?? 1);

  function changeLineInputDevice(deviceId: string) {
    if (lineInputPreview?.active) void stopLineInputPreview();
    const nextDevice = microphoneDevices.find((device) => device.id === deviceId);
    const channels = Math.max(1, nextDevice?.input_channels ?? 1);
    setLineInputDevice(deviceId);
    if (Number(lineInputChannel) > channels || (lineInputStereo && Number(lineInputChannel) >= channels)) {
      setLineInputChannel("1");
    }
    if (channels < 2) setLineInputStereo(false);
  }

  async function saveProfile(event: FormEvent) {
    event.preventDefault();
    await persistProfile();
  }

  async function persistProfile(): Promise<boolean> {
    setBusy("saving");
    setError(null);
    setNotice(null);
    try {
      const saved = await invoke<BroadcastProfile>("broadcast_save_profile", {
        profile: {
          name: profileName,
          outputKind,
          host,
          port: Number(port),
          mount,
          username,
          stationName,
          description,
          bitrateKbps: Number(bitrate),
          tls,
          public: isPublic,
          rtmpPlatform,
          rtmpServerUrl,
          rtmpVideoBitrateKbps: Number(rtmpVideoBitrate),
          rtmpAudioBitrateKbps: Number(rtmpAudioBitrate),
          videoCompositor,
          password: password || null,
          clearPassword
        }
      });
      hydrateProfile(saved, false);
      const [nextPreflight, nextProfiles] = await Promise.all([
        invoke<BroadcastPreflight>("broadcast_preflight"),
        invoke<BroadcastProfile[]>("broadcast_profiles")
      ]);
      setPreflight(nextPreflight);
      setProfiles(nextProfiles);
      setNotice(t("Destino de salida guardado."));
      return true;
    } catch (cause) {
      setError(errorMessage(cause, locale));
      return false;
    } finally {
      setBusy(null);
    }
  }

  async function saveControlSettings() {
    await runAction("control-settings", async () => {
      const saved = await invoke<BroadcastControlSettings>("broadcast_save_control_settings", {
        settings: {
          microphoneEnabled,
          microphoneDevice,
          microphoneGainPercent: Number(microphoneGain),
          lineInputEnabled,
          lineInputDevice,
          lineInputChannel: Number(lineInputChannel),
          lineInputStereo,
          lineInputGainPercent: Number(lineInputGain),
          applicationAudioEnabled,
          applicationAudioBundleId,
          applicationAudioGainPercent: Number(applicationAudioGain)
        }
      });
      const controlPatch = {
        microphone_enabled: saved.microphone_enabled,
        microphone_device: saved.microphone_device,
        microphone_gain_percent: saved.microphone_gain_percent,
        line_input_enabled: saved.line_input_enabled,
        line_input_device: saved.line_input_device,
        line_input_channel: saved.line_input_channel,
        line_input_stereo: saved.line_input_stereo,
        line_input_gain_percent: saved.line_input_gain_percent,
        application_audio_enabled: saved.application_audio_enabled,
        application_audio_bundle_id: saved.application_audio_bundle_id,
        application_audio_gain_percent: saved.application_audio_gain_percent
      };
      setMicrophoneEnabled(saved.microphone_enabled);
      setMicrophoneDevice(saved.microphone_device || "default");
      setMicrophoneGain(String(saved.microphone_gain_percent));
      setLineInputEnabled(saved.line_input_enabled);
      setLineInputDevice(saved.line_input_device || "default");
      setLineInputChannel(String(saved.line_input_channel));
      setLineInputStereo(saved.line_input_stereo);
      setLineInputGain(String(saved.line_input_gain_percent));
      setApplicationAudioEnabled(saved.application_audio_enabled);
      setApplicationAudioBundleId(saved.application_audio_bundle_id || SYSTEM_AUDIO_TARGET_ID);
      setApplicationAudioGain(String(saved.application_audio_gain_percent));
      setProfile((current) => current ? { ...current, ...controlPatch } : current);
      setProfiles((current) => current.map((item) => ({ ...item, ...controlPatch })));
      setPreflight(await invoke<BroadcastPreflight>("broadcast_preflight"));
      setNotice(t("Fuentes de entrada guardadas."));
    });
  }

  async function createDestination(event: FormEvent) {
    event.preventDefault();
    const name = newDestinationName.trim();
    if (!name) return;
    await runAction("destination-create", async () => {
      const created = await invoke<BroadcastProfile>("broadcast_create_profile", { name });
      const [nextProfiles, nextPreflight] = await Promise.all([
        invoke<BroadcastProfile[]>("broadcast_profiles"),
        invoke<BroadcastPreflight>("broadcast_preflight")
      ]);
      hydrateProfile(created, false);
      setProfiles(nextProfiles);
      setPreflight(nextPreflight);
      setCreatingDestination(false);
      setNewDestinationName("");
      setNotice(t("Destino creado y activado. Ajusta sus datos y guárdalo."));
    });
  }

  async function activateDestination(profileId: string) {
    if (profile?.id === profileId) return;
    await runAction(`destination-activate:${profileId}`, async () => {
      const activated = await invoke<BroadcastProfile>("broadcast_activate_profile", { profileId });
      const [nextProfiles, nextPreflight] = await Promise.all([
        invoke<BroadcastProfile[]>("broadcast_profiles"),
        invoke<BroadcastPreflight>("broadcast_preflight")
      ]);
      hydrateProfile(activated, false);
      setProfiles(nextProfiles);
      setPreflight(nextPreflight);
      setNotice(t("Destino {name} activado.", { name: activated.name }));
    });
  }

  async function deleteDestination(destination: BroadcastProfile) {
    if (!window.confirm(t("¿Eliminar el destino {name}?", { name: destination.name }))) return;
    await runAction(`destination-delete:${destination.id}`, async () => {
      const active = await invoke<BroadcastProfile>("broadcast_delete_profile", { profileId: destination.id });
      const [nextProfiles, nextPreflight] = await Promise.all([
        invoke<BroadcastProfile[]>("broadcast_profiles"),
        invoke<BroadcastPreflight>("broadcast_preflight")
      ]);
      hydrateProfile(active, false);
      setProfiles(nextProfiles);
      setPreflight(nextPreflight);
      setNotice(t("Destino eliminado."));
    });
  }

  async function appendPlaylist() {
    if (!selectedPlaylistSource) return;
    setBusy("appending");
    setError(null);
    setNotice(null);
    try {
      const result = selectedPlaylistSource.kind === "local"
        ? await invoke<QueueAppendResult>("broadcast_append_draft", {
            draftId: selectedPlaylistSource.id
          })
        : await invoke<QueueAppendResult>("broadcast_append_playlist", {
            libraryId: selectedPlaylistSource.library_id,
            playlistPath: selectedPlaylistSource.id
          });
      setQueue(result.queue);
      setNotice(t("Se agregaron {count} pistas al broadcast. {skipped} omitidas.", {
        count: result.appended_total,
        skipped: result.skipped_missing_total
      }));
    } catch (cause) {
      setError(errorMessage(cause, locale));
    } finally {
      setBusy(null);
    }
  }

  async function changeBroadcastMode(mode: "immediate" | "scheduled") {
    if (automation.mode === mode) return;
    setBusy("automation-mode");
    setError(null);
    try {
      const saved = await invoke<BroadcastAutomationSettings>("broadcast_save_automation_settings", {
        settings: {
          mode,
          bedEnabled: automation.bed_enabled,
          bedLibraryId: automation.bed_library_id ?? null,
          bedTrackId: automation.bed_track_id ?? null,
          bedGainPercent: automation.bed_gain_percent,
          bedDucking: automation.bed_ducking
        }
      });
      setAutomation(saved);
      setNotice(mode === "scheduled" ? t("Modo programado activado.") : t("Modo inmediato activado."));
    } catch (cause) {
      setError(errorMessage(cause, locale));
    } finally {
      setBusy(null);
    }
  }

  async function startBroadcast() {
    await runAction("starting", async () => {
      setStatus(await invoke<BroadcastStatus>("broadcast_start", {
        streamKey: outputKind === "rtmp" ? streamKey : null
      }));
      setNotice(outputKind === "rtmp"
        ? t("Enviando señal RTMP. Revisa la vista previa antes de salir al aire.")
        : t("Iniciando transmisión a Icecast."));
    });
  }

  async function stopBroadcast() {
    await runAction("stopping", async () => {
      setStatus(await invoke<BroadcastStatus>("broadcast_stop"));
      if (outputKind === "rtmp") setStreamKey("");
    });
  }

  async function skipTrack() {
    await runAction("skipping", async () => {
      setStatus(await invoke<BroadcastStatus>("broadcast_skip"));
    });
  }

  async function playQueueEntry(entry: BroadcastQueueEntry) {
    await runAction(`play:${entry.id}`, async () => {
      setStatus(await invoke<BroadcastStatus>("broadcast_play_queue_entry", { entryId: entry.id }));
      setNotice(t("Cambiando a {track}...", { track: entryTitle(entry) }));
    });
  }

  async function toggleMicrophone() {
    const live = !(status?.microphone?.live ?? false);
    await runAction("microphone", async () => {
      await invoke<BroadcastStatus>("broadcast_set_microphone_live", { live });
      setStatus((current) => current ? {
        ...current,
        microphone: {
          ...(current.microphone ?? {
            configured: true,
            ready: true,
            receiving_audio: false,
            level_percent: 0,
            device: profile?.microphone_device ?? "default",
            gain_percent: profile?.microphone_gain_percent ?? 100,
            message: ""
          }),
          live,
          message: live ? t("Micrófono al aire.") : t("Micrófono silenciado.")
        }
      } : current);
    });
  }

  async function toggleLineInput() {
    const live = !(status?.line_input?.live ?? false);
    await runAction("line-input", async () => {
      await invoke<BroadcastStatus>("broadcast_set_line_input_live", { live });
      setStatus((current) => current ? {
        ...current,
        source_mode: live ? "line_input" : "playlist",
        now_playing: live ? null : current.now_playing,
        message: live ? t("Línea directa al aire.") : t("Radio en vivo · fuente Playlist."),
        line_input: {
          ...(current.line_input ?? {
            configured: true,
            ready: true,
            receiving_audio: false,
            level_percent: 0,
            device: profile?.line_input_device ?? "default",
            channel: profile?.line_input_channel ?? 1,
            stereo: profile?.line_input_stereo ?? true,
            gain_percent: profile?.line_input_gain_percent ?? 100,
            message: ""
          }),
          live,
          message: live ? t("Línea directa al aire.") : t("Fuente Playlist al aire.")
        }
      } : current);
    });
  }

  async function toggleApplicationAudio() {
    const live = !(status?.application_audio?.live ?? false);
    await runAction("application-audio", async () => {
      await invoke<BroadcastStatus>("broadcast_set_application_audio_live", { live });
      const label = applicationAudioBundleId === SYSTEM_AUDIO_TARGET_ID
        ? t("Salida completa del Mac")
        : applicationAudioDevices.find((application) => application.id === applicationAudioBundleId)?.label;
      const liveMessage = applicationAudioBundleId === SYSTEM_AUDIO_TARGET_ID
        ? t("Salida completa del Mac al aire.")
        : t("Audio de {application} al aire.", { application: label ?? t("aplicación") });
      setStatus((current) => current ? {
        ...current,
        source_mode: live ? "application_audio" : "playlist",
        now_playing: live ? null : current.now_playing,
        message: live
          ? liveMessage
          : t("Radio en vivo · fuente Playlist."),
        application_audio: {
          ...(current.application_audio ?? {
            configured: true,
            ready: true,
            receiving_audio: false,
            level_percent: 0,
            application: profile?.application_audio_bundle_id ?? "",
            label: label ?? null,
            gain_percent: profile?.application_audio_gain_percent ?? 100,
            message: ""
          }),
          live,
          message: live
            ? liveMessage
            : t("Fuente Playlist al aire.")
        }
      } : current);
    });
  }

  async function sendCameraMix(mixPercent: number, transitionMillis: number) {
    const normalized = Math.max(0, Math.min(100, Math.round(mixPercent)));
    setCameraMix(normalized);
    await runAction("camera-mix", async () => {
      const nextStatus = await invoke<BroadcastStatus>("broadcast_set_camera_mix", {
        mixPercent: normalized,
        transitionMillis
      });
      setStatus(nextStatus);
      setCameraMix(nextStatus.camera?.mix_percent ?? normalized);
    });
  }

  function changeVideoCompositor(next: BroadcastVideoCompositor) {
    setVideoCompositor(next);
    const revision = ++compositorSaveRevision.current;
    if (compositorSaveTimer.current !== null) {
      window.clearTimeout(compositorSaveTimer.current);
    }
    compositorSaveTimer.current = window.setTimeout(() => {
      compositorSaveTimer.current = null;
      const command = runningRef.current
        ? invoke<BroadcastStatus>("broadcast_update_camera_settings", { config: next })
        : invoke<void>("broadcast_save_video_compositor", { config: next });
      void command
        .then((nextStatus) => {
          if (revision !== compositorSaveRevision.current) return;
          if (nextStatus) setStatus(nextStatus);
          setProfile((current) => current ? { ...current, video_compositor: next } : current);
        })
        .catch((cause) => {
          if (revision === compositorSaveRevision.current) {
            setError(errorMessage(cause, locale));
          }
        });
    }, 100);
  }

  async function refreshMicrophones() {
    await runAction("microphones", async () => {
      const devices = await invoke<BroadcastMicrophoneDevice[]>("broadcast_microphone_devices");
      setMicrophoneDevices(devices);
      if (!devices.some((device) => device.id === microphoneDevice)) {
        setMicrophoneDevice("default");
      }
      if (!devices.some((device) => device.id === lineInputDevice)) {
        setLineInputDevice("default");
      }
    });
  }

  async function stopLineInputPreview() {
    setLineInputPreview((current) => current ? {
      ...current,
      active: false,
      receiving_audio: false,
      level_percent: 0,
      message: t("Previsualización de línea detenida.")
    } : current);
    await invoke("broadcast_stop_line_input_preview");
  }

  async function toggleLineInputPreview() {
    await runAction("line-input-preview", async () => {
      if (lineInputPreview?.active) {
        await stopLineInputPreview();
        return;
      }
      await invoke("broadcast_start_line_input_preview", {
        device: lineInputDevice,
        channel: Number(lineInputChannel),
        stereo: lineInputStereo,
        gainPercent: Number(lineInputGain)
      });
    });
  }

  async function refreshApplications() {
    await runAction("applications", async () => {
      const applications = await invoke<BroadcastApplicationAudioDevice[]>("broadcast_application_audio_devices");
      setApplicationAudioDevices(applications);
      if (!applicationAudioBundleId) {
        setApplicationAudioBundleId(SYSTEM_AUDIO_TARGET_ID);
      }
    });
  }

  async function openApplicationAudioSettings() {
    await runAction("application-settings", async () => {
      await invoke("broadcast_open_application_audio_settings");
      setNotice(t("Activa Rau Studio, cierra completamente la app y vuelve a abrirla."));
    });
  }

  async function clearQueue() {
    await runAction("clearing", async () => {
      const deleted = await invoke<number>("broadcast_clear_queue");
      await refreshRuntime();
      setNotice(t("Se quitaron {count} entradas de la cola.", { count: deleted }));
    });
  }

  async function removeEntry(entryId: string) {
    await runAction(`remove:${entryId}`, async () => {
      await invoke("broadcast_remove_queue_entry", { entryId });
      setQueue(await invoke<BroadcastQueueEntry[]>("broadcast_queue"));
    });
  }

  async function persistQueuedOrder(entryIds: string[]) {
    await runAction("reordering", async () => {
      setQueue(await invoke<BroadcastQueueEntry[]>("broadcast_reorder_queue", { entryIds }));
    });
  }

  async function moveQueuedEntry(entryId: string, direction: -1 | 1) {
    const entryIds = queue.filter((entry) => entry.status === "queued").map((entry) => entry.id);
    const currentIndex = entryIds.indexOf(entryId);
    const nextIndex = currentIndex + direction;
    if (currentIndex < 0 || nextIndex < 0 || nextIndex >= entryIds.length) return;
    [entryIds[currentIndex], entryIds[nextIndex]] = [entryIds[nextIndex], entryIds[currentIndex]];
    await persistQueuedOrder(entryIds);
  }

  async function moveQueuedEntryToTarget(entryId: string, targetId: string) {
    if (entryId === targetId) return;
    const entryIds = queue.filter((entry) => entry.status === "queued").map((entry) => entry.id);
    const currentIndex = entryIds.indexOf(entryId);
    const targetIndex = entryIds.indexOf(targetId);
    if (currentIndex < 0 || targetIndex < 0) return;
    entryIds.splice(currentIndex, 1);
    const adjustedTargetIndex = entryIds.indexOf(targetId);
    entryIds.splice(currentIndex < targetIndex ? adjustedTargetIndex + 1 : adjustedTargetIndex, 0, entryId);
    await persistQueuedOrder(entryIds);
  }

  async function sortQueuedEntries(sort: "title" | "artist" | "duration") {
    const collator = new Intl.Collator(locale, { numeric: true, sensitivity: "base" });
    const sorted = queue.filter((entry) => entry.status === "queued").sort((left, right) => {
      if (sort === "duration") {
        return (left.duration_seconds ?? Number.MAX_SAFE_INTEGER) - (right.duration_seconds ?? Number.MAX_SAFE_INTEGER)
          || collator.compare(entryTitle(left), entryTitle(right));
      }
      const leftValue = sort === "artist" ? left.artist ?? left.title : left.title;
      const rightValue = sort === "artist" ? right.artist ?? right.title : right.title;
      return collator.compare(leftValue, rightValue) || collator.compare(entryTitle(left), entryTitle(right));
    });
    await persistQueuedOrder(sorted.map((entry) => entry.id));
  }

  async function runAction(action: BusyAction, callback: () => Promise<void>) {
    setBusy(action);
    setError(null);
    setNotice(null);
    try {
      await callback();
    } catch (cause) {
      setError(errorMessage(cause, locale));
    } finally {
      setBusy(null);
    }
  }

  function clearTerminal() {
    setTerminalLogs([]);
  }

  function renderControlSettingsEditor() {
    return (
                <div className="overflow-hidden rounded-lg border border-border bg-muted/15">
                  <div className="grid grid-cols-3 gap-1 border-b border-border bg-secondary/70 p-1" role="tablist" aria-label={t("Fuentes de audio")}>
                    <SourceTabButton
                      id="broadcast-source-microphone-tab"
                      controls="broadcast-source-microphone-panel"
                      active={sourceTab === "microphone"}
                      enabled={microphoneEnabled}
                      icon={<Mic className="h-3.5 w-3.5" />}
                      label={t("Micrófono")}
                      onClick={() => setSourceTab("microphone")}
                    />
                    <SourceTabButton
                      id="broadcast-source-line-tab"
                      controls="broadcast-source-line-panel"
                      active={sourceTab === "line_input"}
                      enabled={lineInputEnabled}
                      icon={<Radio className="h-3.5 w-3.5" />}
                      label={t("Línea")}
                      onClick={() => setSourceTab("line_input")}
                    />
                    <SourceTabButton
                      id="broadcast-source-system-tab"
                      controls="broadcast-source-system-panel"
                      active={sourceTab === "system_audio"}
                      enabled={applicationAudioEnabled && applicationAudioSupported}
                      icon={<AudioLines className="h-3.5 w-3.5" />}
                      label={t("Sistema")}
                      onClick={() => setSourceTab("system_audio")}
                    />
                  </div>
                  <div className="min-h-[250px]">
                <div
                  id="broadcast-source-microphone-panel"
                  role="tabpanel"
                  aria-labelledby="broadcast-source-microphone-tab"
                  hidden={sourceTab !== "microphone"}
                  className={cn("grid gap-3 p-3", sourceTab !== "microphone" && "hidden")}
                >
                  <div className="flex items-center justify-between gap-3">
                    <div className="flex items-center gap-2">
                      <Mic className="h-4 w-4" />
                      <strong className="text-sm">{t("Entrada de micrófono")}</strong>
                    </div>
                    <Button type="button" size="sm" variant="ghost" disabled={running || busy === "microphones"} onClick={() => void refreshMicrophones()}>
                      <RefreshCcw className={cn("h-4 w-4", busy === "microphones" && "animate-spin")} />
                      {t("Refrescar")}
                    </Button>
                  </div>
                  <label className="flex items-center gap-2 text-sm">
                    <input
                      type="checkbox"
                      checked={microphoneEnabled}
                      disabled={running || !preflight?.microphone_input_available}
                      onChange={(event) => setMicrophoneEnabled(event.target.checked)}
                    />
                    {t("Preparar micrófono al iniciar")}
                  </label>
                  {microphoneEnabled ? (
                    <>
                      <Field label={t("Dispositivo de entrada")}>
                        <select className={fieldClass} value={microphoneDevice} disabled={running} onChange={(event) => setMicrophoneDevice(event.target.value)}>
                          {microphoneDevices.map((device) => <option key={device.id} value={device.id}>{device.is_default ? t(device.label) : device.label}</option>)}
                        </select>
                      </Field>
                      <Field label={t("Ganancia del micrófono: {gain}%", { gain: microphoneGain })}>
                        <input
                          className="w-full accent-foreground"
                          type="range"
                          min={0}
                          max={200}
                          step={5}
                          value={microphoneGain}
                          disabled={running}
                          onChange={(event) => setMicrophoneGain(event.target.value)}
                        />
                      </Field>
                      <p className="text-xs text-muted-foreground">
                        {t("Se prepara silenciado. Actívalo desde Control de transmisión cuando quieras hablar.")}
                        {" "}{t("Cuando detecta tu voz, la música baja automáticamente y vuelve a subir al terminar.")}
                      </p>
                    </>
                  ) : (
                    <p className="text-xs text-muted-foreground">
                      {preflight?.microphone_input_available
                        ? t("Activa esta opción para seleccionar un micrófono.")
                        : t("No hay un dispositivo de entrada de audio disponible.")}
                    </p>
                  )}
                </div>
                <div
                  id="broadcast-source-line-panel"
                  role="tabpanel"
                  aria-labelledby="broadcast-source-line-tab"
                  hidden={sourceTab !== "line_input"}
                  className={cn("grid gap-3 p-3", sourceTab !== "line_input" && "hidden")}
                >
                  <div className="flex items-center justify-between gap-3">
                    <div className="flex items-center gap-2">
                      <Radio className="h-4 w-4" />
                      <strong className="text-sm">{t("Entrada de línea directa")}</strong>
                    </div>
                    <Button type="button" size="sm" variant="ghost" disabled={running || busy === "microphones"} onClick={() => void refreshMicrophones()}>
                      <RefreshCcw className={cn("h-4 w-4", busy === "microphones" && "animate-spin")} />
                      {t("Refrescar")}
                    </Button>
                  </div>
                  <label className="flex items-center gap-2 text-sm">
                    <input
                      type="checkbox"
                      checked={lineInputEnabled}
                      disabled={running || !preflight?.microphone_input_available}
                      onChange={(event) => {
                        if (!event.target.checked && lineInputPreview?.active) void stopLineInputPreview();
                        setLineInputEnabled(event.target.checked);
                      }}
                    />
                    {t("Preparar línea directa al iniciar")}
                  </label>
                  {lineInputEnabled ? (
                    <>
                      <Field label={t("Dispositivo de línea")}>
                        <select className={fieldClass} value={lineInputDevice} disabled={running} onChange={(event) => changeLineInputDevice(event.target.value)}>
                          {microphoneDevices.map((device) => <option key={device.id} value={device.id}>{device.is_default ? t(device.label) : device.label} · {device.input_channels} ch</option>)}
                        </select>
                      </Field>
                      <Field label={t("Canal de entrada")}>
                        <select
                          className={fieldClass}
                          value={`${lineInputStereo ? "stereo" : "mono"}:${lineInputChannel}`}
                          disabled={running}
                          onChange={(event) => {
                            if (lineInputPreview?.active) void stopLineInputPreview();
                            const [mode, channel] = event.target.value.split(":");
                            setLineInputStereo(mode === "stereo");
                            setLineInputChannel(channel);
                          }}
                        >
                          <optgroup label={t("Mono")}>
                            {Array.from({ length: lineInputChannels }, (_, index) => index + 1).map((channel) => (
                              <option key={`mono:${channel}`} value={`mono:${channel}`}>{t("Canal {channel} mono", { channel })}</option>
                            ))}
                          </optgroup>
                          {lineInputChannels > 1 ? (
                            <optgroup label={t("Estéreo")}>
                              {Array.from({ length: lineInputChannels - 1 }, (_, index) => index + 1).map((channel) => (
                                <option key={`stereo:${channel}`} value={`stereo:${channel}`}>{t("Canales {left}–{right} estéreo", { left: channel, right: channel + 1 })}</option>
                              ))}
                            </optgroup>
                          ) : null}
                        </select>
                      </Field>
                      <Field label={t("Ganancia de línea: {gain}%", { gain: lineInputGain })}>
                        <input
                          className="w-full accent-foreground"
                          type="range"
                          min={0}
                          max={200}
                          step={5}
                          value={lineInputGain}
                          disabled={running}
                          onChange={(event) => {
                            if (lineInputPreview?.active) void stopLineInputPreview();
                            setLineInputGain(event.target.value);
                          }}
                        />
                      </Field>
                      <div className="grid gap-2 rounded-md border border-border bg-background/60 p-3">
                        <div className="flex flex-wrap items-center justify-between gap-2">
                          <div>
                            <strong className="block text-xs">{t("Preview del canal")}</strong>
                            <span className="text-[11px] text-muted-foreground">
                              {lineInputPreview?.active
                                ? translateBackendMessage(locale, lineInputPreview.message)
                                : t("Comprueba la señal seleccionada sin enviarla al broadcast.")}
                            </span>
                          </div>
                          <Button
                            type="button"
                            size="sm"
                            variant={lineInputPreview?.active ? "destructive" : "secondary"}
                            disabled={running || busy === "line-input-preview"}
                            onClick={() => void toggleLineInputPreview()}
                          >
                            {busy === "line-input-preview"
                              ? <LoaderCircle className="h-4 w-4 animate-spin" />
                              : lineInputPreview?.active ? <Square className="h-4 w-4" /> : <Play className="h-4 w-4" />}
                            {lineInputPreview?.active ? t("Detener preview") : t("Previsualizar canal")}
                          </Button>
                        </div>
                        <div className="h-2 overflow-hidden rounded-full bg-secondary">
                          <div
                            className={cn(
                              "h-full rounded-full transition-[width] duration-100",
                              (lineInputPreview?.level_percent ?? 0) > 80 ? "bg-red-500" : "bg-cyan-500"
                            )}
                            style={{ width: `${lineInputPreview?.level_percent ?? 0}%` }}
                          />
                        </div>
                        <div className="flex justify-between text-[10px] text-muted-foreground">
                          <span>{lineInputPreview?.receiving_audio ? t("Señal detectada") : t("Sin señal")}</span>
                          <span>{lineInputPreview?.level_percent ?? 0}%</span>
                        </div>
                      </div>
                      <p className="text-xs text-muted-foreground">
                        {t("La línea reemplaza temporalmente la playlist y pasa directo al destino, sin ducking.")}
                      </p>
                    </>
                  ) : (
                    <p className="text-xs text-muted-foreground">
                      {preflight?.microphone_input_available
                        ? t("Activa esta opción para preparar una interfaz o entrada de línea.")
                        : t("No hay un dispositivo de entrada de audio disponible.")}
                    </p>
                  )}
                </div>
                <div
                  id="broadcast-source-system-panel"
                  role="tabpanel"
                  aria-labelledby="broadcast-source-system-tab"
                  hidden={sourceTab !== "system_audio"}
                  className={cn("grid gap-3 p-3", sourceTab !== "system_audio" && "hidden")}
                >
                  <div className="flex items-center justify-between gap-3">
                    <div className="flex items-center gap-2">
                      <AudioLines className="h-4 w-4" />
                      <strong className="text-sm">{t("Salida del Mac")}</strong>
                    </div>
                    <div className="flex flex-wrap justify-end gap-1">
                      {applicationAudioDevices.length === 0 ? (
                        <Button type="button" size="sm" variant="ghost" disabled={!applicationAudioSupported || running || busy === "application-settings"} onClick={() => void openApplicationAudioSettings()}>
                          {t("Abrir ajustes")}
                        </Button>
                      ) : null}
                      <Button type="button" size="sm" variant="ghost" disabled={!applicationAudioSupported || running || busy === "applications"} onClick={() => void refreshApplications()}>
                        <RefreshCcw className={cn("h-4 w-4", busy === "applications" && "animate-spin")} />
                        {applicationAudioDevices.length === 0 ? t("Solicitar acceso") : t("Refrescar")}
                      </Button>
                    </div>
                  </div>
                  <label className="flex items-center gap-2 text-sm">
                    <input
                      type="checkbox"
                      checked={applicationAudioEnabled}
                      disabled={running || (!applicationAudioSupported && !applicationAudioEnabled)}
                      onChange={(event) => {
                        const enabled = event.target.checked;
                        if (enabled && !applicationAudioSupported) return;
                        setApplicationAudioEnabled(enabled);
                        if (enabled && !applicationAudioBundleId) {
                          setApplicationAudioBundleId(SYSTEM_AUDIO_TARGET_ID);
                        }
                      }}
                    />
                    {t("Preparar salida del Mac al iniciar")}
                  </label>
                  {!applicationAudioSupported ? (
                    <div className="flex gap-2 rounded-md border border-amber-500/40 bg-amber-500/10 p-3 text-xs text-amber-800 dark:text-amber-200">
                      <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
                      <span>{translateBackendMessage(locale, applicationAudioSupport?.message ?? "")}</span>
                    </div>
                  ) : applicationAudioEnabled ? (
                    <>
                      <Field label={t("Fuente de audio") }>
                        <select className={fieldClass} value={applicationAudioBundleId} disabled={running} onChange={(event) => setApplicationAudioBundleId(event.target.value)}>
                          <option value={SYSTEM_AUDIO_TARGET_ID}>{t("Toda la salida del Mac")}</option>
                          {applicationAudioBundleId && applicationAudioBundleId !== SYSTEM_AUDIO_TARGET_ID && !applicationAudioDevices.some((application) => application.id === applicationAudioBundleId) ? (
                            <option value={applicationAudioBundleId}>{applicationAudioBundleId} · {t("no está abierta")}</option>
                          ) : null}
                          {applicationAudioDevices.length > 0 ? (
                            <optgroup label={t("Aplicación específica (opcional)")}>
                              {applicationAudioDevices.map((application) => (
                                <option key={`${application.id}:${application.process_id}`} value={application.id}>{application.label}</option>
                              ))}
                            </optgroup>
                          ) : null}
                        </select>
                      </Field>
                      <Field label={t("Ganancia de salida: {gain}%", { gain: applicationAudioGain })}>
                        <input
                          className="w-full accent-foreground"
                          type="range"
                          min={0}
                          max={200}
                          step={5}
                          value={applicationAudioGain}
                          disabled={running}
                          onChange={(event) => setApplicationAudioGain(event.target.value)}
                        />
                      </Field>
                      <p className="text-xs text-muted-foreground">
                        {t("Reemplaza temporalmente la playlist por todo lo que suena en el Mac, sin micrófono ni ducking. Rau Studio excluye su propio audio para evitar realimentación.")}
                        {" "}{t("macOS pedirá permiso de Grabación de pantalla y audio del sistema.")}
                      </p>
                    </>
                  ) : (
                    <p className="text-xs text-muted-foreground">
                      {t("Activa esta opción para enviar toda la salida normal del computador al broadcast. También puedes limitarla a una aplicación.")}
                    </p>
                  )}
                </div>
                  </div>
                </div>
    );
  }

  if (busy === "loading" && !profile) {
    return (
      <main className="grid min-h-screen place-items-center p-6">
        <LoaderCircle className="h-7 w-7 animate-spin text-muted-foreground" aria-label={t("Cargando")} />
      </main>
    );
  }

  return (
    <main
      className={cn(
        "overflow-y-auto bg-background p-4 text-foreground lg:p-6",
        terminalExpanded ? "h-[calc(100dvh-17.25rem)]" : "h-[calc(100dvh-4.75rem)]"
      )}
    >
      <div className="mx-auto grid w-full max-w-[1480px] gap-4">
        <header className="flex flex-wrap items-start justify-between gap-4">
          <div>
            <div className="flex items-center gap-2 text-muted-foreground">
              <Radio className="h-4 w-4" />
              <span className="text-xs font-semibold uppercase tracking-[0.18em]">{t("Broadcast")}</span>
            </div>
            <h1 className="mt-1 text-2xl font-semibold tracking-tight">{t("Broadcast desde casa")}</h1>
            <p className="mt-1 max-w-3xl text-sm text-muted-foreground">
              {t("Rau Studio mezcla tu cola y entradas locales para transmitir por Icecast o RTMP.")}
            </p>
          </div>
          <StatusBadge status={status?.status ?? "idle"} label={status?.message ?? t("Radio detenida.")} />
        </header>

        {error ? (
          <div role="alert" className="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
            {error}
          </div>
        ) : null}
        {notice ? (
          <div className="rounded-md border border-emerald-500/25 bg-emerald-500/10 px-3 py-2 text-sm text-emerald-800 dark:text-emerald-200">
            {notice}
          </div>
        ) : null}

        <section className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
          <Metric label={t("Estado")} value={statusLabel(status?.status ?? "idle", t)} icon={<Wifi className="h-4 w-4" />} />
          <Metric label={t("En cola")} value={String(queuedTotal)} icon={<Library className="h-4 w-4" />} />
          <Metric label={t("Reproducidas")} value={String(completedTotal)} icon={<Play className="h-4 w-4" />} />
          <Metric label={t("Fallidas")} value={String(failedTotal)} icon={<RefreshCcw className="h-4 w-4" />} danger={failedTotal > 0} />
        </section>

        <nav className="grid grid-cols-3 gap-1 rounded-lg border border-border bg-secondary/60 p-1" role="tablist" aria-label={t("Secciones de broadcast")}>
          {([
            { id: "destinations", label: t("Destinos de salida"), icon: <Wifi className="h-4 w-4" /> },
            { id: "control", label: t("Control"), icon: <SlidersHorizontal className="h-4 w-4" /> },
            { id: "schedule", label: t("Parrilla"), icon: <Calendar className="h-4 w-4" /> }
          ] as const).map((tab) => (
            <button
              key={tab.id}
              type="button"
              role="tab"
              aria-selected={workspaceTab === tab.id}
              className={cn(
                "flex min-w-0 items-center justify-center gap-2 rounded-md px-3 py-2 text-sm font-semibold transition-colors",
                workspaceTab === tab.id
                  ? "bg-background text-foreground shadow-sm ring-1 ring-border"
                  : "text-muted-foreground hover:bg-background/60 hover:text-foreground"
              )}
              onClick={() => setWorkspaceTab(tab.id)}
            >
              {tab.icon}
              <span className="truncate">{tab.label}</span>
              {tab.id === "destinations" ? (
                <span className="rounded-full bg-secondary px-1.5 py-0.5 text-[10px] tabular-nums">{profiles.length}</span>
              ) : tab.id === "schedule" && automation.mode === "scheduled" ? (
                <span className="h-2 w-2 rounded-full bg-emerald-500" aria-label={t("Parrilla activa")} />
              ) : null}
            </button>
          ))}
        </nav>

        <section className={cn(
          "grid gap-4",
          workspaceTab === "destinations" && "xl:grid-cols-[minmax(280px,0.55fr)_minmax(600px,1.45fr)]"
        )}>
          {workspaceTab === "destinations" ? (
            <DestinationProfilesCard
              profiles={profiles}
              activeId={profile?.id ?? null}
              running={running}
              busy={busy}
              creating={creatingDestination}
              newName={newDestinationName}
              onCreatingChange={setCreatingDestination}
              onNewNameChange={setNewDestinationName}
              onCreate={createDestination}
              onActivate={activateDestination}
              onDelete={deleteDestination}
              t={t}
            />
          ) : null}
          {workspaceTab === "destinations" ? <Card>
            <CardHeader>
              <div className="min-w-0">
                <CardTitle>{t("Configurar destino")}</CardTitle>
                <p className="mt-1 truncate text-xs text-muted-foreground">{profile?.name ?? t("Destino de salida")}</p>
              </div>
              <span className={cn(
                "rounded-full px-2 py-1 text-[11px] font-semibold",
                preflight?.ready
                  ? "bg-emerald-500/10 text-emerald-800 dark:text-emerald-200"
                  : "bg-amber-500/10 text-amber-800 dark:text-amber-200"
              )}>
                {preflight?.ready ? t("FFmpeg listo") : t("Revisar FFmpeg")}
              </span>
            </CardHeader>
            <CardContent className="p-3">
              <form className="grid gap-3" onSubmit={saveProfile}>
                <Field label={t("Nombre del destino") }>
                  <input
                    className={fieldClass}
                    value={profileName}
                    required
                    maxLength={80}
                    disabled={running}
                    placeholder={t("Ej. Radio principal")}
                    onChange={(event) => setProfileName(event.target.value)}
                  />
                </Field>
                <Field label={t("Tipo de destino")}>
                  <select className={fieldClass} value={outputKind} disabled={running} onChange={(event) => setOutputKind(event.target.value as BroadcastOutputKind)}>
                    <option value="icecast">Icecast · MP3</option>
                    <option value="rtmp">RTMP / RTMPS · {t("Video en vivo")}</option>
                  </select>
                </Field>
                {outputKind === "icecast" ? (
                  <>
                    <div className="grid gap-3 sm:grid-cols-[1fr_110px]">
                      <Field label={t("Host")}>
                        <input className={fieldClass} value={host} required disabled={running} onChange={(event) => setHost(event.target.value)} />
                      </Field>
                      <Field label={t("Puerto")}>
                        <input className={fieldClass} type="number" min={1} max={65535} value={port} required disabled={running} onChange={(event) => setPort(event.target.value)} />
                      </Field>
                    </div>
                    <Field label={t("Mountpoint MP3")}>
                      <input className={fieldClass} value={mount} required disabled={running} placeholder="/live.mp3" onChange={(event) => setMount(event.target.value)} />
                    </Field>
                    <div className="grid gap-3 sm:grid-cols-2">
                      <Field label={t("Usuario source")}>
                        <input className={fieldClass} value={username} required disabled={running} onChange={(event) => setUsername(event.target.value)} />
                      </Field>
                      <Field label={t("Bitrate MP3")}>
                        <select className={fieldClass} value={bitrate} disabled={running} onChange={(event) => setBitrate(event.target.value)}>
                          {[96, 128, 160, 192, 256, 320].map((value) => <option key={value} value={value}>{value} kbps</option>)}
                        </select>
                      </Field>
                    </div>
                    <Field label={profile?.password_configured ? t("Nueva contraseña source (opcional)") : t("Contraseña source")}>
                      <input
                        className={fieldClass}
                        type="password"
                        value={password}
                        required={!profile?.password_configured && !clearPassword}
                        disabled={running || clearPassword}
                        autoComplete="new-password"
                        onChange={(event) => setPassword(event.target.value)}
                      />
                    </Field>
                    <div className="grid gap-2 text-sm sm:grid-cols-2">
                      <label className="flex items-center gap-2 rounded-md border border-border px-3 py-2">
                        <input type="checkbox" checked={tls} disabled={running} onChange={(event) => setTls(event.target.checked)} />
                        {t("Usar TLS")}
                      </label>
                      <label className="flex items-center gap-2 rounded-md border border-border px-3 py-2">
                        <input type="checkbox" checked={isPublic} disabled={running} onChange={(event) => setIsPublic(event.target.checked)} />
                        {t("Listar públicamente")}
                      </label>
                      {profile?.password_configured ? (
                        <label className="flex items-center gap-2 rounded-md border border-border px-3 py-2 sm:col-span-2">
                          <input type="checkbox" checked={clearPassword} disabled={running} onChange={(event) => setClearPassword(event.target.checked)} />
                          {t("Eliminar contraseña guardada")}
                        </label>
                      ) : null}
                    </div>
                  </>
                ) : (
                  <>
                    <Field label={t("Plataforma")}>
                      <select className={fieldClass} value={rtmpPlatform} disabled={running} onChange={(event) => setRtmpPlatform(event.target.value as RtmpPlatform)}>
                        <option value="instagram">Instagram Live</option>
                        <option value="custom">{t("RTMP personalizado")}</option>
                      </select>
                    </Field>
                    <Field label={t("URL del servidor RTMP")}>
                      <input
                        className={fieldClass}
                        type="url"
                        value={rtmpServerUrl}
                        required
                        disabled={running}
                        placeholder="rtmps://live-upload.instagram.com:443/rtmp/"
                        onChange={(event) => setRtmpServerUrl(event.target.value)}
                      />
                    </Field>
                    <Field label={t("Clave de transmisión · solo esta sesión")}>
                      <input
                        className={fieldClass}
                        type="password"
                        value={streamKey}
                        disabled={running}
                        autoComplete="off"
                        placeholder={t("Pégala antes de enviar la señal")}
                        onChange={(event) => setStreamKey(event.target.value)}
                      />
                    </Field>
                    <div className="grid gap-3 sm:grid-cols-2">
                      <Field label={t("Bitrate de video")}>
                        <select className={fieldClass} value={rtmpVideoBitrate} disabled={running} onChange={(event) => setRtmpVideoBitrate(event.target.value)}>
                          {[2250, 3000, 3500, 4500, 6000].map((value) => <option key={value} value={value}>{value} kbps</option>)}
                        </select>
                      </Field>
                      <Field label={t("Bitrate AAC")}>
                        <select className={fieldClass} value={rtmpAudioBitrate} disabled={running} onChange={(event) => setRtmpAudioBitrate(event.target.value)}>
                          {[96, 128, 160, 192, 256].map((value) => <option key={value} value={value}>{value} kbps</option>)}
                        </select>
                      </Field>
                    </div>
                    <div className="rounded-md border border-violet-500/25 bg-violet-500/5 px-3 py-2 text-xs text-muted-foreground">
                      <div className="flex items-start justify-between gap-3">
                        <div>
                          <strong className="block text-foreground">720 × 1280 · 30 fps · H.264/AAC</strong>
                          <span>{t("Rau genera una señal visual monocroma con identidad de la radio y la pista actual, actualizada sin cortar el Live.")}</span>
                        </div>
                        <Button type="button" size="sm" variant="secondary" onClick={() => setVideoStudioOpen(true)}>
                          <SlidersHorizontal className="h-4 w-4" />
                          {t("Video Studio")}
                        </Button>
                      </div>
                    </div>
                    {rtmpPlatform === "instagram" ? (
                      <div className="rounded-md border border-amber-500/25 bg-amber-500/5 px-3 py-2 text-xs text-amber-900 dark:text-amber-100">
                        {t("Crea el Live en Instagram.com, copia su URL y clave, envía la señal desde Rau y confirma la vista previa en Live Producer. Para terminar, finaliza primero en Instagram.")}
                      </div>
                    ) : null}
                  </>
                )}
                <Field label={t("Nombre de estación")}>
                  <input className={fieldClass} value={stationName} required maxLength={120} disabled={running} onChange={(event) => setStationName(event.target.value)} />
                </Field>
                <Field label={t("Descripción")}>
                  <input className={fieldClass} value={description} maxLength={240} disabled={running} onChange={(event) => setDescription(event.target.value)} />
                </Field>
                <div className="rounded-md bg-secondary/60 px-3 py-2 text-xs text-muted-foreground">
                  <strong className="block break-all text-foreground">
                    {outputKind === "rtmp" ? (rtmpServerUrl || t("Configura la URL RTMP")) : (profile?.listener_url ?? "—")}
                  </strong>
                  <span>{translateBackendMessage(locale, preflight?.message ?? t("Revisando motor FFmpeg..."))}</span>
                  {destinationNeedsSave ? (
                    <span className="mt-1 block font-medium text-amber-700 dark:text-amber-300">{t("Guarda los cambios del destino antes de iniciar.")}</span>
                  ) : null}
                </div>
                <Button type="submit" disabled={busy === "saving" || running}>
                  {busy === "saving" ? <LoaderCircle className="h-4 w-4 animate-spin" /> : <Save className="h-4 w-4" />}
                  {t("Guardar destino")}
                </Button>
              </form>
            </CardContent>
          </Card> : null}

          {workspaceTab !== "destinations" ? <div className={cn(
            "grid min-h-0 gap-4",
            workspaceTab === "control" && "xl:grid-cols-[minmax(340px,0.72fr)_minmax(520px,1.28fr)]"
          )}>
            {workspaceTab === "control" ? <div className="grid content-start gap-4 self-start">
            <Card>
              <CardHeader className="flex-col items-stretch gap-2 py-3">
                <div className="min-w-0">
                  <CardTitle>{t("Control de transmisión")}</CardTitle>
                  <p className="mt-1 text-xs text-muted-foreground">{t("Acciones rápidas de la señal y sus fuentes.")}</p>
                </div>
                <div className="flex w-fit max-w-full flex-nowrap items-center gap-1 rounded-md border border-border bg-secondary/40 p-1">
                  {!running ? (
                    <BroadcastControlAction
                      label={t("Salir al aire")}
                      description={t("Inicia la transmisión usando el destino de salida seleccionado.")}
                      disabled={destinationNeedsSave || controlNeedsSave || !preflight?.ready || (outputKind === "rtmp" && !streamKey.trim()) || ((microphoneEnabled || lineInputEnabled) && !preflight?.microphone_input_available) || busy !== null}
                      icon={busy === "starting" ? <LoaderCircle className="h-4 w-4 animate-spin" /> : <Play className="h-4 w-4" />}
                      onClick={() => void startBroadcast()}
                    />
                  ) : (
                    <BroadcastControlAction
                      label={t("Detener")}
                      description={t("Finaliza la transmisión y cierra la conexión con el destino actual.")}
                      variant="destructive"
                      disabled={busy === "stopping" || status?.status === "stopping"}
                      icon={busy === "stopping" ? <LoaderCircle className="h-4 w-4 animate-spin" /> : <Square className="h-4 w-4" />}
                      onClick={() => void stopBroadcast()}
                    />
                  )}
                  <BroadcastControlAction
                    label={t("Saltar")}
                    description={t("Finaliza la pista actual y reproduce inmediatamente la siguiente.")}
                    disabled={!status?.now_playing || busy === "skipping"}
                    icon={busy === "skipping" ? <LoaderCircle className="h-4 w-4 animate-spin" /> : <SkipForward className="h-4 w-4" />}
                    onClick={() => void skipTrack()}
                  />
                  {(outputKind === "rtmp" || (running && (profile?.microphone_enabled || profile?.line_input_enabled || profile?.application_audio_enabled))) ? (
                    <span className="mx-0.5 h-6 w-px bg-border" aria-hidden="true" />
                  ) : null}
                  {outputKind === "rtmp" ? (
                    <BroadcastControlAction
                      label={t(status?.camera?.live ? "Fuentes en Program" : "Video Studio")}
                      description={t("Abre Preview / Program para organizar las fuentes visuales del RTMP.")}
                      variant={status?.camera?.live ? "default" : "secondary"}
                      icon={videoCompositor.screenEnabled
                        ? <Monitor className={cn("h-4 w-4", status?.camera?.live && "animate-pulse")} />
                        : <Camera className={cn("h-4 w-4", status?.camera?.live && "animate-pulse")} />}
                      onClick={() => setVideoStudioOpen(true)}
                    />
                  ) : null}
                  {running && profile?.microphone_enabled ? (
                    <BroadcastControlAction
                      label={t(status?.microphone?.live ? "Silenciar micrófono" : "Micrófono al aire")}
                      description={t(status?.microphone?.live
                        ? "Silencia el micrófono y devuelve el protagonismo a la fuente principal."
                        : "Mezcla el micrófono sobre la fuente principal usando la ganancia configurada.")}
                      variant={status?.microphone?.live ? "destructive" : "secondary"}
                      disabled={!status?.microphone?.ready || ["line_input", "application_audio"].includes(status?.source_mode ?? "") || busy === "microphone"}
                      icon={status?.microphone?.live ? <MicOff className="h-4 w-4" /> : <Mic className="h-4 w-4" />}
                      onClick={() => void toggleMicrophone()}
                    />
                  ) : null}
                  {running && profile?.line_input_enabled ? (
                    <BroadcastControlAction
                      label={t(status?.source_mode === "line_input" ? "Volver a Playlist" : "Línea directa al aire")}
                      description={t(status?.source_mode === "line_input"
                        ? "Cierra la entrada directa y retoma la playlist."
                        : "Reemplaza temporalmente la playlist por la entrada de línea configurada.")}
                      variant={status?.source_mode === "line_input" ? "default" : "secondary"}
                      disabled={!status?.line_input?.ready || status?.source_mode === "application_audio" || busy === "line-input"}
                      icon={<Radio className={cn("h-4 w-4", status?.source_mode === "line_input" && "animate-pulse")} />}
                      onClick={() => void toggleLineInput()}
                    />
                  ) : null}
                  {running && profile?.application_audio_enabled ? (
                    <BroadcastControlAction
                      label={t(status?.source_mode === "application_audio" ? "Volver a Playlist" : "Salida del Mac al aire")}
                      description={t(status?.source_mode === "application_audio"
                        ? "Cierra la captura del Mac y retoma la playlist."
                        : "Reemplaza temporalmente la playlist por el audio del Mac.")}
                      variant={status?.source_mode === "application_audio" ? "default" : "secondary"}
                      disabled={!status?.application_audio?.ready || status?.source_mode === "line_input" || busy === "application-audio"}
                      icon={<AudioLines className={cn("h-4 w-4", status?.source_mode === "application_audio" && "animate-pulse")} />}
                      onClick={() => void toggleApplicationAudio()}
                    />
                  ) : null}
                </div>
              </CardHeader>
              <CardContent className="p-3">
                {status?.source_mode === "application_audio" ? (
                  <div className="rounded-md border border-violet-500/25 bg-violet-500/5 p-4">
                    <span className="text-xs font-semibold uppercase tracking-[0.15em] text-violet-700 dark:text-violet-300">{t("Fuente principal al aire")}</span>
                    <strong className="mt-2 block text-lg">{status.application_audio.label ?? t("Salida del Mac")}</strong>
                    <span className="mt-1 block text-xs text-muted-foreground">
                      {t("Audio estéreo del sistema")} · {t("Playlist en espera")}
                    </span>
                  </div>
                ) : status?.source_mode === "line_input" ? (
                  <div className="rounded-md border border-cyan-500/25 bg-cyan-500/5 p-4">
                    <span className="text-xs font-semibold uppercase tracking-[0.15em] text-cyan-700 dark:text-cyan-300">{t("Fuente principal al aire")}</span>
                    <strong className="mt-2 block text-lg">{t("Línea directa")}</strong>
                    <span className="mt-1 block text-xs text-muted-foreground">
                      {status.line_input.stereo
                        ? t("Canales {left}–{right} estéreo", { left: status.line_input.channel, right: status.line_input.channel + 1 })
                        : t("Canal {channel} mono", { channel: status.line_input.channel })}
                      {" · "}{t("Playlist en espera")}
                    </span>
                  </div>
                ) : status?.now_playing ? (
                  <div className="rounded-md border border-emerald-500/25 bg-emerald-500/5 p-4">
                    <span className="text-xs font-semibold uppercase tracking-[0.15em] text-emerald-700 dark:text-emerald-300">{t("Ahora al aire")}</span>
                    <strong className="mt-2 block text-lg">{entryTitle(status.now_playing)}</strong>
                    <span className="mt-1 block text-xs text-muted-foreground">{status.now_playing.playlist_name}</span>
                  </div>
                ) : (
                  <div className="rounded-md border border-dashed border-border p-4 text-sm text-muted-foreground">
                    {running ? t("La conexión sigue viva transmitiendo silencio hasta que haya una pista.") : t("Configura un destino, agrega una playlist y sal al aire.")}
                  </div>
                )}
                {profile?.microphone_enabled ? (
                  <div className={cn(
                    "mt-3 flex items-center gap-2 rounded-md border px-3 py-2 text-xs",
                    status?.microphone?.live
                      ? "border-red-500/30 bg-red-500/10 text-red-700 dark:text-red-200"
                      : "border-border text-muted-foreground"
                  )}>
                    {status?.microphone?.live ? <Mic className="h-4 w-4 animate-pulse" /> : <MicOff className="h-4 w-4" />}
                    <span className="min-w-0 flex-1 truncate">
                      {translateBackendMessage(locale, status?.microphone?.message ?? t("Micrófono esperando inicio."))}
                    </span>
                    {status?.microphone?.live ? (
                      <div className="flex shrink-0 items-center gap-2" title={t("Nivel de entrada")}>
                        <span className="w-14 text-right tabular-nums">
                          {status.microphone.receiving_audio ? `${status.microphone.level_percent}%` : t("Sin señal")}
                        </span>
                        <div className="h-2 w-20 overflow-hidden rounded-full bg-background/70 ring-1 ring-current/15">
                          <div
                            className={cn(
                              "h-full rounded-full transition-[width] duration-150",
                              status.microphone.level_percent > 80 ? "bg-red-500" : "bg-emerald-500"
                            )}
                            style={{ width: `${status.microphone.level_percent}%` }}
                          />
                        </div>
                      </div>
                    ) : null}
                  </div>
                ) : null}
                {profile?.line_input_enabled ? (
                  <div className={cn(
                    "mt-3 flex items-center gap-2 rounded-md border px-3 py-2 text-xs",
                    status?.line_input?.live
                      ? "border-cyan-500/30 bg-cyan-500/10 text-cyan-800 dark:text-cyan-200"
                      : "border-border text-muted-foreground"
                  )}>
                    <Radio className={cn("h-4 w-4", status?.line_input?.live && "animate-pulse")} />
                    <span className="min-w-0 flex-1 truncate">
                      {translateBackendMessage(locale, status?.line_input?.message ?? t("Línea directa esperando inicio."))}
                    </span>
                    {status?.line_input?.live ? (
                      <div className="flex shrink-0 items-center gap-2" title={t("Nivel de entrada")}>
                        <span className="w-14 text-right tabular-nums">
                          {status.line_input.receiving_audio ? `${status.line_input.level_percent}%` : t("Sin señal")}
                        </span>
                        <div className="h-2 w-20 overflow-hidden rounded-full bg-background/70 ring-1 ring-current/15">
                          <div
                            className={cn(
                              "h-full rounded-full transition-[width] duration-150",
                              status.line_input.level_percent > 80 ? "bg-red-500" : "bg-cyan-500"
                            )}
                            style={{ width: `${status.line_input.level_percent}%` }}
                          />
                        </div>
                      </div>
                    ) : null}
                  </div>
                ) : null}
                {profile?.application_audio_enabled && applicationAudioNeedsAttention ? (
                  <Popover>
                    <PopoverTrigger asChild>
                      <button
                        type="button"
                        className="mt-3 flex w-full min-w-0 items-center gap-2 rounded-md border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-left text-xs text-amber-800 transition-colors hover:bg-amber-500/15 dark:text-amber-200"
                      >
                        <AlertTriangle className="h-4 w-4 shrink-0" />
                        <span className="min-w-0 flex-1 truncate font-medium">
                          {t(applicationAudioPermissionMissing ? "Audio del Mac sin acceso." : "Audio del Mac requiere atención.")}
                        </span>
                        <span className="shrink-0 text-[11px] font-semibold">{t("Ver detalle")}</span>
                      </button>
                    </PopoverTrigger>
                    <PopoverContent align="end" side="top" className="w-80 max-w-[calc(100vw-2rem)] p-3">
                      <div className="flex items-start gap-2.5">
                        <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0 text-amber-500" />
                        <div className="min-w-0">
                          <h4 className="text-sm font-semibold">{t("Audio del Mac requiere atención")}</h4>
                          <p className="mt-1 break-words text-xs leading-relaxed text-muted-foreground">
                            {applicationAudioDetail}
                          </p>
                        </div>
                      </div>
                      <div className="mt-3 flex justify-end border-t border-border pt-3">
                        <Button
                          size="sm"
                          variant="secondary"
                          disabled={busy === "application-settings"}
                          onClick={() => void openApplicationAudioSettings()}
                        >
                          {t("Abrir ajustes")}
                        </Button>
                      </div>
                    </PopoverContent>
                  </Popover>
                ) : profile?.application_audio_enabled ? (
                  <div className={cn(
                    "mt-3 flex items-center gap-2 rounded-md border px-3 py-2 text-xs",
                    status?.application_audio?.live
                      ? "border-violet-500/30 bg-violet-500/10 text-violet-800 dark:text-violet-200"
                      : "border-border text-muted-foreground"
                  )}>
                    <AudioLines className={cn("h-4 w-4", status?.application_audio?.live && "animate-pulse")} />
                    <span className="min-w-0 flex-1 truncate">
                      {applicationAudioDetail}
                    </span>
                    {status?.application_audio?.live ? (
                      <div className="flex shrink-0 items-center gap-2" title={t("Nivel de entrada")}>
                        <span className="w-14 text-right tabular-nums">
                          {status.application_audio.receiving_audio ? `${status.application_audio.level_percent}%` : t("Sin señal")}
                        </span>
                        <div className="h-2 w-20 overflow-hidden rounded-full bg-background/70 ring-1 ring-current/15">
                          <div
                            className={cn(
                              "h-full rounded-full transition-[width] duration-150",
                              status.application_audio.level_percent > 80 ? "bg-red-500" : "bg-violet-500"
                            )}
                            style={{ width: `${status.application_audio.level_percent}%` }}
                          />
                        </div>
                      </div>
                    ) : null}
                  </div>
                ) : null}
              </CardContent>
            </Card>
            <Card className="overflow-hidden">
              <CardHeader className="py-3">
                <div className="min-w-0">
                  <CardTitle>{t("Fuentes de entrada")}</CardTitle>
                  <p className="mt-1 text-xs text-muted-foreground">{t("Esta configuración es global y se aplica a todos los destinos de salida.")}</p>
                </div>
              </CardHeader>
              <CardContent className="grid gap-3 p-3 pt-0">
                {renderControlSettingsEditor()}
                <div className="flex items-center justify-between gap-3">
                  <span className="text-xs text-muted-foreground">
                    {controlNeedsSave ? t("Hay cambios sin guardar.") : t("Fuentes actualizadas.")}
                  </span>
                  <Button
                    type="button"
                    size="sm"
                    disabled={running || !controlNeedsSave || busy === "control-settings"}
                    onClick={() => void saveControlSettings()}
                  >
                    {busy === "control-settings" ? <LoaderCircle className="h-4 w-4 animate-spin" /> : <Save className="h-4 w-4" />}
                    {t("Guardar fuentes")}
                  </Button>
                </div>
              </CardContent>
            </Card>
            </div> : null}

            <Card className={cn(
              "flex min-h-0 max-h-[860px] flex-col overflow-hidden",
              workspaceTab === "control"
                ? terminalExpanded
                  ? "h-[calc(100dvh-24.5rem)]"
                  : "h-[calc(100dvh-12rem)]"
                : terminalExpanded
                  ? "h-[calc(100dvh-17.25rem)]"
                  : "h-[calc(100dvh-4.75rem)]"
            )}>
              <CardHeader className="flex-wrap py-2">
                <div className="min-w-0">
                  <CardTitle>{t(workspaceTab === "schedule" ? "Parrilla musical" : "Cola al aire")}</CardTitle>
                  {workspaceTab === "control" && automation.mode === "scheduled" ? (
                    <p className="mt-1 text-xs text-muted-foreground">{t("La parrilla está activa; esta cola refleja los bloques programados.")}</p>
                  ) : null}
                </div>
                <div className="ml-auto flex flex-wrap items-center gap-2">
                  {workspaceTab === "schedule" ? <div className="flex rounded-md border border-border bg-secondary p-0.5">
                    <Button
                      size="sm"
                      variant={automation.mode === "immediate" ? "default" : "ghost"}
                      disabled={busy === "automation-mode"}
                      onClick={() => void changeBroadcastMode("immediate")}
                    >
                      {t("Inmediato")}
                    </Button>
                    <Button
                      size="sm"
                      variant={automation.mode === "scheduled" ? "default" : "ghost"}
                      disabled={busy === "automation-mode"}
                      onClick={() => void changeBroadcastMode("scheduled")}
                    >
                      <Calendar className="h-3.5 w-3.5" />
                      {t("Programado")}
                    </Button>
                  </div> : null}
                  {workspaceTab === "control" ? <><select
                    aria-label={t("Ordenar pistas")}
                    className="h-8 max-w-36 rounded-md border border-input bg-background px-2 text-xs text-foreground outline-none disabled:opacity-50"
                    value=""
                    disabled={queuedTotal === 0 || busy === "reordering"}
                    onChange={(event) => void sortQueuedEntries(event.currentTarget.value as "title" | "artist" | "duration")}
                  >
                    <option value="" disabled>{t("Ordenar próximas...")}</option>
                    <option value="title">{t("Título A–Z")}</option>
                    <option value="artist">{t("Artista A–Z")}</option>
                    <option value="duration">{t("Duración menor primero")}</option>
                  </select>
                  <Button size="sm" variant="ghost" disabled={queuedTotal === 0 || busy === "clearing"} onClick={() => void clearQueue()}>
                    <Trash2 className="h-4 w-4" />
                    {t("Limpiar")}
                  </Button>
                  </> : null}
                </div>
              </CardHeader>
              {workspaceTab === "control" ? <CardContent className="flex min-h-0 flex-1 flex-col overflow-hidden">
                {automation.mode === "immediate" ? <div className="grid shrink-0 gap-2 border-b border-border p-3 md:grid-cols-[minmax(260px,1fr)_auto]">
                  <Popover open={playlistComboboxOpen} onOpenChange={setPlaylistComboboxOpen}>
                    <PopoverTrigger asChild>
                      <Button
                        variant="secondary"
                        role="combobox"
                        aria-expanded={playlistComboboxOpen}
                        className="h-10 min-w-0 justify-between border border-input bg-background px-3 font-normal hover:bg-accent"
                      >
                        {selectedPlaylistSource ? (
                          <span className="min-w-0 truncate">
                            <span className="font-medium">{selectedPlaylistSource.name}</span>
                            <span className="text-muted-foreground">
                              {" · "}{t(selectedPlaylistSource.kind === "local" ? "Local" : "Rekordbox")}{" · "}{selectedPlaylistSource.track_count} {t("tracks")}
                            </span>
                          </span>
                        ) : (
                          <span className="truncate text-muted-foreground">{t("Buscar una playlist para agregar...")}</span>
                        )}
                        <ChevronsUpDown className="ml-2 h-4 w-4 shrink-0 text-muted-foreground" />
                      </Button>
                    </PopoverTrigger>
                    <PopoverContent
                      align="start"
                      className="w-[var(--radix-popover-trigger-width)] max-w-[calc(100vw-2rem)]"
                    >
                      <Command>
                        <CommandInput placeholder={t("Buscar por nombre, biblioteca u origen...")} />
                        <CommandList>
                          <CommandEmpty>{t("No se encontraron playlists.")}</CommandEmpty>
                          {[
                            { label: t("Playlists locales"), items: localPlaylistSources },
                            { label: t("Playlists de Rekordbox"), items: rekordboxPlaylistSources }
                          ].map((group) => group.items.length > 0 ? (
                            <CommandGroup key={group.label} heading={group.label}>
                              {group.items.map((source) => (
                                <CommandItem
                                  key={source.key}
                                  value={`${source.key} ${source.name} ${source.library_name} ${source.kind}`}
                                  onSelect={() => {
                                    setPlaylistSourceKey(source.key);
                                    setPlaylistComboboxOpen(false);
                                  }}
                                >
                                  <Check className={cn("mr-2 h-4 w-4 shrink-0", playlistSourceKey === source.key ? "opacity-100" : "opacity-0")} />
                                  <span className="min-w-0 flex-1">
                                    <span className="block truncate font-medium">{source.name}</span>
                                    <span className="block truncate text-xs text-muted-foreground">
                                      {source.library_name} · {source.track_count} {t("tracks")}
                                    </span>
                                  </span>
                                  <span className={cn(
                                    "ml-3 shrink-0 rounded-full px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wide",
                                    source.kind === "local"
                                      ? "bg-emerald-500/10 text-emerald-700 dark:text-emerald-300"
                                      : "bg-blue-500/10 text-blue-700 dark:text-blue-300"
                                  )}>
                                    {t(source.kind === "local" ? "Local" : "Rekordbox")}
                                  </span>
                                </CommandItem>
                              ))}
                            </CommandGroup>
                          ) : null)}
                        </CommandList>
                      </Command>
                    </PopoverContent>
                  </Popover>
                  <Button disabled={!selectedPlaylistSource || busy === "appending"} onClick={() => void appendPlaylist()}>
                    {busy === "appending" ? <LoaderCircle className="h-4 w-4 animate-spin" /> : <Plus className="h-4 w-4" />}
                    {t("Agregar")}
                  </Button>
                </div> : (
                  <button
                    type="button"
                    className="flex shrink-0 items-center justify-between gap-3 border-b border-border bg-emerald-500/5 p-3 text-left text-xs text-muted-foreground hover:bg-emerald-500/10"
                    onClick={() => setWorkspaceTab("schedule")}
                  >
                    <span><strong className="text-foreground">{t("Parrilla activa")}</strong> · {t("Los bloques se cargan automáticamente según su horario.")}</span>
                    <span className="shrink-0 font-semibold text-foreground">{t("Abrir parrilla")}</span>
                  </button>
                )}
                {modeQueue.length === 0 ? (
                  <div className="grid min-h-0 flex-1 place-items-center p-6 text-sm text-muted-foreground">{t("La cola está vacía.")}</div>
                ) : (
                  <div ref={queueScrollElement} className="min-h-0 flex-1 divide-y divide-border overflow-y-auto overscroll-contain [scrollbar-gutter:stable]">
                    {modeQueue.map((entry) => {
                      const queuedIndex = queuedEntries.findIndex((queuedEntry) => queuedEntry.id === entry.id);
                      const canSelectTrack = running
                        && status?.status !== "stopping"
                        && status?.source_mode === "playlist"
                        && entry.status === "queued";
                      return (
                      <div
                        key={entry.id}
                        ref={entry.id === playingQueueEntryId ? playingQueueEntryElement : undefined}
                        className={cn(
                          "grid grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-2 px-2 py-2.5 transition-colors",
                          entry.status === "playing" && "bg-emerald-500/5",
                          ["played", "skipped"].includes(entry.status) && "bg-muted/10 text-muted-foreground",
                          entry.status === "failed" && "bg-destructive/5",
                          draggedQueueEntryId && entry.status === "queued" && entry.id !== draggedQueueEntryId && "hover:bg-accent/60"
                        )}
                        onDragOver={(event) => {
                          if (entry.status !== "queued" || !draggedQueueEntryId) return;
                          event.preventDefault();
                          event.dataTransfer.dropEffect = "move";
                        }}
                        onDrop={(event) => {
                          event.preventDefault();
                          const sourceId = draggedQueueEntryId ?? event.dataTransfer.getData("text/plain");
                          setDraggedQueueEntryId(null);
                          if (sourceId && entry.status === "queued") void moveQueuedEntryToTarget(sourceId, entry.id);
                        }}
                      >
                        <button
                          type="button"
                          draggable={entry.status === "queued" && busy === null}
                          className={cn(
                            "grid h-8 w-6 shrink-0 place-items-center rounded text-muted-foreground",
                            entry.status === "queued" ? "cursor-grab hover:bg-accent hover:text-foreground active:cursor-grabbing" : "cursor-not-allowed opacity-25"
                          )}
                          aria-label={t("Arrastrar para reordenar")}
                          title={t("Arrastrar para reordenar")}
                          onDragStart={(event) => {
                            if (entry.status !== "queued") return;
                            setDraggedQueueEntryId(entry.id);
                            event.dataTransfer.effectAllowed = "move";
                            event.dataTransfer.setData("text/plain", entry.id);
                          }}
                          onDragEnd={() => setDraggedQueueEntryId(null)}
                        >
                          <GripVertical className="h-4 w-4" />
                        </button>
                        <div className="min-w-0">
                          <div className="flex min-w-0 items-center gap-2">
                            <span className="truncate text-sm font-medium">{entryTitle(entry)}</span>
                            <QueueStatus status={entry.status} />
                          </div>
                          <span className="mt-0.5 block truncate text-xs text-muted-foreground">{t(entry.playlist_name)} · {formatDuration(entry.duration_seconds)}</span>
                          {entry.error ? <span className="mt-1 block text-xs text-destructive">{entry.error}</span> : null}
                        </div>
                        <div className="flex items-center gap-0.5">
                          <Button
                            size="icon"
                            variant={entry.status === "playing" ? "secondary" : "ghost"}
                            aria-label={entry.status === "playing" ? t("Pista al aire") : t("Reproducir ahora")}
                            title={entry.status === "playing" ? t("Pista al aire") : t("Reproducir ahora")}
                            disabled={!canSelectTrack || busy === `play:${entry.id}`}
                            onClick={() => void playQueueEntry(entry)}
                          >
                            {busy === `play:${entry.id}` ? <LoaderCircle className="h-4 w-4 animate-spin" /> : <Play className="h-4 w-4" />}
                          </Button>
                          <Button
                            size="icon"
                            variant="ghost"
                            aria-label={t("Mover hacia arriba")}
                            title={t("Mover hacia arriba")}
                            disabled={queuedIndex <= 0 || busy === "reordering"}
                            onClick={() => void moveQueuedEntry(entry.id, -1)}
                          >
                            <ArrowUp className="h-3.5 w-3.5" />
                          </Button>
                          <Button
                            size="icon"
                            variant="ghost"
                            aria-label={t("Mover hacia abajo")}
                            title={t("Mover hacia abajo")}
                            disabled={queuedIndex < 0 || queuedIndex >= queuedEntries.length - 1 || busy === "reordering"}
                            onClick={() => void moveQueuedEntry(entry.id, 1)}
                          >
                            <ArrowDown className="h-3.5 w-3.5" />
                          </Button>
                          <Button
                            size="icon"
                            variant="ghost"
                            aria-label={t("Quitar de la cola")}
                            disabled={entry.status !== "queued" || busy === `remove:${entry.id}`}
                            onClick={() => void removeEntry(entry.id)}
                          >
                            {busy === `remove:${entry.id}` ? <LoaderCircle className="h-4 w-4 animate-spin" /> : <Trash2 className="h-4 w-4" />}
                          </Button>
                        </div>
                      </div>
                    )})}
                  </div>
                )}
              </CardContent> : (
                <ScheduledBroadcastPanel
                  sources={playlistSources}
                  items={scheduleItems}
                  automation={automation}
                  running={running}
                  onItemsChange={setScheduleItems}
                  onAutomationChange={setAutomation}
                  onError={setError}
                  onNotice={setNotice}
                />
              )}
            </Card>
          </div> : null}
        </section>

      </div>
      <VideoStudioModal
        open={videoStudioOpen}
        config={videoCompositor}
        devices={cameraDevices}
        stationName={stationName}
        trackTitle={status?.now_playing ? entryTitle(status.now_playing) : t("WAITING FOR NEXT TRACK")}
        running={running}
        cameraReady={status?.camera?.ready ?? false}
        mixPercent={cameraMix}
        busy={busy}
        onClose={() => setVideoStudioOpen(false)}
        onChange={(next) => void changeVideoCompositor(next)}
        onMix={sendCameraMix}
        onSave={async () => {
          if (await persistProfile()) setVideoStudioOpen(false);
        }}
      />
      <TerminalDrawer
        logs={terminalLogs}
        expanded={terminalExpanded}
        terminalRef={terminalElement}
        subtitle={t("ffmpeg / destinos / entradas de audio")}
        emptyMessage="Sin eventos todavía."
        onToggle={() => setTerminalExpanded((current) => !current)}
        onClear={clearTerminal}
      />
    </main>
  );
}

function DestinationProfilesCard({
  profiles,
  activeId,
  running,
  busy,
  creating,
  newName,
  onCreatingChange,
  onNewNameChange,
  onCreate,
  onActivate,
  onDelete,
  t
}: {
  profiles: BroadcastProfile[];
  activeId: string | null;
  running: boolean;
  busy: BusyAction;
  creating: boolean;
  newName: string;
  onCreatingChange: (value: boolean) => void;
  onNewNameChange: (value: string) => void;
  onCreate: (event: FormEvent) => void;
  onActivate: (profileId: string) => void;
  onDelete: (profile: BroadcastProfile) => void;
  t: (key: string, values?: Record<string, string | number | null | undefined>) => string;
}) {
  return (
    <Card className="self-start overflow-hidden xl:sticky xl:top-0">
      <CardHeader>
        <div>
          <CardTitle>{t("Destinos guardados")}</CardTitle>
          <p className="mt-1 text-xs text-muted-foreground">
            {t("Selecciona qué servidor recibirá la próxima transmisión.")}
          </p>
        </div>
        <Button
          size="sm"
          variant="secondary"
          disabled={running || busy !== null}
          onClick={() => onCreatingChange(true)}
        >
          <Plus className="h-4 w-4" />
          {t("Nuevo")}
        </Button>
      </CardHeader>
      <CardContent className="p-3">
        {creating ? (
          <form className="mb-3 grid gap-2 rounded-lg border border-border bg-secondary/40 p-3" onSubmit={onCreate}>
            <label className="text-xs font-semibold" htmlFor="new-broadcast-destination">{t("Nombre del nuevo destino")}</label>
            <input
              id="new-broadcast-destination"
              className={fieldClass}
              value={newName}
              maxLength={80}
              autoFocus
              placeholder={t("Ej. Radio secundaria")}
              onChange={(event) => onNewNameChange(event.target.value)}
            />
            <div className="flex justify-end gap-2">
              <Button
                type="button"
                size="sm"
                variant="ghost"
                onClick={() => {
                  onCreatingChange(false);
                  onNewNameChange("");
                }}
              >
                {t("Cancelar")}
              </Button>
              <Button type="submit" size="sm" disabled={!newName.trim() || busy === "destination-create"}>
                {busy === "destination-create" ? <LoaderCircle className="h-4 w-4 animate-spin" /> : <Plus className="h-4 w-4" />}
                {t("Crear y activar")}
              </Button>
            </div>
          </form>
        ) : null}
        <div className="max-h-[min(620px,calc(100vh-19rem))] space-y-2 overflow-y-auto overscroll-contain pr-1">
          {profiles.map((destination) => {
            const active = destination.id === activeId;
            const endpoint = destination.output_kind === "rtmp"
              ? destination.rtmp_server_url || t("RTMP sin configurar")
              : destination.listener_url;
            return (
              <div
                key={destination.id}
                className={cn(
                  "rounded-lg border p-3 transition-colors",
                  active ? "border-emerald-500/35 bg-emerald-500/5" : "border-border bg-background"
                )}
              >
                <div className="flex items-start gap-3">
                  <span className={cn(
                    "grid h-9 w-9 shrink-0 place-items-center rounded-md",
                    active ? "bg-emerald-500/15 text-emerald-700 dark:text-emerald-300" : "bg-secondary text-muted-foreground"
                  )}>
                    {destination.output_kind === "rtmp" ? <Monitor className="h-4 w-4" /> : <Radio className="h-4 w-4" />}
                  </span>
                  <div className="min-w-0 flex-1">
                    <div className="flex min-w-0 items-center gap-2">
                      <strong className="truncate text-sm">{destination.name}</strong>
                      {active ? (
                        <span className="shrink-0 rounded-full bg-emerald-500/15 px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wide text-emerald-700 dark:text-emerald-300">
                          {t("Activo")}
                        </span>
                      ) : null}
                    </div>
                    <span className="mt-1 block truncate text-xs text-muted-foreground">{endpoint}</span>
                    <span className="mt-1 block text-[10px] font-semibold uppercase tracking-wide text-muted-foreground">
                      {destination.output_kind === "rtmp" ? "RTMP" : "Icecast"}
                    </span>
                  </div>
                </div>
                <div className="mt-3 flex justify-end gap-1 border-t border-border/70 pt-2">
                  <Button
                    size="sm"
                    variant={active ? "secondary" : "default"}
                    disabled={active || running || busy !== null}
                    onClick={() => onActivate(destination.id)}
                  >
                    {busy === `destination-activate:${destination.id}` ? <LoaderCircle className="h-4 w-4 animate-spin" /> : <Check className="h-4 w-4" />}
                    {active ? t("Seleccionado") : t("Activar")}
                  </Button>
                  <Button
                    size="icon"
                    variant="ghost"
                    aria-label={t("Eliminar destino")}
                    title={t("Eliminar destino")}
                    disabled={profiles.length <= 1 || running || busy !== null}
                    onClick={() => onDelete(destination)}
                  >
                    {busy === `destination-delete:${destination.id}` ? <LoaderCircle className="h-4 w-4 animate-spin" /> : <Trash2 className="h-4 w-4" />}
                  </Button>
                </div>
              </div>
            );
          })}
        </div>
        {running ? (
          <p className="mt-3 rounded-md border border-amber-500/25 bg-amber-500/5 p-2 text-xs text-muted-foreground">
            {t("Detén la transmisión para cambiar o crear destinos.")}
          </p>
        ) : null}
      </CardContent>
    </Card>
  );
}

function ScheduledBroadcastPanel({
  sources,
  items,
  automation,
  running,
  onItemsChange,
  onAutomationChange,
  onError,
  onNotice
}: {
  sources: BroadcastPlaylistSource[];
  items: BroadcastScheduleItem[];
  automation: BroadcastAutomationSettings;
  running: boolean;
  onItemsChange: (items: BroadcastScheduleItem[]) => void;
  onAutomationChange: (settings: BroadcastAutomationSettings) => void;
  onError: (message: string | null) => void;
  onNotice: (message: string | null) => void;
}) {
  const { locale, t } = useI18n();
  const [date, setDate] = useState(() => localDateInputValue(new Date()));
  const [time, setTime] = useState(() => nextScheduleTime());
  const [policy, setPolicy] = useState<"soft" | "exact">("soft");
  const [sourceKey, setSourceKey] = useState("");
  const [sourceTracks, setSourceTracks] = useState<BroadcastSourceTrack[]>([]);
  const [trackId, setTrackId] = useState("");
  const [loadingTracks, setLoadingTracks] = useState(false);
  const [savingItem, setSavingItem] = useState(false);
  const [deletingItemId, setDeletingItemId] = useState("");
  const [editingItem, setEditingItem] = useState<BroadcastScheduleItem | null>(null);
  const [loadingEditorId, setLoadingEditorId] = useState("");
  const [bedEnabled, setBedEnabled] = useState(automation.bed_enabled);
  const [bedSourceKey, setBedSourceKey] = useState("");
  const [bedTracks, setBedTracks] = useState<BroadcastSourceTrack[]>([]);
  const [bedTrackId, setBedTrackId] = useState(automation.bed_track_id ?? "");
  const [bedGain, setBedGain] = useState(String(automation.bed_gain_percent));
  const [bedDucking, setBedDucking] = useState(automation.bed_ducking);
  const [savingBed, setSavingBed] = useState(false);
  const selectedSource = sources.find((source) => source.key === sourceKey) ?? null;
  const selectedBedSource = sources.find((source) => source.key === bedSourceKey) ?? null;
  const dayItems = useMemo(
    () => items.filter((item) => localDateInputValue(new Date(item.start_at)) === date),
    [date, items]
  );
  const overlapById = useMemo(() => scheduleOverlapInfo(items), [items]);
  const editableActivatedItemId = useMemo(
    () => items
      .filter((item) => item.status === "activated")
      .sort((left, right) => {
        const leftKey = left.activated_at ?? left.updated_at;
        const rightKey = right.activated_at ?? right.updated_at;
        return rightKey.localeCompare(leftKey) || right.start_at.localeCompare(left.start_at);
      })[0]?.id ?? "",
    [items]
  );

  useEffect(() => {
    setBedEnabled(automation.bed_enabled);
    setBedTrackId(automation.bed_track_id ?? "");
    setBedGain(String(automation.bed_gain_percent));
    setBedDucking(automation.bed_ducking);
  }, [automation]);

  useEffect(() => {
    if (!selectedSource) {
      setSourceTracks([]);
      setTrackId("");
      return;
    }
    let disposed = false;
    setLoadingTracks(true);
    void loadBroadcastSourceTracks(selectedSource)
      .then((tracks) => {
        if (!disposed) setSourceTracks(tracks.filter((track) => track.source_exists && track.source_path));
      })
      .catch((cause) => !disposed && onError(errorMessage(cause, locale)))
      .finally(() => !disposed && setLoadingTracks(false));
    return () => { disposed = true; };
  }, [locale, selectedSource?.key]);

  useEffect(() => {
    if (!selectedBedSource) {
      setBedTracks([]);
      return;
    }
    let disposed = false;
    void loadBroadcastSourceTracks(selectedBedSource)
      .then((tracks) => {
        if (!disposed) setBedTracks(tracks.filter((track) => track.source_exists && track.source_path));
      })
      .catch((cause) => !disposed && onError(errorMessage(cause, locale)));
    return () => { disposed = true; };
  }, [locale, selectedBedSource?.key]);

  async function refreshItems() {
    onItemsChange(await invoke<BroadcastScheduleItem[]>("broadcast_schedule_items"));
  }

  async function createItem() {
    if (!selectedSource || !date || !time) return;
    const startsAt = new Date(`${date}T${time}:00`);
    if (Number.isNaN(startsAt.getTime())) {
      onError(t("La fecha y hora programadas no son válidas."));
      return;
    }
    setSavingItem(true);
    onError(null);
    onNotice(null);
    try {
      await invoke<BroadcastScheduleItem>("broadcast_create_schedule_item", {
        item: {
          startAt: startsAt.toISOString(),
          policy,
          sourceKind: trackId ? "track" : selectedSource.kind === "local" ? "draft" : "playlist",
          libraryId: selectedSource.library_id,
          sourceId: trackId || selectedSource.id
        }
      });
      await refreshItems();
      setTrackId("");
      onNotice(t("Bloque agregado a la parrilla."));
    } catch (cause) {
      onError(errorMessage(cause, locale));
    } finally {
      setSavingItem(false);
    }
  }

  async function deleteItem(itemId: string) {
    setDeletingItemId(itemId);
    onError(null);
    try {
      await invoke("broadcast_delete_schedule_item", { itemId });
      await refreshItems();
    } catch (cause) {
      onError(errorMessage(cause, locale));
    } finally {
      setDeletingItemId("");
    }
  }

  async function openItemEditor(itemId: string) {
    setLoadingEditorId(itemId);
    onError(null);
    try {
      setEditingItem(await invoke<BroadcastScheduleItem>("broadcast_schedule_item", { itemId }));
    } catch (cause) {
      onError(errorMessage(cause, locale));
    } finally {
      setLoadingEditorId("");
    }
  }

  async function handleEditedItem(next: BroadcastScheduleItem, notice?: string) {
    setEditingItem(next);
    await refreshItems();
    if (notice) onNotice(notice);
  }

  async function saveBed() {
    const libraryId = selectedBedSource?.library_id ?? automation.bed_library_id ?? null;
    const selectedTrackId = bedTrackId || automation.bed_track_id || null;
    setSavingBed(true);
    onError(null);
    onNotice(null);
    try {
      const saved = await invoke<BroadcastAutomationSettings>("broadcast_save_automation_settings", {
        settings: {
          mode: "scheduled",
          bedEnabled,
          bedLibraryId: libraryId,
          bedTrackId: selectedTrackId,
          bedGainPercent: Number(bedGain),
          bedDucking
        }
      });
      onAutomationChange(saved);
      onNotice(t("Cortina musical guardada."));
    } catch (cause) {
      onError(errorMessage(cause, locale));
    } finally {
      setSavingBed(false);
    }
  }

  return (
    <CardContent className="min-h-0 flex-1 overflow-y-auto p-3">
      <div className="grid gap-3">
        <section className="rounded-md border border-border bg-secondary/30 p-3">
          <div className="flex flex-wrap items-start justify-between gap-3">
            <div>
              <h3 className="text-sm font-semibold">{t("Agregar bloque horario")}</h3>
              <p className="mt-1 text-xs text-muted-foreground">
                {t("Programa una playlist completa o una pista individual para este día.")}
              </p>
            </div>
            <div className="flex items-center gap-2">
              <input className={cn(fieldClass, "h-9 w-auto")} type="date" value={date} onChange={(event) => setDate(event.currentTarget.value)} />
              <input className={cn(fieldClass, "h-9 w-auto")} type="time" value={time} onChange={(event) => setTime(event.currentTarget.value)} />
            </div>
          </div>
          <div className="mt-3 grid gap-2 lg:grid-cols-[minmax(180px,1fr)_minmax(180px,1fr)_220px_auto]">
            <BroadcastCombobox
              value={sourceKey}
              placeholder={t("Selecciona una playlist")}
              searchPlaceholder={t("Buscar por nombre, biblioteca u origen...")}
              options={sources.map((source) => ({
                value: source.key,
                label: source.name,
                detail: `${source.library_name} · ${source.track_count} ${t("tracks")}`,
                keywords: `${source.kind} ${source.library_name}`
              }))}
              onChange={(value) => {
                setSourceKey(value);
                setTrackId("");
              }}
            />
            <BroadcastCombobox
              value={trackId}
              disabled={!selectedSource || loadingTracks}
              placeholder={loadingTracks ? t("Cargando tracks...") : t("Playlist completa")}
              searchPlaceholder={t("Buscar un track...")}
              emptyLabel={t("No se encontraron tracks.")}
              options={[
                { value: "", label: t("Playlist completa"), detail: selectedSource ? `${selectedSource.track_count} ${t("tracks")}` : undefined },
                ...sourceTracks.map((track) => ({
                  value: track.track_id,
                  label: track.name?.trim() || t("Sin titulo"),
                  detail: track.artist?.trim() || formatDuration(track.total_time),
                  keywords: track.artist ?? ""
                }))
              ]}
              onChange={setTrackId}
            />
            <div className="grid grid-cols-2 gap-1 rounded-md border border-border bg-secondary p-1">
              <Button size="sm" variant={policy === "soft" ? "default" : "ghost"} onClick={() => setPolicy("soft")}>{t("Al terminar")}</Button>
              <Button size="sm" variant={policy === "exact" ? "default" : "ghost"} onClick={() => setPolicy("exact")}>{t("Hora exacta")}</Button>
            </div>
            <Button disabled={!selectedSource || savingItem} onClick={() => void createItem()}>
              {savingItem ? <LoaderCircle className="h-4 w-4 animate-spin" /> : <Plus className="h-4 w-4" />}
              {t("Programar")}
            </Button>
          </div>
        </section>

        <section className="overflow-hidden rounded-md border border-border">
          <header className="flex flex-wrap items-center justify-between gap-2 border-b border-border bg-secondary/60 px-3 py-2">
            <div className="flex items-center gap-2">
              <Calendar className="h-4 w-4" />
              <strong className="text-sm">{formatScheduleDay(date, locale)}</strong>
              <span className="text-xs text-muted-foreground">{dayItems.length} {t("bloques")}</span>
            </div>
            <span className="text-xs text-muted-foreground">{t("Zona horaria local")}</span>
          </header>
          {dayItems.length === 0 ? (
            <div className="grid min-h-40 place-items-center p-6 text-sm text-muted-foreground">
              {t("No hay bloques programados para este día.")}
            </div>
          ) : (
            <div className="divide-y divide-border">
              {dayItems.map((item) => {
                const overlap = overlapById.get(item.id);
                const editable = item.status === "pending" || item.id === editableActivatedItemId;
                return (
                  <div key={item.id} className="grid grid-cols-[64px_minmax(0,1fr)_auto] items-center gap-3 px-3 py-3">
                  <time className="font-mono text-sm font-semibold">{formatScheduleClock(item.start_at)}</time>
                  <div className="min-w-0">
                    <div className="flex min-w-0 flex-wrap items-center gap-2">
                      <strong className="truncate text-sm">{item.source_name}</strong>
                      <ScheduleStatus status={item.status} />
                      <span className={cn(
                        "rounded-full px-2 py-0.5 text-[10px] font-semibold uppercase",
                        item.policy === "exact" ? "bg-amber-500/10 text-amber-700 dark:text-amber-300" : "bg-blue-500/10 text-blue-700 dark:text-blue-300"
                      )}>
                        {t(item.policy === "exact" ? "Hora exacta" : "Al terminar")}
                      </span>
                      {overlap ? (
                        <span className="inline-flex items-center gap-1 text-xs font-medium text-amber-600">
                          {t("Se cruza con el siguiente bloque")}
                          <Popover>
                            <PopoverTrigger asChild>
                              <button
                                type="button"
                                className="grid h-6 w-6 place-items-center rounded-full text-amber-600 transition-colors hover:bg-amber-500/10 hover:text-amber-700"
                                aria-label={t("Explicar cruce de bloques")}
                              >
                                <Info className="h-4 w-4" />
                              </button>
                            </PopoverTrigger>
                            <PopoverContent align="start" className="w-80 max-w-[calc(100vw-2rem)] p-3">
                              <div className="flex items-start gap-2.5">
                                <Info className="mt-0.5 h-4 w-4 shrink-0 text-amber-500" />
                                <div className="min-w-0">
                                  <h4 className="text-sm font-semibold text-foreground">{t("Este bloque excede su ventana")}</h4>
                                  <p className="mt-1 text-xs leading-relaxed text-muted-foreground">
                                    {overlap.next.policy === "exact"
                                      ? t("A las {time}, {name} comenzará inmediatamente y cortará la pista que esté sonando.", {
                                          time: formatScheduleClock(overlap.next.start_at),
                                          name: overlap.next.source_name
                                        })
                                      : t("A las {time}, {name} esperará el final de la pista que esté sonando y luego comenzará.", {
                                          time: formatScheduleClock(overlap.next.start_at),
                                          name: overlap.next.source_name
                                        })}
                                  </p>
                                  <p className="mt-2 text-xs leading-relaxed text-muted-foreground">
                                    {t("Las pistas pendientes de este bloque se omitirán y no se retomarán después.")}
                                  </p>
                                  <dl className="mt-3 grid grid-cols-3 gap-2 border-t border-border pt-3 text-center">
                                    <div>
                                      <dt className="text-[10px] uppercase tracking-wide text-muted-foreground">{t("Ventana")}</dt>
                                      <dd className="mt-0.5 text-xs font-semibold text-foreground">{formatDuration(overlap.windowSeconds)}</dd>
                                    </div>
                                    <div>
                                      <dt className="text-[10px] uppercase tracking-wide text-muted-foreground">{t("Contenido")}</dt>
                                      <dd className="mt-0.5 text-xs font-semibold text-foreground">{formatDuration(item.duration_seconds)}</dd>
                                    </div>
                                    <div>
                                      <dt className="text-[10px] uppercase tracking-wide text-muted-foreground">{t("Exceso estimado")}</dt>
                                      <dd className="mt-0.5 text-xs font-semibold text-amber-600">{formatDuration(overlap.overflowSeconds)}</dd>
                                    </div>
                                  </dl>
                                </div>
                              </div>
                            </PopoverContent>
                          </Popover>
                        </span>
                      ) : null}
                    </div>
                    <span className="mt-1 block truncate text-xs text-muted-foreground">
                      {item.track_count} {t("tracks")} · {formatDuration(item.duration_seconds)}
                    </span>
                    {item.error ? <span className="mt-1 block text-xs text-destructive">{t(item.error)}</span> : null}
                  </div>
                  <div className="flex items-center gap-1">
                    <Button
                      size="sm"
                      variant="secondary"
                      disabled={!editable || loadingEditorId === item.id}
                      title={editable ? t("Editar") : t("Sólo se puede editar el bloque activo actual.")}
                      onClick={() => void openItemEditor(item.id)}
                    >
                      {loadingEditorId === item.id ? <LoaderCircle className="h-4 w-4 animate-spin" /> : <SlidersHorizontal className="h-4 w-4" />}
                      {t("Editar")}
                    </Button>
                    <Button
                      size="icon"
                      variant="ghost"
                      disabled={item.status !== "pending" || deletingItemId === item.id}
                      aria-label={t("Eliminar")}
                      onClick={() => void deleteItem(item.id)}
                    >
                      {deletingItemId === item.id ? <LoaderCircle className="h-4 w-4 animate-spin" /> : <Trash2 className="h-4 w-4" />}
                    </Button>
                  </div>
                  </div>
                );
              })}
            </div>
          )}
        </section>

        <section className="rounded-md border border-border bg-card p-3">
          <div className="flex items-start gap-3">
            <div className="grid h-9 w-9 shrink-0 place-items-center rounded-md bg-violet-500/10 text-violet-700 dark:text-violet-300">
              <Music2 className="h-4 w-4" />
            </div>
            <div className="min-w-0 flex-1">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <div>
                  <h3 className="text-sm font-semibold">{t("Cortina musical")}</h3>
                  <p className="mt-1 text-xs text-muted-foreground">
                    {t("Se reproduce en loop cuando la cola queda vacía y baja al abrir el micrófono.")}
                  </p>
                </div>
                <label className="flex items-center gap-2 text-xs font-medium">
                  <input type="checkbox" checked={bedEnabled} disabled={running} onChange={(event) => setBedEnabled(event.currentTarget.checked)} />
                  {t("Activar cortina")}
                </label>
              </div>
              <div className="mt-3 grid gap-2 lg:grid-cols-[minmax(180px,1fr)_minmax(180px,1fr)_130px_auto]">
                <BroadcastCombobox
                  value={bedSourceKey}
                  disabled={running}
                  placeholder={automation.bed_title ? `${t("Actual")}: ${automation.bed_title}` : t("Selecciona una playlist")}
                  searchPlaceholder={t("Buscar por nombre, biblioteca u origen...")}
                  options={sources.map((source) => ({
                    value: source.key,
                    label: source.name,
                    detail: `${source.library_name} · ${source.track_count} ${t("tracks")}`,
                    keywords: `${source.kind} ${source.library_name}`
                  }))}
                  onChange={(value) => {
                    setBedSourceKey(value);
                    setBedTrackId("");
                  }}
                />
                <BroadcastCombobox
                  value={bedTrackId}
                  disabled={running || (!selectedBedSource && !automation.bed_track_id)}
                  placeholder={automation.bed_title ?? t("Selecciona un track")}
                  searchPlaceholder={t("Buscar un track...")}
                  emptyLabel={t("No se encontraron tracks.")}
                  options={[
                    ...(automation.bed_track_id && !bedTracks.some((track) => track.track_id === automation.bed_track_id)
                      ? [{ value: automation.bed_track_id, label: automation.bed_title ?? t("Cortina actual"), detail: automation.bed_artist ?? undefined }]
                      : []),
                    ...bedTracks.map((track) => ({
                      value: track.track_id,
                      label: track.name?.trim() || t("Sin titulo"),
                      detail: track.artist?.trim() || formatDuration(track.total_time),
                      keywords: track.artist ?? ""
                    }))
                  ]}
                  onChange={setBedTrackId}
                />
                <label className="grid gap-1 text-xs font-medium">
                  {t("Volumen")} · {bedGain}%
                  <input type="range" min={0} max={100} step={1} value={bedGain} disabled={running} onChange={(event) => setBedGain(event.currentTarget.value)} />
                </label>
                <Button variant="secondary" disabled={running || savingBed || (bedEnabled && !bedTrackId && !automation.bed_track_id)} onClick={() => void saveBed()}>
                  {savingBed ? <LoaderCircle className="h-4 w-4 animate-spin" /> : <Save className="h-4 w-4" />}
                  {t("Guardar")}
                </Button>
              </div>
              <label className="mt-3 flex items-center gap-2 text-xs text-muted-foreground">
                <input type="checkbox" checked={bedDucking} disabled={running} onChange={(event) => setBedDucking(event.currentTarget.checked)} />
                {t("Bajar automáticamente la cortina cuando el micrófono detecta voz")}
              </label>
              {running ? <p className="mt-2 text-xs text-amber-600">{t("Detén el broadcast para cambiar la cortina; el modo programado puede cambiarse al aire.")}</p> : null}
            </div>
          </div>
        </section>
      </div>
      {editingItem ? (
        <ScheduleBlockEditorModal
          item={editingItem}
          sources={sources}
          onClose={() => setEditingItem(null)}
          onSaved={handleEditedItem}
          onError={onError}
        />
      ) : null}
    </CardContent>
  );
}

function ScheduleBlockEditorModal({
  item,
  sources,
  onClose,
  onSaved,
  onError
}: {
  item: BroadcastScheduleItem;
  sources: BroadcastPlaylistSource[];
  onClose: () => void;
  onSaved: (item: BroadcastScheduleItem, notice?: string) => void;
  onError: (message: string | null) => void;
}) {
  const { locale, t } = useI18n();
  const initialStart = new Date(item.start_at);
  const [date, setDate] = useState(() => localDateInputValue(initialStart));
  const [time, setTime] = useState(() => localTimeInputValue(initialStart));
  const [policy, setPolicy] = useState<"soft" | "exact">(item.policy === "exact" ? "exact" : "soft");
  const [tracks, setTracks] = useState(item.tracks);
  const [sourceKey, setSourceKey] = useState("");
  const [sourceTracks, setSourceTracks] = useState<BroadcastSourceTrack[]>([]);
  const [trackId, setTrackId] = useState("");
  const [loadingTracks, setLoadingTracks] = useState(false);
  const [busy, setBusy] = useState("");
  const [draggedTrackId, setDraggedTrackId] = useState("");
  const [modalError, setModalError] = useState<string | null>(null);
  const liveEditing = item.status === "activated";
  const selectedSource = sources.find((source) => source.key === sourceKey) ?? null;
  const totalDuration = tracks.reduce((total, track) => total + (track.duration_seconds ?? 0), 0);

  function reportError(message: string | null) {
    setModalError(message);
    onError(message);
  }

  useEffect(() => {
    if (!selectedSource) {
      setSourceTracks([]);
      setTrackId("");
      return;
    }
    let disposed = false;
    setLoadingTracks(true);
    void loadBroadcastSourceTracks(selectedSource)
      .then((nextTracks) => {
        if (!disposed) setSourceTracks(nextTracks.filter((track) => track.source_exists && track.source_path));
      })
      .catch((cause) => !disposed && reportError(errorMessage(cause, locale)))
      .finally(() => !disposed && setLoadingTracks(false));
    return () => { disposed = true; };
  }, [locale, selectedSource?.key]);

  useEffect(() => {
    if (!liveEditing) return;
    let disposed = false;
    const refreshPendingTracks = () => {
      void invoke<BroadcastScheduleItem>("broadcast_schedule_item", { itemId: item.id })
        .then((saved) => {
          if (!disposed) setTracks(saved.tracks);
        })
        .catch(() => undefined);
    };
    const timer = window.setInterval(refreshPendingTracks, 2000);
    return () => {
      disposed = true;
      window.clearInterval(timer);
    };
  }, [item.id, liveEditing]);

  async function saveSchedule() {
    const startsAt = new Date(`${date}T${time}:00`);
    if (Number.isNaN(startsAt.getTime())) {
      reportError(t("La fecha y hora programadas no son válidas."));
      return;
    }
    setBusy("schedule");
    reportError(null);
    try {
      const saved = await invoke<BroadcastScheduleItem>("broadcast_update_schedule_item", {
        itemId: item.id,
        schedule: { startAt: startsAt.toISOString(), policy }
      });
      setTracks(saved.tracks);
      await onSaved(saved, t("Horario del bloque actualizado."));
    } catch (cause) {
      reportError(errorMessage(cause, locale));
    } finally {
      setBusy("");
    }
  }

  async function appendContent() {
    if (!selectedSource) return;
    setBusy("append");
    reportError(null);
    try {
      const saved = await invoke<BroadcastScheduleItem>("broadcast_append_schedule_content", {
        itemId: item.id,
        content: {
          sourceKind: trackId ? "track" : selectedSource.kind === "local" ? "draft" : "playlist",
          libraryId: selectedSource.library_id,
          sourceId: trackId || selectedSource.id
        }
      });
      setTracks(saved.tracks);
      setTrackId("");
      await onSaved(saved, t("Contenido agregado al bloque."));
    } catch (cause) {
      reportError(errorMessage(cause, locale));
    } finally {
      setBusy("");
    }
  }

  async function persistTrackOrder(nextTracks: BroadcastScheduleTrack[]) {
    setTracks(nextTracks);
    setBusy("order");
    reportError(null);
    try {
      const saved = await invoke<BroadcastScheduleItem>("broadcast_reorder_schedule_tracks", {
        itemId: item.id,
        trackIds: nextTracks.map((track) => track.id)
      });
      setTracks(saved.tracks);
      await onSaved(saved, t("Orden de pistas actualizado."));
    } catch (cause) {
      setTracks(tracks);
      reportError(errorMessage(cause, locale));
    } finally {
      setBusy("");
    }
  }

  async function moveTrack(index: number, direction: -1 | 1) {
    const target = index + direction;
    if (target < 0 || target >= tracks.length) return;
    const nextTracks = [...tracks];
    [nextTracks[index], nextTracks[target]] = [nextTracks[target], nextTracks[index]];
    await persistTrackOrder(nextTracks);
  }

  async function dropTrack(targetId: string) {
    if (!draggedTrackId || draggedTrackId === targetId) return;
    const nextTracks = [...tracks];
    const sourceIndex = nextTracks.findIndex((track) => track.id === draggedTrackId);
    const targetIndex = nextTracks.findIndex((track) => track.id === targetId);
    if (sourceIndex < 0 || targetIndex < 0) return;
    const [moved] = nextTracks.splice(sourceIndex, 1);
    const adjustedTargetIndex = nextTracks.findIndex((track) => track.id === targetId);
    nextTracks.splice(adjustedTargetIndex, 0, moved);
    setDraggedTrackId("");
    await persistTrackOrder(nextTracks);
  }

  async function removeTrack(track: BroadcastScheduleTrack) {
    setBusy(`remove:${track.id}`);
    reportError(null);
    try {
      const saved = await invoke<BroadcastScheduleItem>("broadcast_remove_schedule_track", {
        itemId: item.id,
        trackId: track.id
      });
      setTracks(saved.tracks);
      await onSaved(saved, t("Pista quitada del bloque."));
    } catch (cause) {
      reportError(errorMessage(cause, locale));
    } finally {
      setBusy("");
    }
  }

  return (
    <div
      className="fixed inset-0 z-[100] grid place-items-center bg-black/70 p-3 backdrop-blur-sm"
      role="presentation"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <section
        role="dialog"
        aria-modal="true"
        aria-labelledby="schedule-block-editor-title"
        className="flex max-h-[min(860px,calc(100vh-1.5rem))] w-full max-w-4xl flex-col overflow-hidden rounded-xl border border-border bg-background shadow-2xl"
      >
        <header className="flex shrink-0 items-start justify-between gap-3 border-b border-border px-4 py-3">
          <div className="min-w-0">
            <h2 id="schedule-block-editor-title" className="truncate text-base font-semibold">
              {t(liveEditing ? "Editar bloque en vivo" : "Editar bloque de parrilla")}
            </h2>
            <p className="mt-1 text-xs text-muted-foreground">
              {tracks.length} {t(liveEditing ? "tracks pendientes" : "tracks")} · {formatDuration(totalDuration)} · {item.source_name}
            </p>
          </div>
          <Button size="sm" variant="ghost" onClick={onClose}>{t("Cerrar")}</Button>
        </header>

        <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain p-4">
          <div className="grid gap-4">
            {modalError ? (
              <div role="alert" className="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
                {modalError}
              </div>
            ) : null}
            {liveEditing ? (
              <div className="flex items-start gap-2 rounded-md border border-emerald-500/30 bg-emerald-500/10 px-3 py-2 text-sm">
                <Radio className="mt-0.5 h-4 w-4 shrink-0 text-emerald-600" />
                <div>
                  <strong className="text-foreground">{t("Edición en vivo")}</strong>
                  <p className="mt-0.5 text-xs leading-relaxed text-muted-foreground">
                    {t("Sólo se muestran las pistas que aún no comenzaron. Puedes reordenarlas, quitarlas o agregar más contenido; la pista al aire no será interrumpida.")}
                  </p>
                </div>
              </div>
            ) : null}
            <section className="rounded-lg border border-border bg-secondary/30 p-3">
              <div className="flex flex-wrap items-end gap-2">
                <label className="grid gap-1 text-xs font-semibold">
                  {t("Fecha")}
                  <input className={cn(fieldClass, "h-9 w-auto")} type="date" value={date} disabled={liveEditing} onChange={(event) => setDate(event.currentTarget.value)} />
                </label>
                <label className="grid gap-1 text-xs font-semibold">
                  {t("Hora")}
                  <input className={cn(fieldClass, "h-9 w-auto")} type="time" value={time} disabled={liveEditing} onChange={(event) => setTime(event.currentTarget.value)} />
                </label>
                <div className="grid h-9 grid-cols-2 gap-1 rounded-md border border-border bg-secondary p-1">
                  <Button size="sm" disabled={liveEditing} variant={policy === "soft" ? "default" : "ghost"} onClick={() => setPolicy("soft")}>{t("Al terminar")}</Button>
                  <Button size="sm" disabled={liveEditing} variant={policy === "exact" ? "default" : "ghost"} onClick={() => setPolicy("exact")}>{t("Hora exacta")}</Button>
                </div>
                {liveEditing ? (
                  <span className="ml-auto rounded-md border border-border bg-background px-3 py-2 text-xs font-medium text-muted-foreground">
                    {t("El horario ya comenzó")}
                  </span>
                ) : (
                  <Button className="ml-auto" disabled={Boolean(busy)} onClick={() => void saveSchedule()}>
                    {busy === "schedule" ? <LoaderCircle className="h-4 w-4 animate-spin" /> : <Save className="h-4 w-4" />}
                    {t("Guardar horario")}
                  </Button>
                )}
              </div>
            </section>

            <section className="overflow-hidden rounded-lg border border-border">
              <header className="flex items-center justify-between gap-3 border-b border-border bg-secondary/60 px-3 py-2">
                <div>
                  <h3 className="text-sm font-semibold">{t(liveEditing ? "Próximas pistas" : "Orden de pistas")}</h3>
                  <p className="text-xs text-muted-foreground">
                    {t(liveEditing ? "Los cambios se aplican directamente a la cola al aire." : "Las pistas se emitirán de arriba hacia abajo.")}
                  </p>
                </div>
                {busy === "order" ? <LoaderCircle className="h-4 w-4 animate-spin text-muted-foreground" /> : null}
              </header>
              <div className="max-h-[360px] divide-y divide-border overflow-y-auto overscroll-contain">
                {tracks.length === 0 ? (
                  <div className="grid min-h-24 place-items-center px-4 py-6 text-center text-sm text-muted-foreground">
                    {t(liveEditing ? "No quedan pistas pendientes. Puedes agregar más contenido abajo." : "El bloque no tiene pistas.")}
                  </div>
                ) : tracks.map((track, index) => (
                  <div
                    key={track.id}
                    className={cn(
                      "grid grid-cols-[48px_minmax(0,1fr)_auto] items-center gap-2 px-3 py-2 transition-colors",
                      draggedTrackId && draggedTrackId !== track.id && "hover:bg-accent/60"
                    )}
                    onDragOver={(event) => {
                      if (!draggedTrackId || busy) return;
                      event.preventDefault();
                      event.dataTransfer.dropEffect = "move";
                    }}
                    onDrop={(event) => {
                      event.preventDefault();
                      void dropTrack(track.id);
                    }}
                  >
                    <button
                      type="button"
                      draggable={!busy}
                      className="flex cursor-grab items-center gap-1 rounded px-1 py-2 font-mono text-xs text-muted-foreground hover:bg-accent hover:text-foreground active:cursor-grabbing"
                      aria-label={t("Arrastrar para reordenar")}
                      onDragStart={(event) => {
                        setDraggedTrackId(track.id);
                        event.dataTransfer.effectAllowed = "move";
                        event.dataTransfer.setData("text/plain", track.id);
                      }}
                      onDragEnd={() => setDraggedTrackId("")}
                    >
                      <GripVertical className="h-3.5 w-3.5" />
                      {String(index + 1).padStart(2, "0")}
                    </button>
                    <div className="min-w-0">
                      <strong className="block truncate text-sm">{track.title}</strong>
                      <span className="block truncate text-xs text-muted-foreground">
                        {track.artist?.trim() || t("Sin artista")} · {formatDuration(track.duration_seconds)}
                      </span>
                    </div>
                    <div className="flex items-center gap-0.5">
                      <Button size="icon" variant="ghost" disabled={Boolean(busy) || index === 0} aria-label={t("Mover hacia arriba")} onClick={() => void moveTrack(index, -1)}>
                        <ArrowUp className="h-3.5 w-3.5" />
                      </Button>
                      <Button size="icon" variant="ghost" disabled={Boolean(busy) || index === tracks.length - 1} aria-label={t("Mover hacia abajo")} onClick={() => void moveTrack(index, 1)}>
                        <ArrowDown className="h-3.5 w-3.5" />
                      </Button>
                      <Button size="icon" variant="ghost" disabled={Boolean(busy) || (!liveEditing && tracks.length <= 1)} aria-label={t("Quitar del bloque")} onClick={() => void removeTrack(track)}>
                        {busy === `remove:${track.id}` ? <LoaderCircle className="h-4 w-4 animate-spin" /> : <Trash2 className="h-4 w-4" />}
                      </Button>
                    </div>
                  </div>
                ))}
              </div>
            </section>

            <section className="rounded-lg border border-border bg-secondary/30 p-3">
              <div>
                <h3 className="text-sm font-semibold">{t("Agregar contenido")}</h3>
                <p className="mt-1 text-xs text-muted-foreground">
                  {t(liveEditing ? "Lo nuevo se agregará al final de la cola activa." : "Anexa una playlist completa o una pista al final del bloque.")}
                </p>
              </div>
              <div className="mt-3 grid gap-2 md:grid-cols-[minmax(220px,1fr)_minmax(220px,1fr)_auto]">
                <BroadcastCombobox
                  value={sourceKey}
                  placeholder={t("Selecciona una playlist")}
                  searchPlaceholder={t("Buscar por nombre, biblioteca u origen...")}
                  options={sources.map((source) => ({
                    value: source.key,
                    label: source.name,
                    detail: `${source.library_name} · ${source.track_count} ${t("tracks")}`,
                    keywords: `${source.kind} ${source.library_name}`
                  }))}
                  onChange={(value) => {
                    setSourceKey(value);
                    setTrackId("");
                  }}
                />
                <BroadcastCombobox
                  value={trackId}
                  disabled={!selectedSource || loadingTracks}
                  placeholder={loadingTracks ? t("Cargando tracks...") : t("Playlist completa")}
                  searchPlaceholder={t("Buscar un track...")}
                  emptyLabel={t("No se encontraron tracks.")}
                  options={[
                    { value: "", label: t("Playlist completa"), detail: selectedSource ? `${selectedSource.track_count} ${t("tracks")}` : undefined },
                    ...sourceTracks.map((track) => ({
                      value: track.track_id,
                      label: track.name?.trim() || t("Sin titulo"),
                      detail: track.artist?.trim() || formatDuration(track.total_time),
                      keywords: track.artist ?? ""
                    }))
                  ]}
                  onChange={setTrackId}
                />
                <Button disabled={!selectedSource || Boolean(busy)} onClick={() => void appendContent()}>
                  {busy === "append" ? <LoaderCircle className="h-4 w-4 animate-spin" /> : <Plus className="h-4 w-4" />}
                  {trackId ? t("Agregar pista") : t("Agregar playlist")}
                </Button>
              </div>
            </section>
          </div>
        </div>
      </section>
    </div>
  );
}

async function loadBroadcastSourceTracks(source: BroadcastPlaylistSource): Promise<BroadcastSourceTrack[]> {
  return source.kind === "local"
    ? invoke<BroadcastSourceTrack[]>("playlist_index_draft_tracks", { draftId: source.id })
    : invoke<BroadcastSourceTrack[]>("playlist_index_playlist_tracks", {
        libraryId: source.library_id,
        playlistPath: source.id
      });
}

function localDateInputValue(date: Date) {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

function localTimeInputValue(date: Date) {
  return `${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`;
}

function nextScheduleTime() {
  const date = new Date(Date.now() + 15 * 60 * 1000);
  return `${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`;
}

function formatScheduleClock(startAt: string) {
  return new Date(startAt).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

function formatScheduleDay(value: string, locale: string) {
  const date = new Date(`${value}T12:00:00`);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleDateString(locale === "en" ? "en-US" : "es-CL", {
    weekday: "long",
    day: "numeric",
    month: "long"
  });
}

type ScheduleOverlap = {
  next: BroadcastScheduleItem;
  windowSeconds: number;
  overflowSeconds: number;
};

function scheduleOverlapInfo(items: BroadcastScheduleItem[]) {
  const pending = items.filter((item) => item.status === "pending").sort((left, right) => left.start_at.localeCompare(right.start_at));
  const overlaps = new Map<string, ScheduleOverlap>();
  for (let index = 0; index < pending.length - 1; index += 1) {
    const item = pending[index];
    const next = pending[index + 1];
    if (!item.duration_seconds) continue;
    const windowSeconds = Math.max(
      0,
      Math.floor((new Date(next.start_at).getTime() - new Date(item.start_at).getTime()) / 1000)
    );
    const overflowSeconds = item.duration_seconds - windowSeconds;
    if (overflowSeconds > 0) {
      overlaps.set(item.id, { next, windowSeconds, overflowSeconds });
    }
  }
  return overlaps;
}

function ScheduleStatus({ status }: { status: string }) {
  const { t } = useI18n();
  const style = status === "pending"
    ? "bg-blue-500/10 text-blue-700 dark:text-blue-300"
    : status === "activated"
      ? "bg-emerald-500/10 text-emerald-700 dark:text-emerald-300"
      : status === "failed"
        ? "bg-red-500/10 text-red-700 dark:text-red-300"
        : "bg-muted text-muted-foreground";
  const label = status === "pending" ? "Pendiente" : status === "activated" ? "Activado" : status === "failed" ? "Fallido" : "Omitido";
  return <span className={cn("rounded-full px-2 py-0.5 text-[10px] font-semibold uppercase", style)}>{t(label)}</span>;
}

function BroadcastCombobox({
  value,
  options,
  placeholder,
  searchPlaceholder,
  emptyLabel,
  disabled = false,
  onChange
}: {
  value: string;
  options: Array<{ value: string; label: string; detail?: string; keywords?: string }>;
  placeholder: string;
  searchPlaceholder: string;
  emptyLabel?: string;
  disabled?: boolean;
  onChange: (value: string) => void;
}) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const selected = options.find((option) => option.value === value) ?? null;

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button
          type="button"
          variant="secondary"
          role="combobox"
          aria-expanded={open}
          disabled={disabled}
          className="h-10 w-full min-w-0 justify-between overflow-hidden border border-input bg-background px-3 font-normal hover:bg-accent"
        >
          {selected ? (
            <span className="min-w-0 flex-1 truncate text-left">
              <span className="font-medium">{selected.label}</span>
              {selected.detail ? <span className="text-muted-foreground"> · {selected.detail}</span> : null}
            </span>
          ) : <span className="truncate text-muted-foreground">{placeholder}</span>}
          <ChevronsUpDown className="ml-2 h-4 w-4 shrink-0 text-muted-foreground" />
        </Button>
      </PopoverTrigger>
      <PopoverContent align="start" className="z-[120] w-[var(--radix-popover-trigger-width)] max-w-[calc(100vw-2rem)] p-0">
        <Command>
          <CommandInput placeholder={searchPlaceholder} />
          <CommandList className="max-h-64 overscroll-contain">
            <CommandEmpty>{emptyLabel ?? t("No se encontraron opciones.")}</CommandEmpty>
            {options.map((option) => (
              <CommandItem
                key={`${option.value}:${option.label}`}
                value={`${option.value || "all"} ${option.label} ${option.detail ?? ""} ${option.keywords ?? ""}`}
                onSelect={() => {
                  onChange(option.value);
                  setOpen(false);
                }}
              >
                <Check className={cn("mr-2 h-4 w-4 shrink-0", option.value === value ? "opacity-100" : "opacity-0")} />
                <span className="min-w-0 flex-1">
                  <strong className="block truncate font-medium">{option.label}</strong>
                  {option.detail ? <span className="block truncate text-xs text-muted-foreground">{option.detail}</span> : null}
                </span>
              </CommandItem>
            ))}
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  );
}

function VideoStudioModal({
  open,
  config,
  devices,
  stationName,
  trackTitle,
  running,
  cameraReady,
  mixPercent,
  busy,
  onClose,
  onChange,
  onMix,
  onSave
}: {
  open: boolean;
  config: BroadcastVideoCompositor;
  devices: BroadcastCameraDevice[];
  stationName: string;
  trackTitle: string;
  running: boolean;
  cameraReady: boolean;
  mixPercent: number;
  busy: BusyAction;
  onClose: () => void;
  onChange: (config: BroadcastVideoCompositor) => void;
  onMix: (mixPercent: number, transitionMillis: number) => Promise<void>;
  onSave: () => Promise<void>;
}) {
  const { locale, t } = useI18n();
  const cameraVideo = useRef<HTMLVideoElement | null>(null);
  const screenVideo = useRef<HTMLVideoElement | null>(null);
  const previewCanvas = useRef<HTMLCanvasElement | null>(null);
  const programCanvas = useRef<HTMLCanvasElement | null>(null);
  const cameraStream = useRef<MediaStream | null>(null);
  const screenStream = useRef<MediaStream | null>(null);
  const offscreenCanvas = useRef<HTMLCanvasElement | null>(null);
  const uploadPending = useRef(false);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [draftMix, setDraftMix] = useState(mixPercent);
  const [handoffPending, setHandoffPending] = useState(false);
  const [activeLayer, setActiveLayer] = useState<"camera" | "screen">("camera");
  const [gestureConfig, setGestureConfig] = useState<BroadcastVideoCompositor | null>(null);
  const [cameraAvailable, setCameraAvailable] = useState(false);
  const [screenAvailable, setScreenAvailable] = useState(false);
  const [browserCameras, setBrowserCameras] = useState<BroadcastCameraDevice[]>([]);
  const cameraDevices = [...devices.filter((device) => device.kind === "camera"), ...browserCameras]
    .filter((device, index, all) => all.findIndex((candidate) => candidate.label === device.label) === index);
  const studioConfig = gestureConfig ?? config;
  const studioConfigRef = useRef(studioConfig);
  studioConfigRef.current = studioConfig;

  const update = useCallback((patch: Partial<BroadcastVideoCompositor>) => {
    onChange({ ...config, ...patch, captureMode: "browser" });
  }, [config, onChange]);

  const stopCamera = useCallback(() => {
    cameraStream.current?.getTracks().forEach((track) => track.stop());
    cameraStream.current = null;
    if (cameraVideo.current) cameraVideo.current.srcObject = null;
    setCameraAvailable(false);
  }, []);

  const stopScreen = useCallback(() => {
    screenStream.current?.getTracks().forEach((track) => track.stop());
    screenStream.current = null;
    if (screenVideo.current) screenVideo.current.srcObject = null;
    setScreenAvailable(false);
  }, []);

  const startCamera = useCallback(async (preferredLabel = config.cameraDevice) => {
    try {
      setPreviewError(null);
      stopCamera();
      let stream = await navigator.mediaDevices.getUserMedia({
        audio: false,
        video: { width: { ideal: 1280 }, height: { ideal: 720 }, frameRate: { ideal: 30, max: 30 } }
      });
      const enumerated = await navigator.mediaDevices.enumerateDevices();
      const cameras = enumerated
        .filter((device) => device.kind === "videoinput")
        .map((device, index) => ({
          id: device.label || device.deviceId || `camera-${index + 1}`,
          label: device.label || `${t("Cámara")} ${index + 1}`,
          kind: "camera"
        }));
      setBrowserCameras(cameras);
      const preferred = cameras.find((device) => device.label === preferredLabel || device.id === preferredLabel);
      const browserDevice = enumerated.find((device) => device.kind === "videoinput" && (
        device.label === preferred?.label || device.deviceId === preferred?.id
      ));
      if (browserDevice && stream.getVideoTracks()[0]?.label !== browserDevice.label) {
        stream.getTracks().forEach((track) => track.stop());
        stream = await navigator.mediaDevices.getUserMedia({
          audio: false,
          video: {
            deviceId: { exact: browserDevice.deviceId },
            width: { ideal: 1280 },
            height: { ideal: 720 },
            frameRate: { ideal: 30, max: 30 }
          }
        });
      }
      cameraStream.current = stream;
      if (cameraVideo.current) {
        cameraVideo.current.srcObject = stream;
        await cameraVideo.current.play().catch(() => undefined);
      }
      setCameraAvailable(true);
      const selectedLabel = stream.getVideoTracks()[0]?.label || preferredLabel || "default";
      if (config.captureMode !== "browser") {
        update({ captureMode: "browser", cameraEnabled: true, cameraDevice: selectedLabel, cameraRotationDegrees: 0 });
      }
      return selectedLabel;
    } catch (cause) {
      setPreviewError(cause instanceof Error ? cause.message : String(cause));
      return null;
    }
  }, [config.cameraDevice, config.captureMode, stopCamera, t, update]);

  const chooseScreenOrWindow = useCallback(async () => {
    try {
      setPreviewError(null);
      stopScreen();
      const stream = await navigator.mediaDevices.getDisplayMedia({
        audio: false,
        video: { frameRate: { ideal: 30, max: 30 } }
      });
      const track = stream.getVideoTracks()[0];
      if (!track) throw new Error(t("No se recibió video de la pantalla o ventana seleccionada."));
      screenStream.current = stream;
      track.addEventListener("ended", () => {
        screenStream.current = null;
        setScreenAvailable(false);
      }, { once: true });
      if (screenVideo.current) {
        screenVideo.current.srcObject = stream;
        await screenVideo.current.play().catch(() => undefined);
      }
      const surface = track.getSettings().displaySurface;
      const surfaceLabel = surface === "window" ? t("Ventana") : surface === "monitor" ? t("Pantalla") : t("Pantalla o ventana");
      const label = track.label ? `${surfaceLabel} · ${track.label}` : surfaceLabel;
      setScreenAvailable(true);
      update({ screenEnabled: true, screenLabel: label });
    } catch (cause) {
      setPreviewError(cause instanceof Error ? cause.message : String(cause));
    }
  }, [stopScreen, t, update]);

  useEffect(() => setDraftMix(mixPercent), [mixPercent]);

  useEffect(() => {
    if (open && config.enabled && config.cameraEnabled && !cameraStream.current) {
      void startCamera();
    }
  }, [config.cameraEnabled, config.enabled, open, startCamera]);

  useEffect(() => {
    if (!config.cameraEnabled) stopCamera();
  }, [config.cameraEnabled, stopCamera]);

  useEffect(() => {
    if (!config.screenEnabled) stopScreen();
  }, [config.screenEnabled, stopScreen]);

  useEffect(() => () => {
    cameraStream.current?.getTracks().forEach((track) => track.stop());
    screenStream.current?.getTracks().forEach((track) => track.stop());
  }, []);

  useEffect(() => {
    if (!offscreenCanvas.current) {
      offscreenCanvas.current = document.createElement("canvas");
      offscreenCanvas.current.width = 360;
      offscreenCanvas.current.height = 640;
    }
    let frameRequest = 0;
    let lastUploadAt = 0;
    let renderErrorReported = false;
    const render = (now: number) => {
      try {
        const canvas = offscreenCanvas.current;
        const context = canvas?.getContext("2d");
        if (canvas && context) {
          context.clearRect(0, 0, canvas.width, canvas.height);
          const currentConfig = studioConfigRef.current;
          const layers = [
            currentConfig.screenEnabled && screenAvailable && screenVideo.current
              ? { key: "screen" as const, video: screenVideo.current, config: visualLayerConfig(currentConfig, "screen") }
              : null,
            currentConfig.cameraEnabled && cameraAvailable && cameraVideo.current
              ? { key: "camera" as const, video: cameraVideo.current, config: visualLayerConfig(currentConfig, "camera") }
              : null
          ].filter((layer): layer is NonNullable<typeof layer> => Boolean(layer))
            .sort((left, right) => left.config.zIndex - right.config.zIndex);
          for (const layer of layers) drawVisualLayer(context, layer.video, layer.config);
          for (const monitor of [previewCanvas.current, programCanvas.current]) {
            const monitorContext = monitor?.getContext("2d");
            if (!monitor || !monitorContext) continue;
            monitorContext.clearRect(0, 0, monitor.width, monitor.height);
            monitorContext.drawImage(canvas, 0, 0);
          }
          if (running && currentConfig.enabled && layers.length > 0 && now - lastUploadAt >= 1000 / 24 && !uploadPending.current) {
            lastUploadAt = now;
            uploadPending.current = true;
            const pixels = context.getImageData(0, 0, canvas.width, canvas.height).data;
            void invoke("broadcast_push_visual_frame", {
              frameBase64: bytesToBase64(pixels)
            })
              .catch((cause) => setPreviewError(errorMessage(cause, locale)))
              .finally(() => { uploadPending.current = false; });
          }
        }
      } catch (cause) {
        uploadPending.current = false;
        if (!renderErrorReported) {
          renderErrorReported = true;
          setPreviewError(errorMessage(cause, locale));
        }
      } finally {
        frameRequest = requestAnimationFrame(render);
      }
    };
    frameRequest = requestAnimationFrame(render);
    return () => cancelAnimationFrame(frameRequest);
  }, [cameraAvailable, locale, running, screenAvailable]);

  const faderEnabled = config.enabled;
  const take = async (nextMix: number, transitionMillis: number) => {
    if (handoffPending || busy === "camera-mix") return;
    setDraftMix(nextMix);
    setHandoffPending(true);
    try {
      await onMix(nextMix, transitionMillis);
    } finally {
      setHandoffPending(false);
    }
  };

  const layer = visualLayerConfig(studioConfig, activeLayer);
  const changeLayerRect = useCallback((source: "camera" | "screen", rect: VisualLayerRect, commit: boolean) => {
    const next = applyVisualLayerRect(config, source, rect);
    setGestureConfig(next);
    if (commit) {
      onChange({ ...next, captureMode: "browser" });
      setGestureConfig(null);
    }
  }, [config, onChange]);
  const updateLayer = (patch: Partial<VisualLayerConfig>) => {
    if (activeLayer === "camera") {
      update({
        cameraLayout: patch.layout ?? config.cameraLayout,
        cameraPosition: patch.position ?? config.cameraPosition,
        cameraSize: patch.size ?? config.cameraSize,
        cameraEffect: patch.effect ?? config.cameraEffect,
        cameraMirror: patch.mirror ?? config.cameraMirror,
        cameraRotationDegrees: patch.rotationDegrees ?? config.cameraRotationDegrees,
        cameraFraming: patch.framing ?? config.cameraFraming,
        cameraOpacityPercent: patch.opacityPercent ?? config.cameraOpacityPercent
      });
    } else {
      update({
        screenLayout: patch.layout ?? config.screenLayout,
        screenPosition: patch.position ?? config.screenPosition,
        screenSize: patch.size ?? config.screenSize,
        screenEffect: patch.effect ?? config.screenEffect,
        screenMirror: patch.mirror ?? config.screenMirror,
        screenRotationDegrees: patch.rotationDegrees ?? config.screenRotationDegrees,
        screenFraming: patch.framing ?? config.screenFraming,
        screenOpacityPercent: patch.opacityPercent ?? config.screenOpacityPercent
      });
    }
  };
  const activeZIndex = activeLayer === "camera" ? studioConfig.cameraZIndex : studioConfig.screenZIndex;
  const otherZIndex = activeLayer === "camera" ? studioConfig.screenZIndex : studioConfig.cameraZIndex;
  const setLayerDepth = (front: boolean) => {
    const foreground = front ? 2 : 1;
    const background = front ? 1 : 2;
    update(activeLayer === "camera"
      ? { cameraZIndex: foreground, screenZIndex: background }
      : { screenZIndex: foreground, cameraZIndex: background });
  };

  const captureElements = (
    <div className="pointer-events-none fixed -left-4 top-0 h-px w-px overflow-hidden opacity-0" aria-hidden="true">
      <video ref={screenVideo} muted playsInline />
      <video ref={cameraVideo} muted playsInline />
    </div>
  );

  if (!open) return <>{captureElements}</>;

  return (
    <>{captureElements}<div className="fixed inset-0 z-[80] flex items-center justify-center p-3" role="dialog" aria-modal="true" aria-labelledby="video-studio-title">
      <div className="absolute inset-0 bg-black/70 backdrop-blur-sm" onClick={onClose} />
      <section className="relative z-[85] flex max-h-[94vh] w-full max-w-6xl flex-col overflow-hidden rounded-xl border border-white/15 bg-[#090b0a] text-white shadow-2xl">
        <header className="flex items-start justify-between gap-4 border-b border-white/10 px-5 py-4">
          <div>
            <div className="flex items-center gap-2 text-[11px] font-semibold uppercase tracking-[0.22em] text-white/45">
              <SlidersHorizontal className="h-4 w-4" /> Rau Broadcast System
            </div>
            <h2 id="video-studio-title" className="mt-1 text-xl font-semibold">{t("Video Studio · Preview / Program")}</h2>
            <p className="mt-1 text-xs text-white/50">{t("Prepara la fuente y usa el fader para enviarla sin reiniciar RTMP.")}</p>
          </div>
          <Button type="button" size="sm" variant="secondary" onClick={onClose}>{t("Cerrar")}</Button>
        </header>

        <div className="grid min-h-0 flex-1 overflow-y-auto lg:grid-cols-[minmax(0,1fr)_310px]">
          <div className="grid gap-4 p-4 sm:grid-cols-2">
            <StudioMonitor
              label="PREVIEW"
              stationName={stationName}
              trackTitle={trackTitle}
              visualVisible={config.enabled}
              canvasRef={previewCanvas}
              visualConfig={studioConfig}
              interactive
              selectedLayer={activeLayer}
              onSelectLayer={setActiveLayer}
              onLayerRectChange={changeLayerRect}
              visualAvailable={(config.cameraEnabled && cameraAvailable) || (config.screenEnabled && screenAvailable)}
              visualPlaceholder={previewError ? t("Vista previa no disponible") : t("Activa una cámara o elige una pantalla o ventana")}
            />
            <StudioMonitor
              label="PROGRAM"
              stationName={stationName}
              trackTitle={trackTitle}
              visualVisible={config.enabled && draftMix > 0}
              visualOpacity={draftMix / 100}
              canvasRef={programCanvas}
              visualConfig={studioConfig}
              visualAvailable={(config.cameraEnabled && cameraAvailable) || (config.screenEnabled && screenAvailable)}
              visualPlaceholder={t("Fuentes en Program")}
              transitionMillis={config.transitionMillis}
            />

            <div className="rounded-lg border border-white/10 bg-white/[0.035] p-4 sm:col-span-2">
              <div className="flex items-center justify-between gap-3 text-xs font-semibold uppercase tracking-[0.16em] text-white/55">
                <span>GRAPHIC</span><span>{draftMix}%</span><span>VISUAL LAYERS</span>
              </div>
              <input
                aria-label={t("Fader Preview a Program")}
                className="mt-3 h-3 w-full cursor-ew-resize accent-white disabled:cursor-not-allowed disabled:opacity-35"
                type="range"
                min={0}
                max={100}
                step={1}
                value={draftMix}
                disabled={!faderEnabled || busy === "camera-mix" || handoffPending}
                onChange={(event) => setDraftMix(Number(event.currentTarget.value))}
                onPointerUp={(event) => void take(Number(event.currentTarget.value), 0)}
                onKeyUp={(event) => void take(Number(event.currentTarget.value), 0)}
              />
              <div className="mt-4 flex flex-wrap items-center justify-between gap-3">
                <span className="text-xs text-white/45">
                  {running
                    ? cameraReady ? t("El fader controla la señal que recibe Instagram.") : t("Esperando que el compositor quede listo...")
                    : t("El fader define la posición inicial de Program para la próxima transmisión.")}
                </span>
                <Button
                  type="button"
                  disabled={!faderEnabled || busy === "camera-mix" || handoffPending}
                  onClick={() => void take(draftMix > 0 ? 0 : 100, config.transitionMillis)}
                >
                  {busy === "camera-mix" || handoffPending ? <LoaderCircle className="h-4 w-4 animate-spin" /> : <Play className="h-4 w-4" />}
                  {draftMix > 0 ? t("Volver a gráfica") : t("AUTO · Enviar a Program")}
                </Button>
              </div>
            </div>
          </div>

          <aside className="grid content-start gap-4 border-t border-white/10 bg-black/20 p-4 lg:border-l lg:border-t-0">
            <div>
              <div className="mb-2 flex items-center justify-between gap-2">
                <strong className="text-sm">{t("Plantilla de presentación")}</strong>
                <span className="font-mono text-[9px] uppercase tracking-[0.18em] text-white/35">PROGRAM</span>
              </div>
              <div className="grid gap-2">
                {broadcastGraphicTemplates.map((template) => {
                  const selected = config.graphicTemplate === template.id;
                  return (
                    <button
                      key={template.id}
                      type="button"
                      disabled={running}
                      className={cn(
                        "flex items-center gap-3 rounded-md border p-2 text-left transition disabled:cursor-not-allowed disabled:opacity-55",
                        selected ? "border-lime-300/70 bg-lime-300/10" : "border-white/10 bg-white/[.025] hover:border-white/25"
                      )}
                      onClick={() => update({ graphicTemplate: template.id })}
                    >
                      <span className="h-10 w-14 shrink-0 border border-black/40" style={{ background: template.swatch }} />
                      <span className="min-w-0">
                        <span className="block text-xs font-semibold">{template.name}</span>
                        <span className="block truncate text-[10px] text-white/40">{t(template.description)}</span>
                      </span>
                      {selected ? <Check className="ml-auto h-4 w-4 shrink-0 text-lime-300" /> : null}
                    </button>
                  );
                })}
              </div>
              <span className="mt-2 block text-[10px] leading-relaxed text-white/35">
                {running ? t("La plantilla queda fija durante el Live para mantener estable RTMP.") : t("La plantilla cambia Preview y la próxima señal RTMP.")}
              </span>
            </div>

            <div className="flex items-center justify-between gap-3">
              <div>
                <strong className="text-sm">{t("Fuente visual")}</strong>
                <span className="block text-xs text-white/40">{config.enabled ? running ? t("Capturando · fuera de Program") : t("Preparada · inicia fuera de Program") : t("Desactivada")}</span>
              </div>
              <label className="flex items-center gap-2 text-xs font-semibold">
                <input
                  type="checkbox"
                  checked={config.enabled}
                  disabled={running}
                  onChange={(event) => update({
                    enabled: event.currentTarget.checked,
                    captureMode: "browser",
                    cameraDevice: config.cameraDevice || "default"
                  })}
                />
                {t("Usar fuente")}
              </label>
            </div>

            <div className="grid grid-cols-2 gap-2">
              <button type="button" className={cn("rounded-md border p-3 text-left", activeLayer === "camera" ? "border-emerald-400/60 bg-emerald-400/10" : "border-white/10 bg-white/[.03]")} onClick={() => setActiveLayer("camera")}>
                <span className="flex items-center gap-2 text-xs font-semibold"><Camera className="h-4 w-4" />{t("Cámara")}</span>
                <span className="mt-1 block text-[10px] text-white/40">{cameraAvailable ? t("Activa") : t("Sin señal")}</span>
              </button>
              <button type="button" className={cn("rounded-md border p-3 text-left", activeLayer === "screen" ? "border-emerald-400/60 bg-emerald-400/10" : "border-white/10 bg-white/[.03]")} onClick={() => setActiveLayer("screen")}>
                <span className="flex items-center gap-2 text-xs font-semibold"><Monitor className="h-4 w-4" />{t("Pantalla / ventana")}</span>
                <span className="mt-1 block truncate text-[10px] text-white/40">{screenAvailable ? config.screenLabel : t("Sin señal")}</span>
              </button>
            </div>

            {activeLayer === "camera" ? <Field label={t("Cámara")}>
              <div className="flex gap-2">
                <select
                  className={cn(fieldClass, "border-white/15 bg-white/5 text-white")}
                  value={config.cameraDevice}
                  disabled={!config.enabled}
                  onChange={(event) => {
                    const cameraDevice = event.currentTarget.value;
                    update({ cameraDevice, cameraEnabled: true, cameraRotationDegrees: 0 });
                    void startCamera(cameraDevice);
                  }}
                >
                  <option value="default">{t("Cámara predeterminada")}</option>
                  {cameraDevices.map((device) => <option key={`${device.kind}:${device.id}`} value={device.label}>{device.label}</option>)}
                </select>
                <Button type="button" size="icon" variant="secondary" onClick={() => void startCamera()} aria-label={t("Refrescar fuentes") }>
                  <RefreshCcw className="h-4 w-4" />
                </Button>
              </div>
              <label className="mt-2 flex items-center gap-2 text-xs text-white/65">
                <input type="checkbox" checked={config.cameraEnabled} disabled={!config.enabled} onChange={(event) => {
                  if (event.currentTarget.checked) {
                    update({ cameraEnabled: true, cameraRotationDegrees: config.captureMode === "native" ? 0 : config.cameraRotationDegrees });
                    void startCamera();
                  } else {
                    stopCamera();
                    update({ cameraEnabled: false });
                  }
                }} />
                {t("Cámara activa")}
              </label>
            </Field> : <Field label={t("Pantalla o ventana")}>
              <Button type="button" variant="secondary" disabled={!config.enabled} onClick={() => void chooseScreenOrWindow()}>
                <Monitor className="h-4 w-4" />{screenAvailable ? t("Cambiar pantalla o ventana") : t("Elegir pantalla o ventana")}
              </Button>
              <label className="mt-2 flex items-center gap-2 text-xs text-white/65">
                <input type="checkbox" checked={config.screenEnabled} disabled={!config.enabled} onChange={(event) => {
                  if (event.currentTarget.checked) void chooseScreenOrWindow();
                  else {
                    stopScreen();
                    update({ screenEnabled: false });
                  }
                }} />
                {t("Pantalla o ventana activa")}
              </label>
              <span className="mt-2 block text-[11px] leading-relaxed text-white/40">{t("El selector del sistema permite compartir una pantalla completa o una ventana de aplicación.")}</span>
            </Field>}

            <Field label={t("Composición") }>
              <select className={cn(fieldClass, "border-white/15 bg-white/5 text-white")} value={layer.layout} disabled={!config.enabled} onChange={(event) => updateLayer({ layout: event.currentTarget.value })}>
                <option value="card">{t("Tarjeta")}</option>
                <option value="wide">{t("Ancho completo")}</option>
                <option value="background">{t("Fondo")}</option>
                <option value="free">{t("Libre · mover en Preview")}</option>
              </select>
            </Field>

            <div className="flex items-center justify-between gap-2 rounded-md border border-white/10 bg-white/[.03] p-2">
              <span className="text-[11px] text-white/45">{t("Orden de capa")} · Z{activeZIndex}</span>
              <div className="flex gap-1">
                <Button type="button" size="sm" variant="secondary" disabled={activeZIndex > otherZIndex} onClick={() => setLayerDepth(true)}>
                  <ArrowUp className="h-3.5 w-3.5" />{t("Al frente")}
                </Button>
                <Button type="button" size="sm" variant="secondary" disabled={activeZIndex < otherZIndex} onClick={() => setLayerDepth(false)}>
                  <ArrowDown className="h-3.5 w-3.5" />{t("Al fondo")}
                </Button>
              </div>
            </div>

            <div className="grid grid-cols-2 gap-3">
              <Field label={t("Posición") }>
                <select className={cn(fieldClass, "border-white/15 bg-white/5 text-white")} value={layer.position} disabled={!config.enabled || layer.layout !== "card"} onChange={(event) => updateLayer({ position: event.currentTarget.value })}>
                  <option value="top_left">{t("Arriba izquierda")}</option>
                  <option value="top_right">{t("Arriba derecha")}</option>
                  <option value="center">{t("Centro")}</option>
                  <option value="bottom_left">{t("Abajo izquierda")}</option>
                  <option value="bottom_right">{t("Abajo derecha")}</option>
                </select>
              </Field>
              <Field label={t("Tamaño") }>
                <select className={cn(fieldClass, "border-white/15 bg-white/5 text-white")} value={layer.size} disabled={!config.enabled || layer.layout !== "card"} onChange={(event) => updateLayer({ size: event.currentTarget.value })}>
                  <option value="small">{t("Pequeña")}</option>
                  <option value="medium">{t("Mediana")}</option>
                  <option value="large">{t("Grande")}</option>
                </select>
              </Field>
            </div>

            <div className="grid grid-cols-2 gap-3">
              <Field label={t("Efecto") }>
                <select className={cn(fieldClass, "border-white/15 bg-white/5 text-white")} value={layer.effect} disabled={!config.enabled} onChange={(event) => updateLayer({ effect: event.currentTarget.value })}>
                  <option value="clean">{t("Limpio")}</option>
                  <option value="mono">{t("Monocromo")}</option>
                  <option value="contrast">{t("Contraste editorial")}</option>
                  <option value="dream">{t("Dream blur")}</option>
                </select>
              </Field>
              <Field label={t("Orientación") }>
                <select className={cn(fieldClass, "border-white/15 bg-white/5 text-white")} value={layer.rotationDegrees} disabled={!config.enabled} onChange={(event) => updateLayer({ rotationDegrees: Number(event.currentTarget.value) })}>
                  <option value={0}>{t("Normal · 0°")}</option>
                  <option value={90}>{t("Girar 90°")}</option>
                  <option value={180}>{t("Girar 180°")}</option>
                  <option value={270}>{t("Girar 270°")}</option>
                </select>
              </Field>
            </div>

            <label className="flex items-center gap-2 text-xs text-white/65">
              <input type="checkbox" checked={layer.mirror} disabled={!config.enabled} onChange={(event) => updateLayer({ mirror: event.currentTarget.checked })} />
              {t("Espejar fuente")}
            </label>

            <Field label={t("Encuadre") }>
              <select className={cn(fieldClass, "border-white/15 bg-white/5 text-white")} value={layer.framing} disabled={!config.enabled} onChange={(event) => updateLayer({ framing: event.currentTarget.value })}>
                <option value="contain">{t("Ajustar · mostrar imagen completa")}</option>
                <option value="cover">{t("Rellenar · recortar bordes")}</option>
              </select>
            </Field>

            <Field label={t("Opacidad máxima: {value}%", { value: layer.opacityPercent })}>
              <input type="range" min={20} max={100} step={5} value={layer.opacityPercent} disabled={!config.enabled} onChange={(event) => updateLayer({ opacityPercent: Number(event.currentTarget.value) })} />
            </Field>
            <Field label={t("Duración AUTO: {value} ms", { value: config.transitionMillis })}>
              <input type="range" min={0} max={3000} step={100} value={config.transitionMillis} disabled={!config.enabled} onChange={(event) => update({ transitionMillis: Number(event.currentTarget.value) })} />
            </Field>

            {previewError ? <p className="rounded-md border border-amber-400/20 bg-amber-400/10 p-2 text-xs text-amber-100">{previewError}</p> : null}
            {!running ? (
              <Button type="button" disabled={busy === "saving" || (config.enabled && !config.cameraEnabled && !config.screenEnabled)} onClick={() => void onSave()}>
                {busy === "saving" ? <LoaderCircle className="h-4 w-4 animate-spin" /> : <Save className="h-4 w-4" />}
                {t("Guardar composición")}
              </Button>
            ) : (
              <p className="text-xs text-white/40">{t("Los cambios de fuente visual se aplican y guardan en vivo sin reiniciar RTMP.")}</p>
            )}
          </aside>
        </div>
      </section>
    </div></>
  );
}

type VisualLayerConfig = {
  layout: string;
  position: string;
  size: string;
  x: number;
  y: number;
  width: number;
  height: number;
  zIndex: number;
  effect: string;
  mirror: boolean;
  rotationDegrees: number;
  framing: string;
  opacityPercent: number;
};

type VisualLayerRect = { x: number; y: number; width: number; height: number };

const VISUAL_CANVAS_WIDTH = 360;
const VISUAL_CANVAS_HEIGHT = 640;
const VISUAL_LAYER_MIN_SIZE = 40;
const VISUAL_LAYER_MIN_VISIBLE = 24;
const VISUAL_LAYER_MAX_SCALE = 4;
const VISUAL_LAYER_MAX_WIDTH = VISUAL_CANVAS_WIDTH * VISUAL_LAYER_MAX_SCALE;
const VISUAL_LAYER_MAX_HEIGHT = VISUAL_CANVAS_HEIGHT * VISUAL_LAYER_MAX_SCALE;

function applyVisualLayerRect(
  config: BroadcastVideoCompositor,
  layer: "camera" | "screen",
  rect: VisualLayerRect
): BroadcastVideoCompositor {
  const rounded = {
    x: Math.round(rect.x),
    y: Math.round(rect.y),
    width: Math.round(rect.width),
    height: Math.round(rect.height)
  };
  return layer === "camera" ? {
    ...config,
    cameraLayout: "free",
    cameraX: rounded.x,
    cameraY: rounded.y,
    cameraWidth: rounded.width,
    cameraHeight: rounded.height
  } : {
    ...config,
    screenLayout: "free",
    screenX: rounded.x,
    screenY: rounded.y,
    screenWidth: rounded.width,
    screenHeight: rounded.height
  };
}

function visualLayerConfig(config: BroadcastVideoCompositor, layer: "camera" | "screen"): VisualLayerConfig {
  return layer === "camera" ? {
    layout: config.cameraLayout,
    position: config.cameraPosition,
    size: config.cameraSize,
    x: config.cameraX,
    y: config.cameraY,
    width: config.cameraWidth,
    height: config.cameraHeight,
    zIndex: config.cameraZIndex,
    effect: config.cameraEffect,
    mirror: config.cameraMirror,
    rotationDegrees: config.cameraRotationDegrees,
    framing: config.cameraFraming,
    opacityPercent: config.cameraOpacityPercent
  } : {
    layout: config.screenLayout,
    position: config.screenPosition,
    size: config.screenSize,
    x: config.screenX,
    y: config.screenY,
    width: config.screenWidth,
    height: config.screenHeight,
    zIndex: config.screenZIndex,
    effect: config.screenEffect,
    mirror: config.screenMirror,
    rotationDegrees: config.screenRotationDegrees,
    framing: config.screenFraming,
    opacityPercent: config.screenOpacityPercent
  };
}

function visualLayerRect(config: VisualLayerConfig) {
  if (config.layout === "free") return { x: config.x, y: config.y, width: config.width, height: config.height };
  if (config.layout === "background") return { x: 0, y: 110, width: VISUAL_CANVAS_WIDTH, height: 340 };
  if (config.layout === "wide") return { x: 0, y: 120, width: VISUAL_CANVAS_WIDTH, height: 225 };
  const size = config.size === "small" ? 105 : config.size === "large" ? 205 : 150;
  const margin = 24;
  const positions: Record<string, { x: number; y: number }> = {
    top_left: { x: margin, y: 120 },
    top_right: { x: VISUAL_CANVAS_WIDTH - size - margin, y: 120 },
    center: { x: (VISUAL_CANVAS_WIDTH - size) / 2, y: (VISUAL_CANVAS_HEIGHT - size) / 2 },
    bottom_left: { x: margin, y: VISUAL_CANVAS_HEIGHT - size - margin },
    bottom_right: {
      x: VISUAL_CANVAS_WIDTH - size - margin,
      y: VISUAL_CANVAS_HEIGHT - size - margin
    }
  };
  return { ...(positions[config.position] ?? positions.top_right), width: size, height: size };
}

function visibleVisualLayerRect(rect: VisualLayerRect): VisualLayerRect {
  const x = Math.max(0, rect.x);
  const y = Math.max(0, rect.y);
  return {
    x,
    y,
    width: Math.max(0, Math.min(VISUAL_CANVAS_WIDTH, rect.x + rect.width) - x),
    height: Math.max(0, Math.min(VISUAL_CANVAS_HEIGHT, rect.y + rect.height) - y)
  };
}

function drawVisualLayer(context: CanvasRenderingContext2D, video: HTMLVideoElement, config: VisualLayerConfig) {
  if (video.readyState < HTMLMediaElement.HAVE_CURRENT_DATA || !video.videoWidth || !video.videoHeight) return;
  const box = visualLayerRect(config);
  const quarterTurn = config.rotationDegrees === 90 || config.rotationDegrees === 270;
  const targetWidth = quarterTurn ? box.height : box.width;
  const targetHeight = quarterTurn ? box.width : box.height;
  const scale = config.framing === "cover"
    ? Math.max(targetWidth / video.videoWidth, targetHeight / video.videoHeight)
    : Math.min(targetWidth / video.videoWidth, targetHeight / video.videoHeight);
  const drawWidth = video.videoWidth * scale;
  const drawHeight = video.videoHeight * scale;
  const filters: Record<string, string> = {
    clean: "none",
    mono: "grayscale(1)",
    contrast: "contrast(1.35) saturate(.82)",
    dream: "blur(1.5px) brightness(1.08) saturate(.72)"
  };

  context.save();
  context.globalAlpha = config.opacityPercent / 100;
  context.beginPath();
  context.rect(box.x, box.y, box.width, box.height);
  context.clip();
  context.fillStyle = "black";
  context.fillRect(box.x, box.y, box.width, box.height);
  context.translate(box.x + box.width / 2, box.y + box.height / 2);
  context.rotate(config.rotationDegrees * Math.PI / 180);
  context.scale(config.mirror ? -1 : 1, 1);
  context.filter = filters[config.effect] ?? "none";
  context.drawImage(video, -drawWidth / 2, -drawHeight / 2, drawWidth, drawHeight);
  context.restore();
}

function bytesToBase64(bytes: Uint8Array | Uint8ClampedArray) {
  let binary = "";
  for (let offset = 0; offset < bytes.length; offset += 32_768) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + 32_768));
  }
  return btoa(binary);
}

function BroadcastTemplateChrome({
  template,
  stationName,
  trackTitle
}: {
  template: string;
  stationName: string;
  trackTitle: string;
}) {
  if (template === "transmission") {
    return <>
      <div className="absolute inset-x-0 top-0 z-10 h-[11.5%] border-b-2 border-black bg-[#f1efe6] px-[5%] py-[4%] text-black">
        <strong className="block text-[clamp(9px,1.9vw,18px)] font-black tracking-[-0.07em]">RAU <span className="text-[#ff4b2b]">/</span> RADIO</strong>
        <span className="absolute bottom-[11%] left-[5%] max-w-[58%] truncate font-mono text-[clamp(4px,.65vw,7px)] uppercase tracking-[0.16em] text-black/55">{stationName}</span>
        <span className="absolute right-[5%] top-[25%] font-mono text-[clamp(4px,.58vw,6px)] tracking-[0.12em]">INDEPENDENT SIGNAL&nbsp; ○</span>
      </div>
      <div className="absolute inset-x-0 top-[11.5%] z-0 h-[5.5%] border-b border-black/60 bg-[#d7ff00]" />
      <div className="absolute inset-x-0 top-[17%] z-0 h-[41%] bg-[#ff4b2b]" style={{ backgroundImage: "linear-gradient(rgba(0,0,0,.18) 1px,transparent 1px),linear-gradient(90deg,rgba(0,0,0,.18) 1px,transparent 1px)", backgroundSize: "20% 18%" }}>
        <span className="absolute left-[5%] top-[6%] font-mono text-[6px] font-semibold tracking-[0.16em] text-black/75">01 / LIVE SOURCE</span>
        <strong className="absolute left-[5%] top-[14%] text-[clamp(14px,3vw,28px)] font-black uppercase leading-[.78] tracking-[-0.08em] text-black/85">LIVE<br />TRANS<br />MISSION</strong>
      </div>
      <div className="absolute inset-x-0 top-[58%] z-10 h-[33%] bg-[#0b0b0b] px-[5.5%] pt-[8%] text-white">
        <span className="font-mono text-[clamp(4px,.68vw,7px)] font-semibold tracking-[0.16em] text-[#d7ff00]">NOW TRANSMITTING</span>
        <span className="mt-[5%] block truncate font-mono text-[clamp(5px,.8vw,8px)] uppercase tracking-[0.1em] text-white/45">{stationName}</span>
        <strong className="mt-[3%] block line-clamp-3 text-[clamp(10px,2vw,20px)] font-semibold uppercase leading-[1.02] tracking-[-0.04em]">{trackTitle}</strong>
        <div className="absolute inset-x-[5.5%] bottom-[9%] flex h-[10%] items-end gap-[1.5%] border-b border-white/25">
          {[35, 55, 72, 92, 48, 65, 85, 42, 68, 95, 52, 76].map((height, index) => <span key={index} className="flex-1 bg-[#f1efe6]" style={{ height: `${height}%` }} />)}
        </div>
      </div>
      <div className="absolute inset-x-0 bottom-0 z-10 h-[9%] border-t-2 border-black bg-[#f1efe6] px-[5%] py-[4%] font-mono text-[clamp(4px,.65vw,7px)] tracking-[0.12em] text-black/55">
        H264 / AAC / 720X1280 / 30FPS <span className="float-right text-black">RAW STREAM ↗</span>
      </div>
    </>;
  }

  if (template === "mono_paper") {
    return <>
      <div className="absolute inset-x-0 top-0 z-10 h-[14.5%] bg-black px-[5%] py-[5%] text-white">
        <strong className="block truncate text-[clamp(9px,1.8vw,17px)] font-semibold uppercase">{stationName}</strong>
        <span className="absolute bottom-[12%] left-[5%] font-mono text-[clamp(4px,.65vw,7px)] tracking-[0.16em] text-white/50">RAU STUDIO / LIVE VISUAL 01</span>
        <span className="absolute right-[5%] top-[23%] h-8 w-8 bg-[#ff4b2b]" />
      </div>
      <div className="absolute left-[5%] right-[5%] top-[18.5%] z-0 h-[40%] bg-[#151515]" />
      <div className="absolute left-[5%] top-[18.5%] z-10 h-[40%] w-[1.4%] bg-[#ff4b2b]" />
      <div className="absolute inset-x-[5%] top-[65%] z-10 border-t-2 border-black pt-[5%] text-black">
        <span className="font-mono text-[clamp(4px,.7vw,7px)] tracking-[0.14em] text-black/50">CURRENT AUDIO / NOW PLAYING</span>
        <strong className="mt-[4%] block line-clamp-4 text-[clamp(11px,2.2vw,22px)] font-black uppercase leading-[.98] tracking-[-0.055em]">{trackTitle}</strong>
      </div>
      <span className="absolute bottom-[3%] left-[5%] z-10 font-mono text-[clamp(4px,.65vw,7px)] tracking-[0.12em] text-black/45">VERTICAL SIGNAL / INDEPENDENT RADIO</span>
    </>;
  }

  return <>
    <div className="absolute inset-x-0 top-0 z-30 h-[1.6%] bg-white" />
    <div className="absolute left-[5%] right-[5%] top-[3%] z-30 h-[11%] border border-white/40 bg-black/70 px-[2.5%] py-[1.5%] text-white">
      <strong className="block truncate text-[clamp(8px,1.8vw,17px)] font-medium uppercase">{stationName}</strong>
      <span className="absolute bottom-[10%] left-[2.5%] font-mono text-[clamp(4px,.65vw,7px)] tracking-[0.16em] text-white/55">LIVE / RAU BROADCAST SYSTEM</span>
    </div>
    <div className="absolute left-[5%] top-[20%] z-10 h-[35%] w-[1.2%] bg-white/85" />
    <div className="absolute left-[9.5%] right-[5%] top-[20%] z-10 h-[35%] border border-white/30 bg-gradient-to-br from-white/55 via-white/5 to-transparent" />
    <div className="absolute inset-x-[5%] top-[70%] z-30 border-t border-white/55 bg-black/90 pt-[4%] text-white">
      <span className="font-mono text-[clamp(5px,.8vw,9px)] tracking-[0.13em] text-white/50">NOW PLAYING / CURRENT AUDIO</span>
      <strong className="mt-[3%] block line-clamp-3 text-[clamp(9px,1.8vw,18px)] font-medium uppercase leading-tight">{trackTitle}</strong>
    </div>
    <span className="absolute bottom-[3%] left-[5%] z-30 font-mono text-[clamp(5px,.7vw,8px)] tracking-[0.12em] text-white/35">H264 / AAC / 720X1280 / 30FPS</span>
  </>;
}

function StudioMonitor({
  label,
  stationName,
  trackTitle,
  visualVisible,
  visualOpacity = 1,
  visualAvailable,
  visualPlaceholder,
  canvasRef,
  visualConfig,
  interactive = false,
  selectedLayer = "camera",
  onSelectLayer,
  onLayerRectChange,
  transitionMillis = 0
}: {
  label: string;
  stationName: string;
  trackTitle: string;
  visualVisible: boolean;
  visualOpacity?: number;
  visualAvailable: boolean;
  visualPlaceholder: string;
  canvasRef: React.RefObject<HTMLCanvasElement | null>;
  visualConfig: BroadcastVideoCompositor;
  interactive?: boolean;
  selectedLayer?: "camera" | "screen";
  onSelectLayer?: (layer: "camera" | "screen") => void;
  onLayerRectChange?: (layer: "camera" | "screen", rect: VisualLayerRect, commit: boolean) => void;
  transitionMillis?: number;
}) {
  const stageRef = useRef<HTMLDivElement | null>(null);
  const beginLayerGesture = (
    event: React.PointerEvent<HTMLElement>,
    layer: "camera" | "screen",
    mode: "move" | "resize"
  ) => {
    if (!interactive || !onLayerRectChange) return;
    event.preventDefault();
    event.stopPropagation();
    onSelectLayer?.(layer);
    const stage = stageRef.current;
    if (!stage) return;
    const bounds = stage.getBoundingClientRect();
    const start = visualLayerRect(visualLayerConfig(visualConfig, layer));
    const startPointer = { x: event.clientX, y: event.clientY };
    let latest = start;
    let changed = false;
    const move = (pointer: PointerEvent) => {
      const deltaX = (pointer.clientX - startPointer.x) * VISUAL_CANVAS_WIDTH / bounds.width;
      const deltaY = (pointer.clientY - startPointer.y) * VISUAL_CANVAS_HEIGHT / bounds.height;
      if (mode === "move") {
        latest = {
          ...start,
          x: Math.max(
            VISUAL_LAYER_MIN_VISIBLE - start.width,
            Math.min(VISUAL_CANVAS_WIDTH - VISUAL_LAYER_MIN_VISIBLE, start.x + deltaX)
          ),
          y: Math.max(
            VISUAL_LAYER_MIN_VISIBLE - start.height,
            Math.min(VISUAL_CANVAS_HEIGHT - VISUAL_LAYER_MIN_VISIBLE, start.y + deltaY)
          )
        };
      } else {
        const minimumWidth = Math.max(VISUAL_LAYER_MIN_SIZE, VISUAL_LAYER_MIN_VISIBLE - start.x);
        const minimumHeight = Math.max(VISUAL_LAYER_MIN_SIZE, VISUAL_LAYER_MIN_VISIBLE - start.y);
        latest = {
          ...start,
          width: Math.max(
            minimumWidth,
            Math.min(VISUAL_LAYER_MAX_WIDTH, start.width + deltaX)
          ),
          height: Math.max(
            minimumHeight,
            Math.min(VISUAL_LAYER_MAX_HEIGHT, start.height + deltaY)
          )
        };
      }
      changed = true;
      onLayerRectChange(layer, latest, false);
    };
    const finish = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", finish);
      window.removeEventListener("pointercancel", finish);
      if (changed) onLayerRectChange(layer, latest, true);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", finish, { once: true });
    window.addEventListener("pointercancel", finish, { once: true });
  };
  const editableLayers = ([
    { key: "screen" as const, enabled: visualConfig.screenEnabled, config: visualLayerConfig(visualConfig, "screen") },
    { key: "camera" as const, enabled: visualConfig.cameraEnabled, config: visualLayerConfig(visualConfig, "camera") }
  ]).filter((layer) => layer.enabled);
  const graphicTemplate = visualConfig.graphicTemplate || "signal_grid";

  return (
    <div>
      <div className="mb-2 flex items-center justify-between text-[11px] font-bold tracking-[0.2em] text-white/55">
        <span>{label}</span>
        <span className={cn("h-2 w-2 rounded-full", label === "PROGRAM" ? "bg-red-500" : "bg-emerald-400")} />
      </div>
      <div
        ref={stageRef}
        className={cn(
          "relative mx-auto aspect-[9/16] max-h-[58vh] overflow-visible",
          interactive ? "z-30" : "z-0"
        )}
      >
        <div
          className={cn(
            "absolute inset-0 overflow-hidden border shadow-inner",
            graphicTemplate === "signal_grid" ? "border-white/15 bg-[#080b09]" : "border-black/30 bg-[#f1efe6]"
          )}
          style={{
            backgroundImage: graphicTemplate === "signal_grid"
              ? "linear-gradient(rgba(255,255,255,.055) 1px, transparent 1px), linear-gradient(90deg, rgba(255,255,255,.055) 1px, transparent 1px)"
              : "linear-gradient(rgba(0,0,0,.065) 1px, transparent 1px), linear-gradient(90deg, rgba(0,0,0,.065) 1px, transparent 1px)",
            backgroundSize: "12.5% 7.03%"
          }}
        >
          <BroadcastTemplateChrome template={graphicTemplate} stationName={stationName} trackTitle={trackTitle} />
          {visualVisible ? <canvas ref={canvasRef} width={VISUAL_CANVAS_WIDTH} height={VISUAL_CANVAS_HEIGHT} className={cn("absolute inset-0 z-20 h-full w-full", interactive && "pointer-events-none")} style={{ opacity: visualOpacity, transition: `opacity ${transitionMillis}ms linear` }} /> : null}
          {visualVisible && !visualAvailable ? (
            <div className="absolute inset-x-[15%] top-[30%] z-20 grid h-[18%] place-items-center border border-white/20 bg-black/70 p-2 text-center">
              <div><Monitor className="mx-auto h-5 w-5 text-white/70" /><span className="mt-1 block text-[8px] uppercase tracking-wider text-white/55">{visualPlaceholder}</span></div>
            </div>
          ) : null}
        </div>
        {interactive && visualVisible ? editableLayers.map((layer) => {
          const rect = visualLayerRect(layer.config);
          const visibleRect = visibleVisualLayerRect(rect);
          const selected = selectedLayer === layer.key;
          const layerZIndex = selected ? 70 : 40 + layer.config.zIndex;
          return (
            <div key={layer.key}>
              <div
                className={cn(
                  "pointer-events-none absolute touch-none border",
                  selected
                    ? "border-emerald-300 shadow-[0_0_0_1px_rgba(16,185,129,.45)]"
                    : "border-white/35"
                )}
                style={{
                  left: `${rect.x / 3.6}%`,
                  top: `${rect.y / 6.4}%`,
                  width: `${rect.width / 3.6}%`,
                  height: `${rect.height / 6.4}%`,
                  zIndex: layerZIndex
                }}
              >
                {selected ? (
                  <button
                    type="button"
                    aria-label="Resize layer"
                    className="pointer-events-auto absolute -bottom-2 -right-2 h-4 w-4 cursor-nwse-resize rounded-sm border border-black bg-emerald-300 shadow"
                    onPointerDown={(event) => beginLayerGesture(event, layer.key, "resize")}
                  />
                ) : null}
              </div>
              <div
                className="absolute touch-none"
                style={{
                  left: `${visibleRect.x / 3.6}%`,
                  top: `${visibleRect.y / 6.4}%`,
                  width: `${visibleRect.width / 3.6}%`,
                  height: `${visibleRect.height / 6.4}%`,
                  zIndex: layerZIndex,
                  cursor: "move"
                }}
                onPointerDown={(event) => beginLayerGesture(event, layer.key, "move")}
              >
                <span className={cn("pointer-events-none absolute -top-5 left-0 rounded-sm px-1.5 py-0.5 font-mono text-[7px] font-semibold uppercase tracking-wider", selected ? "bg-emerald-300 text-black" : "bg-black/80 text-white/70")}>
                  {layer.key === "camera" ? "CAMERA" : "SCREEN / WINDOW"} · Z{layer.config.zIndex}
                </span>
                {selected && (
                  rect.x < 0
                  || rect.y < 0
                  || rect.x + rect.width > VISUAL_CANVAS_WIDTH
                  || rect.y + rect.height > VISUAL_CANVAS_HEIGHT
                ) ? (
                  <span className="pointer-events-none absolute bottom-1 left-1 rounded-sm bg-black/80 px-1 py-0.5 font-mono text-[7px] text-emerald-200">
                    CROPPED
                  </span>
                ) : null}
              </div>
            </div>
          );
        }) : null}
      </div>
    </div>
  );
}

function SourceTabButton({
  id,
  controls,
  active,
  enabled,
  icon,
  label,
  onClick
}: {
  id: string;
  controls: string;
  active: boolean;
  enabled: boolean;
  icon: React.ReactNode;
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      id={id}
      type="button"
      role="tab"
      aria-selected={active}
      aria-controls={controls}
      className={cn(
        "relative flex min-w-0 items-center justify-center gap-1.5 rounded-md px-2 py-2 text-xs font-semibold transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
        active ? "bg-background text-foreground shadow-sm" : "text-muted-foreground hover:bg-background/60 hover:text-foreground"
      )}
      onClick={onClick}
    >
      {icon}
      <span className="truncate">{label}</span>
      {enabled ? <span className="absolute right-1.5 top-1.5 h-1.5 w-1.5 rounded-full bg-emerald-500" aria-hidden="true" /> : null}
    </button>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return <label className="grid gap-1.5 text-xs font-medium text-muted-foreground"><span>{label}</span>{children}</label>;
}

function BroadcastControlAction({
  label,
  description,
  icon,
  variant = "secondary",
  disabled = false,
  onClick
}: {
  label: string;
  description: string;
  icon: ReactNode;
  variant?: ButtonProps["variant"];
  disabled?: boolean;
  onClick: () => void;
}) {
  const [open, setOpen] = useState(false);
  return (
    <Popover open={open}>
      <PopoverTrigger asChild>
        <span
          className="inline-flex"
          onMouseEnter={() => setOpen(true)}
          onMouseLeave={() => setOpen(false)}
          onFocusCapture={() => setOpen(true)}
          onBlurCapture={() => setOpen(false)}
        >
          <Button
            type="button"
            size="icon"
            variant={variant}
            disabled={disabled}
            aria-label={label}
            className="h-9 w-9"
            onClick={() => {
              setOpen(false);
              onClick();
            }}
          >
            {icon}
          </Button>
        </span>
      </PopoverTrigger>
      <PopoverContent
        side="bottom"
        align="center"
        sideOffset={7}
        className="w-64 p-3"
        onOpenAutoFocus={(event) => event.preventDefault()}
      >
        <div className="flex items-start gap-2.5">
          <Info className="mt-0.5 h-4 w-4 shrink-0 text-muted-foreground" />
          <div className="min-w-0">
            <strong className="block text-sm">{label}</strong>
            <p className="mt-1 text-xs leading-relaxed text-muted-foreground">{description}</p>
          </div>
        </div>
      </PopoverContent>
    </Popover>
  );
}

function Metric({ label, value, icon, danger = false }: { label: string; value: string; icon: React.ReactNode; danger?: boolean }) {
  return (
    <Card className={cn("p-3", danger && "border-destructive/35")}>
      <div className="flex items-center gap-2 text-xs text-muted-foreground">{icon}{label}</div>
      <strong className={cn("mt-2 block text-xl", danger && "text-destructive")}>{value}</strong>
    </Card>
  );
}

function StatusBadge({ status, label }: { status: string; label: string }) {
  const live = status === "live";
  const warning = ["connecting", "reconnecting", "stopping"].includes(status);
  return (
    <div className={cn(
      "flex max-w-xl items-center gap-2 rounded-full border px-3 py-1.5 text-xs font-medium",
      live && "border-emerald-500/25 bg-emerald-500/10 text-emerald-800 dark:text-emerald-200",
      warning && "border-amber-500/25 bg-amber-500/10 text-amber-800 dark:text-amber-200",
      !live && !warning && "border-border bg-secondary text-muted-foreground"
    )}>
      <span className={cn("h-2 w-2 shrink-0 rounded-full", live ? "animate-pulse bg-emerald-500" : warning ? "bg-amber-500" : "bg-muted-foreground")} />
      <span className="truncate">{label}</span>
    </div>
  );
}

function QueueStatus({ status }: { status: string }) {
  const labels: Record<string, string> = { queued: "cola", playing: "aire", played: "reproducida", skipped: "saltada", failed: "falló" };
  return (
    <span className={cn(
      "shrink-0 rounded-full px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wide",
      status === "playing" && "bg-emerald-500/10 text-emerald-700 dark:text-emerald-300",
      status === "failed" && "bg-destructive/10 text-destructive",
      !["playing", "failed"].includes(status) && "bg-secondary text-muted-foreground"
    )}>{labels[status] ?? status}</span>
  );
}

function entryTitle(entry: BroadcastQueueEntry) {
  return entry.artist ? `${entry.artist} — ${entry.title}` : entry.title;
}

function formatDuration(seconds?: number | null) {
  if (!seconds || seconds < 1) return "—";
  const minutes = Math.floor(seconds / 60);
  const remainder = Math.floor(seconds % 60);
  return `${minutes}:${String(remainder).padStart(2, "0")}`;
}

function statusLabel(status: string, t: (key: string) => string) {
  const labels: Record<string, string> = {
    idle: t("Detenida"),
    connecting: t("Conectando"),
    live: t("En vivo"),
    reconnecting: t("Reconectando"),
    stopping: t("Deteniendo"),
    error: t("Error")
  };
  return labels[status] ?? status;
}

function errorMessage(cause: unknown, locale: "es" | "en") {
  return translateBackendMessage(locale, cause instanceof Error ? cause.message : String(cause));
}

function safelyUnlisten(unlisten: UnlistenFn) {
  try {
    void Promise.resolve(unlisten()).catch(() => undefined);
  } catch {
    // Tauri may already have removed the listener during a dev reload.
  }
}
