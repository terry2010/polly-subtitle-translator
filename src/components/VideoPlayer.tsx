// libmpv 内嵌视频播放器组件
// 对应需求文档 §3.6 F-09：libmpv 子窗口嵌入 + 播控条 + 位置事件联动
import { memo, useCallback, useEffect, useRef, useState } from "react";
import { api } from "../lib/api";
import type { AudioStream, ProbeResult, InstalledPlayer, PlayerIcon, SubtitleEntry } from "../lib/ipc-types";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { convertFileSrc } from "@tauri-apps/api/core";
import { platform } from "@tauri-apps/plugin-os";
import { Film, Play, Pause, Loader2, Download, Volume2, VolumeX, X, FolderOpen, Info, ChevronRight, MonitorPlay, Languages } from "lucide-react";
import { Button } from "./ui/button";
import { uiState } from "../lib/utils";
import { useTranslation } from "react-i18next";
import { useLibmpvStore } from "../stores/libmpvStore";
import { useSubtitleStore } from "../stores/subtitleStore";
import { useTranslateStore } from "../stores/translateStore";
import { toast } from "sonner";

interface VideoPlayerProps {
  probeResult: ProbeResult | null;
  onPositionUpdate?: (positionSec: number, durationSec: number, paused: boolean) => void;
  onCloseVideo?: () => void;
  onShowVideoInfo?: () => void;
}

// === SECTION 1 END ===

// 音轨选择子组件：独立 memo，避免父组件因 player_position 10Hz 高频 state 更新
// （position/duration/playing）re-render 导致原生 <select> 下拉菜单被关闭。
interface AudioTrackSelectProps {
  audioStreams: AudioStream[];
  audioTrack: number;
  onChange: (aid: number) => void;
}
const AudioTrackSelect = memo(function AudioTrackSelect({
  audioStreams,
  audioTrack,
  onChange,
}: AudioTrackSelectProps) {
  const { t } = useTranslation();
  if (audioStreams.length <= 1) return null;
  return (
    <select
      value={audioTrack}
      onChange={(e) => onChange(parseInt(e.target.value))}
      onFocus={() => { uiState.selectOpen = true; }}
      onBlur={() => { uiState.selectOpen = false; }}
      className="max-w-[140px] truncate rounded-md border border-border bg-background px-1.5 py-1 text-[11px] text-foreground outline-none transition-colors hover:bg-muted focus:border-primary"
      title={t("player.audioTrack", "音轨")}
    >
      {audioStreams.map((a, i) => {
        const lang = a.language || t("player.unknown", "未知");
        const title = a.title ? ` - ${a.title}` : "";
        const codec = a.codec_name ? ` [${a.codec_name}]` : "";
        return (
          <option key={a.index} value={i + 1}>
            {lang}{title}{codec}
          </option>
        );
      })}
    </select>
  );
});

// 倍速选择子组件：独立 memo，避免父组件因 player_position 10Hz 高频 state 更新
// （position/duration/playing）re-render 导致 hover 下拉菜单被关闭。
// 交互：鼠标 hover 触发器时显示下拉列表，点击选项后应用并保持菜单可见（直到鼠标离开）。
interface SpeedSelectProps {
  speed: number;
  options: number[];
  onChange: (s: number) => void;
}
const SpeedSelect = memo(function SpeedSelect({
  speed,
  options,
  onChange,
}: SpeedSelectProps) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  return (
    <div
      className="relative"
      onMouseEnter={() => { uiState.selectOpen = true; setOpen(true); }}
      onMouseLeave={() => { uiState.selectOpen = false; setOpen(false); }}
    >
      <button
        type="button"
        className="flex items-center gap-1.5 rounded-md bg-muted px-2.5 py-1 text-[12px] font-medium tabular-nums transition-colors hover:bg-muted/70"
        title={t("player.speed", "{{speed}}x 倍速", { speed })}
      >
        {speed}x
        <svg width="10" height="10" viewBox="0 0 8 8" className="text-muted-foreground">
          <path d="M1 2.5L4 5.5L7 2.5" fill="none" stroke="currentColor" strokeWidth="1" strokeLinecap="round" strokeLinejoin="round" />
        </svg>
      </button>
      {open && (
        <div className="absolute top-full right-0 mt-1 z-50 min-w-[64px] rounded-md border border-border bg-popover py-1 shadow-md">
          {options.map((s) => (
            <button
              key={s}
              type="button"
              onClick={() => onChange(s)}
              className={`block w-full px-2 py-1 text-left text-[12px] font-medium tabular-nums transition-colors hover:bg-accent ${
                speed === s ? "bg-accent text-accent-foreground" : "text-popover-foreground"
              }`}
            >
              {s}x
            </button>
          ))}
        </div>
      )}
    </div>
  );
});

/// 格式化剩余时间
function formatEta(secs: number): string {
  if (secs <= 0) return "--";
  if (secs < 60) return `${Math.ceil(secs)}秒`;
  const m = Math.floor(secs / 60);
  const s = Math.ceil(secs % 60);
  return `${m}分${s}秒`;
}

export function VideoPlayer({ probeResult, onPositionUpdate, onCloseVideo, onShowVideoInfo }: VideoPlayerProps) {
  const { t } = useTranslation();
  const containerRef = useRef<HTMLDivElement>(null);
  const subtitleStore = useSubtitleStore();
  const translateStore = useTranslateStore();
  // 下载状态从全局 store 获取（路由切换时不丢失）
  const libmpvStatus = useLibmpvStore((s) => s.status);
  const downloading = useLibmpvStore((s) => s.downloading);
  const downloadProgress = useLibmpvStore((s) => s.downloadProgress);
  const downloadStage = useLibmpvStore((s) => s.downloadStage);
  const downloadMessage = useLibmpvStore((s) => s.downloadMessage);
  const downloadError = useLibmpvStore((s) => s.downloadError);
  const downloadSpeed = useLibmpvStore((s) => s.downloadSpeedMbps);
  const downloadEta = useLibmpvStore((s) => s.downloadEtaSecs);
  const startDownload = useLibmpvStore((s) => s.startDownload);
  const [playerReady, setPlayerReady] = useState(false);
  const [playing, setPlaying] = useState(false);
  const [position, setPosition] = useState(0);
  const [duration, setDuration] = useState(0);
  const [volume, setVolume] = useState(100);
  const [speed, setSpeed] = useState(1);
  const [audioTrack, setAudioTrack] = useState(1); // mpv aid，1-based 音频流序号
  const [loadingVideo, setLoadingVideo] = useState(false);
  const speedOptions = [0.5, 0.75, 1, 1.25, 1.5, 2];
  // 右键菜单状态：屏幕物理坐标 → 转 CSS 坐标后定位
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number } | null>(null);
  // 已安装播放器列表（右键"用播放器打开"子菜单）
  const [players, setPlayers] = useState<InstalledPlayer[]>([]);
  const [playersSubmenuOpen, setPlayersSubmenuOpen] = useState(false);
  // 播放器图标映射：exe_path → convertFileSrc URL
  const [iconMap, setIconMap] = useState<Map<string, string>>(new Map());
  // 快捷图标栏的"更多播放器"展开状态（hover 展开箭头时显示）
  const [quickPlayersExpanded, setQuickPlayersExpanded] = useState(false);
  // 当前平台（macOS 上不支持 libmpv 悬浮窗播放）
  const [currentPlatform, setCurrentPlatform] = useState<string>("");
  useEffect(() => {
    try { setCurrentPlatform(platform()); } catch { /* ignore */ }
  }, []);

  // 下载 libmpv（委托给 store，事件监听在 App.tsx 全局处理）
  const handleDownload = useCallback(() => {
    void startDownload();
  }, [startDownload]);

  // 初始化播放器并加载视频
  const initAndLoad = useCallback(async (videoPath: string, dllPath: string, audioStreams: AudioStream[]) => {
    if (!containerRef.current) return;
    const rect = containerRef.current.getBoundingClientRect();
    console.log("[VideoPlayer] 容器 rect:", rect);
    const win = getCurrentWindow();
    const scaleFactor = await win.scaleFactor();
    const x = Math.round(rect.left * scaleFactor);
    const y = Math.round(rect.top * scaleFactor);
    const w = Math.round(rect.width * scaleFactor);
    const h = Math.round(rect.height * scaleFactor);
    console.log("[VideoPlayer] player_init 坐标:", { x, y, w, h });
    try {
      await api.playerInit(dllPath, x, y, w, h);
      setPlayerReady(true);
      await api.playerLoad(videoPath);
      setLoadingVideo(false);
      // 加载后主动设置音轨：mpv 默认 aid=auto 会自动选 disposition_default 的音轨，
      // 但下拉框初始值是数组序号。这里按 disposition_default 找到默认音轨的 aid
      // 并主动设置，确保"播放的音轨 = 下拉框选中项"。
      if (audioStreams.length > 0) {
        const defaultIdx = audioStreams.findIndex((a) => a.disposition_default);
        const aid = defaultIdx >= 0 ? defaultIdx + 1 : 1;
        setAudioTrack(aid);
        await api.playerSetAudioTrack(aid);
      }
      // 自动播放
      await api.playerPlay();
      setPlaying(true);
    } catch (e) {
      console.error("初始化播放器失败:", e);
      setPlayerReady(false);
      setLoadingVideo(false);
    }
  }, []);

  // 视频变更时初始化播放器
  useEffect(() => {
    if (!probeResult || !libmpvStatus?.downloaded || !libmpvStatus.path) return;
    let cancelled = false;
    setLoadingVideo(true);
    setPlayerReady(false);
    setAudioTrack(1); // 重置为默认音轨（load 后会按 disposition_default 校正）
    // 先销毁旧播放器，await 确保销毁完成后再创建新的
    // （不 await 会导致多个 mpv 实例同时存在，旧实例脱离嵌入窗口变成独立窗口）
    (async () => {
      try { await api.playerDestroy(); } catch { /* 播放器未初始化，忽略 */ }
      if (cancelled) return;
      // 延迟一帧让 DOM 更新后获取正确坐标
      await new Promise(r => setTimeout(r, 100));
      if (cancelled) return;
      initAndLoad(probeResult.video_path, libmpvStatus.path!, probeResult.audio_streams);
    })();
    return () => { cancelled = true; };
  }, [probeResult, libmpvStatus, initAndLoad]);

  // 监听位置事件
  // 仅在播放状态变化（开始/暂停/恢复）时记录日志，避免高频刷屏
  const lastPausedRef = useRef<boolean | null>(null);
  useEffect(() => {
    const unlisten = listen<{ position: number; duration: number; paused: boolean }>(
      "player_position",
      (event) => {
        const { position: pos, duration: dur, paused } = event.payload;
        // 仅在 paused 状态变化时记录日志
        if (lastPausedRef.current !== paused) {
          const prev = lastPausedRef.current;
          if (prev === null) {
            console.log("[VideoPlayer] 播放开始", { pos, dur });
          } else if (paused) {
            console.log("[VideoPlayer] 暂停", { pos, dur });
          } else {
            console.log("[VideoPlayer] 恢复播放", { pos, dur });
          }
          lastPausedRef.current = paused;
        }
        setPosition(pos);
        setDuration(dur);
        setPlaying(!paused);
        onPositionUpdate?.(pos, dur, paused);
      },
    );
    return () => { unlisten.then((fn) => fn()); };
  }, [onPositionUpdate]);

  // 窗口缩放/滚动时同步悬浮窗口位置和大小
  // 窗口移动由后端 delta 线程处理（更跟手，无事件延迟）
  useEffect(() => {
    if (!playerReady) return;
    let rafId = 0;
    let debounceTimer: ReturnType<typeof setTimeout> | null = null;
    const syncPosition = () => {
      if (rafId) return;
      rafId = requestAnimationFrame(() => {
        rafId = 0;
        if (!containerRef.current) return;
        const rect = containerRef.current.getBoundingClientRect();
        const win = getCurrentWindow();
        win.scaleFactor().then((sf: number) => {
          const x = Math.round(rect.left * sf);
          const y = Math.round(rect.top * sf);
          const w = Math.round(rect.width * sf);
          const h = Math.round(rect.height * sf);
          api.playerResize(x, y, w, h).catch(() => {});
        });
      });
    };
    const debouncedSync = () => {
      if (debounceTimer) clearTimeout(debounceTimer);
      debounceTimer = setTimeout(() => {
        debounceTimer = null;
        syncPosition();
      }, 80);
    };
    const win = getCurrentWindow();
    // 只监听 resize（大小变化），不监听 move（位置由后端 delta 处理）
    const unlistenResize = win.onResized(() => debouncedSync());
    const onScroll = () => debouncedSync();
    window.addEventListener("resize", onScroll);
    window.addEventListener("scroll", onScroll, true);
    syncPosition();
    return () => {
      unlistenResize.then((fn) => fn());
      window.removeEventListener("resize", onScroll);
      window.removeEventListener("scroll", onScroll, true);
      if (debounceTimer) clearTimeout(debounceTimer);
      if (rafId) cancelAnimationFrame(rafId);
    };
  }, [playerReady]);

  // 组件卸载时销毁播放器
  // 先 hide 再 destroy：hide 立即设置 HOOK_HIDDEN 并隐藏子窗口，
  // 防止导航切换时 DOM 变化触发位置同步钩子把子窗口闪到错误位置。
  useEffect(() => {
    return () => {
      api.playerHide().catch(() => {});
      api.playerDestroy().catch(() => {});
    };
  }, []);

  // 播控操作
  const togglePlay = useCallback(async () => {
    console.log("[VideoPlayer] togglePlay 调用，当前 playing=", playing);
    if (playing) {
      await api.playerPause();
      setPlaying(false);
    } else {
      await api.playerPlay();
      setPlaying(true);
    }
  }, [playing]);

  // 暂停视频（不切换，仅暂停）
  const pauseVideo = useCallback(async () => {
    if (playing) {
      await api.playerPause();
      setPlaying(false);
    }
  }, [playing]);

  // 空格键播放/暂停：仅当鼠标在程序窗口内且不在字幕编辑区时响应。
  // - 鼠标在字幕编辑区：让用户在文本框内正常输入空格
  // - 鼠标在程序窗口外：不响应（避免后台误触发）
  // 用 ref 跟踪鼠标是否离开窗口（mouseleave on document.documentElement）
  const mouseOutsideWindowRef = useRef(false);
  useEffect(() => {
    const onMouseLeave = () => { mouseOutsideWindowRef.current = true; };
    const onMouseEnter = () => { mouseOutsideWindowRef.current = false; };
    document.documentElement.addEventListener("mouseleave", onMouseLeave);
    document.documentElement.addEventListener("mouseenter", onMouseEnter);
    return () => {
      document.documentElement.removeEventListener("mouseleave", onMouseLeave);
      document.documentElement.removeEventListener("mouseenter", onMouseEnter);
    };
  }, []);

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key !== " " && e.code !== "Space") return;
      // 鼠标在字幕编辑区内：不响应，保留给文本编辑
      if (uiState.mouseInSubtitleEditor) return;
      // 鼠标在程序窗口外：不响应
      if (mouseOutsideWindowRef.current) return;
      // 焦点在 input/textarea/contenteditable：让用户正常输入空格
      const target = e.target as HTMLElement | null;
      const tag = target?.tagName;
      if (tag === "INPUT" || tag === "TEXTAREA" || target?.isContentEditable) return;
      if (!playerReady) return;
      e.preventDefault();
      void togglePlay();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [playerReady, togglePlay]);

  // 监听子窗口点击事件（WS_EX_TRANSPARENT 穿透不可靠，
  // 改由后端 child_wnd_proc 捕获 WM_LBUTTONDOWN 并 emit "player-click"）
  useEffect(() => {
    const unlisten = listen("player-click", () => {
      console.log("[VideoPlayer] 收到 player-click 事件");
      togglePlay();
    });
    return () => { unlisten.then((fn) => fn()); };
  }, [togglePlay]);

  // 监听子窗口右键事件：后端 child_wnd_proc 捕获 WM_RBUTTONDOWN，
  // emit 屏幕物理坐标 (x, y)。前端转为 CSS 坐标后定位菜单。
  // 弹菜单前先 playerHide 隐藏悬浮窗（否则菜单被视频画面遮挡），菜单关闭时 playerShow 恢复。
  useEffect(() => {
    if (!playerReady) return;
    const unlisten = listen<[number, number]>("player-right-click", async (event) => {
      const [screenX, screenY] = event.payload;
      try {
        const win = getCurrentWindow();
        const sf = await win.scaleFactor();
        const pos = await win.outerPosition();
        const cssX = (screenX - pos.x) / sf;
        const cssY = (screenY - pos.y) / sf;
        // 隐藏悬浮窗，让 HTML 菜单可见
        await api.playerHide();
        setContextMenu({ x: cssX, y: cssY });
        setPlayersSubmenuOpen(false);
      } catch (e) {
        console.error("右键菜单定位失败:", e);
      }
    });
    return () => { unlisten.then((fn) => fn()); };
  }, [playerReady]);

  // 菜单关闭时恢复悬浮窗显示
  const closeContextMenu = useCallback(() => {
    setContextMenu(null);
    setPlayersSubmenuOpen(false);
    api.playerShow().catch(() => {});
  }, []);

  // 点击外部 / ESC 关闭菜单
  useEffect(() => {
    if (!contextMenu) return;
    const handleClick = () => closeContextMenu();
    const handleEsc = (e: KeyboardEvent) => { if (e.key === "Escape") closeContextMenu(); };
    // 延迟一帧绑定，避免触发右键的同一事件立即关闭
    const timer = setTimeout(() => {
      window.addEventListener("click", handleClick);
      window.addEventListener("keydown", handleEsc);
    }, 0);
    return () => {
      clearTimeout(timer);
      window.removeEventListener("click", handleClick);
      window.removeEventListener("keydown", handleEsc);
    };
  }, [contextMenu, closeContextMenu]);

  // 加载已安装播放器列表 + 图标映射（右键菜单展开"用播放器打开"子菜单时按需加载）
  const loadPlayers = useCallback(async () => {
    if (!probeResult) return;
    try {
      const list = await api.listInstalledPlayers(probeResult.video_path);
      setPlayers(list);
      // 同时加载图标映射（图标已在加载视频时异步提取到缓存目录）
      // 扫描缓存目录，为每个播放器匹配图标
      const icons = await api.extractPlayerIcons(probeResult.video_path).catch(() => [] as PlayerIcon[]);
      const map = new Map<string, string>();
      for (const icon of icons) {
        map.set(icon.exe_path, convertFileSrc(icon.icon_path));
      }
      setIconMap(map);
    } catch (e) {
      console.error("获取播放器列表失败:", e);
    }
  }, [probeResult]);

  // 视频加载后自动加载播放器列表和图标（供播放控制栏的快捷图标按钮使用）
  useEffect(() => {
    if (probeResult) void loadPlayers();
  }, [probeResult, loadPlayers]);

  // === 右键菜单动作 ===

  // 关闭视频
  const handleCloseVideo = useCallback(() => {
    closeContextMenu();
    onCloseVideo?.();
  }, [onCloseVideo, closeContextMenu]);

  // 查找当前播放位置对应的字幕条目
  const findCurrentEntry = useCallback((): SubtitleEntry | null => {
    const file = subtitleStore.file;
    if (!file) return null;
    const currentMs = position * 1000;
    return file.entries.find((e) => !e._deleted && currentMs >= e.start_ms && currentMs < e.end_ms) ?? null;
  }, [subtitleStore.file, position]);

  // 从本字幕开头播放：seek 到当前字幕的 start_ms 并播放
  const handlePlayFromCurrent = useCallback(async () => {
    closeContextMenu();
    const entry = findCurrentEntry();
    if (!entry) {
      toast.info(t("player.noCurrentSubtitle", "当前没有对应字幕"));
      return;
    }
    try {
      await api.playerSeek(entry.start_ms / 1000);
      await api.playerPlay();
      setPlaying(true);
    } catch (e) {
      console.error("从本字幕开头播放失败:", e);
    }
  }, [findCurrentEntry, closeContextMenu, t]);

  // 从下一句字幕播放
  const handlePlayFromNext = useCallback(async () => {
    closeContextMenu();
    const file = subtitleStore.file;
    if (!file) return;
    const currentMs = position * 1000;
    // 找到当前条目之后的第一条未删除字幕
    const next = file.entries.find((e) => !e._deleted && e.start_ms > currentMs);
    if (!next) {
      toast.info(t("player.noNextSubtitle", "没有下一句字幕"));
      return;
    }
    try {
      await api.playerSeek(next.start_ms / 1000);
      await api.playerPlay();
      setPlaying(true);
    } catch (e) {
      console.error("从下一句字幕播放失败:", e);
    }
  }, [subtitleStore.file, position, closeContextMenu, t]);

  // 翻译本条字幕
  const handleTranslateCurrent = useCallback(async () => {
    closeContextMenu();
    const entry = findCurrentEntry();
    if (!entry) {
      toast.info(t("player.noCurrentSubtitle", "当前没有对应字幕"));
      return;
    }
    if (entry.text.includes("\\p1")) return; // 跳过矢量绘图指令
    try {
      await translateStore.startTranslate([entry], (index, translated, failed) => {
        subtitleStore.updateEntry(index, { translated, failed });
      });
    } catch (e) {
      console.error("翻译本条字幕失败:", e);
    }
  }, [findCurrentEntry, translateStore, subtitleStore, closeContextMenu, t]);

  // 防止重复打开播放器的锁（双击/连续点击保护）
  const openingPlayerRef = useRef(false);

  // 用指定播放器打开
  const handleOpenWithPlayer = useCallback(async (exePath: string, playerName?: string) => {
    if (openingPlayerRef.current) return; // 正在打开中，忽略重复点击
    openingPlayerRef.current = true;
    closeContextMenu();
    if (!probeResult) { openingPlayerRef.current = false; return; }
    // 立刻 toast 提示
    const displayName = playerName ?? exePath.split(/[\\/]/).pop()?.replace(/\.exe$/i, "") ?? t("player.defaultPlayer", "播放器");
    toast.info(`${t("player.openingWith", "正在使用")} ${displayName} ${t("player.openingVideo", "打开视频文件")}`);
    await pauseVideo(); // 暂停内嵌播放，避免和外部播放器同时播放
    try {
      await api.openWithPlayer(exePath, probeResult.video_path);
    } catch (e) {
      toast.error(t("player.openWithPlayerFailed", "打开播放器失败"));
      console.error(e);
    } finally {
      // 500ms 后释放锁，防止用户立刻再次点击
      setTimeout(() => { openingPlayerRef.current = false; }, 1000);
    }
  }, [probeResult, closeContextMenu, t, pauseVideo]);

  // 打开视频文件夹
  const handleOpenFolder = useCallback(() => {
    closeContextMenu();
    if (!probeResult) return;
    void pauseVideo();
    api.revealInExplorer(probeResult.video_path).catch((e) => {
      console.error("打开文件夹失败:", e);
    });
  }, [probeResult, closeContextMenu, pauseVideo]);

  // 视频信息：通过回调通知 MainView 展开顶部卡片 + 显示遮罩
  const handleShowVideoInfo = useCallback(() => {
    closeContextMenu();
    onShowVideoInfo?.();
  }, [closeContextMenu, onShowVideoInfo]);

  const handleSeek = useCallback(async (e: React.ChangeEvent<HTMLInputElement>) => {
    const time = parseFloat(e.target.value);
    setPosition(time);
    await api.playerSeek(time);
  }, []);

  const handleVolume = useCallback(async (e: React.ChangeEvent<HTMLInputElement>) => {
    const vol = parseInt(e.target.value);
    setVolume(vol);
    if (vol > 0) lastVolumeRef.current = vol;
    await api.playerSetVolume(vol);
  }, []);

  // 静音切换：记住静音前的音量，再次点击恢复
  const lastVolumeRef = useRef(100);
  const toggleMute = useCallback(async () => {
    if (volume > 0) {
      lastVolumeRef.current = volume;
      setVolume(0);
      await api.playerSetVolume(0);
    } else {
      const restore = lastVolumeRef.current || 100;
      setVolume(restore);
      await api.playerSetVolume(restore);
    }
  }, [volume]);

  // 右键菜单：静音切换（复用 toggleMute）
  const contextToggleMute = useCallback(async () => {
    closeContextMenu();
    await toggleMute();
  }, [toggleMute, closeContextMenu]);

  const applySpeed = useCallback(async (spd: number) => {
    setSpeed(spd);
    await api.playerSetSpeed(spd);
  }, []);

  // 切换音频轨道：选中后立刻让视频换成选中的音轨
  const handleAudioTrackChange = useCallback(async (aid: number) => {
    setAudioTrack(aid);
    await api.playerSetAudioTrack(aid);
  }, []);

  // 格式化时间
  const formatTime = (sec: number) => {
    if (!sec || isNaN(sec)) return "00:00";
    const m = Math.floor(sec / 60);
    const s = Math.floor(sec % 60);
    return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
  };

// === SECTION 2 END ===

  // 渲染
  const aspectRatio = probeResult?.video_stream
    ? `${probeResult.video_stream.width} / ${probeResult.video_stream.height}`
    : "16 / 9";

  // 进度条 / 音量滑块填充百分比（用渐变背景表现已播放部分）
  const seekPct = duration > 0 ? Math.min(100, (position / duration) * 100) : 0;
  const sliderBg = (pct: number) =>
    `linear-gradient(to right, hsl(var(--primary)) ${pct}%, hsl(var(--muted)) ${pct}%)`;

  // 未下载 libmpv：显示下载提示
  if (libmpvStatus && !libmpvStatus.downloaded) {
    const stageLabel = downloadStage === "fetching" ? t("player.libmpvStageFetching", "获取版本信息")
      : downloadStage === "downloading" ? t("player.libmpvStageDownloading", "下载中")
      : downloadStage === "extracting" ? t("player.libmpvStageExtracting", "解压安装")
      : downloadStage === "done" ? t("player.libmpvStageDone", "完成")
      : t("player.libmpvStagePreparing", "准备中");
    return (
      <div
        className="relative bg-black flex items-center justify-center overflow-hidden rounded"
        style={{ aspectRatio }}
      >
        <div className="text-center text-white/50 w-64">
          <Film className="mx-auto h-12 w-12 mb-3 opacity-40" />
          {downloading ? (
            <>
              <p className="text-xs mb-2 text-white/70">{stageLabel}... {downloadProgress >= 0 ? `${downloadProgress}%` : ""}</p>
              {/* 进度条：-1 为不定态 */}
              {downloadProgress >= 0 ? (
                <div className="w-full h-2 bg-white/10 rounded-full overflow-hidden mb-2">
                  <div
                    className="h-full bg-primary rounded-full transition-all duration-300"
                    style={{ width: `${downloadProgress}%` }}
                  />
                </div>
              ) : (
                <div className="w-full h-2 bg-white/10 rounded-full overflow-hidden mb-2 relative">
                  <div
                    className="h-full bg-primary rounded-full absolute"
                    style={{ width: "40%", left: "-40%", animation: "indeterminate 1.5s infinite linear" }}
                  />
                </div>
              )}
              <div className="flex justify-between text-xs text-white/40 tabular-nums">
                <span className="truncate">{stageLabel}</span>
                {downloadStage === "downloading" && downloadSpeed > 0 && (
                  <span className="shrink-0">{downloadSpeed.toFixed(1)} MB/s · {formatEta(downloadEta)}</span>
                )}
              </div>
            </>
          ) : (
            <>
              {downloadError ? (
                <p className="text-xs mb-3 text-red-400/80 line-clamp-3">{downloadError}</p>
              ) : (
                <p className="text-xs mb-4">{t("player.libmpvDownloadHint", "需要下载播放组件（约 30MB）")}</p>
              )}
              <Button size="sm" onClick={handleDownload}>
                <Download className="mr-1 h-4 w-4" />{t("player.libmpvDownloadButton", "下载播放组件")}
              </Button>
            </>
          )}
        </div>
      </div>
    );
  }

  // 无视频：显示占位
  if (!probeResult) {
    return (
      <div
        className="relative bg-black flex items-center justify-center overflow-hidden rounded"
        style={{ aspectRatio }}
      >
        <div className="text-center text-white/30">
          <Film className="mx-auto h-12 w-12 mb-2 opacity-50" />
          <p className="text-xs">{t("player.placeholder", "打开视频后在此播放")}</p>
        </div>
      </div>
    );
  }

  // 有视频 + libmpv 已下载：显示播放器
  // z-10：建立层叠上下文，使倍速下拉框（向下展开）能盖住下方字幕预览区
  return (
    <div className="relative bg-black rounded z-10">
      {/* 视频渲染区（libmpv 子窗口会覆盖在此区域） */}
      <div
        ref={containerRef}
        className="w-full bg-black flex items-center justify-center overflow-hidden rounded"
        style={{ aspectRatio, maxHeight: "40vh" }}
        onClick={() => { if (playerReady) togglePlay(); }}
        onContextMenu={(e) => { e.preventDefault(); e.stopPropagation(); }}
        title={playerReady ? (playing ? t("player.clickPause", "点击暂停") : t("player.clickPlay", "点击播放")) : undefined}
      >
        {loadingVideo && (
          <div className="absolute inset-0 flex items-center justify-center text-white/50">
            <Loader2 className="h-8 w-8 animate-spin" />
          </div>
        )}
      </div>

      {/* 独立播放控制器（视频下方，统一主题风格） */}
      {playerReady && (
        <div className="relative z-50 select-none border-t border-border bg-card px-3 pt-2.5 pb-2">
          {/* 进度条行：当前时间 — 进度条 — 总时长 */}
          <div className="mb-2 flex items-center gap-2.5">
            <span className="w-[44px] text-right font-mono text-[11px] tabular-nums text-muted-foreground">
              {formatTime(position)}
            </span>
            <input
              type="range"
              min={0}
              max={duration || 0}
              step={0.1}
              value={position}
              onChange={handleSeek}
              className="player-slider flex-1"
              style={{ background: sliderBg(seekPct) }}
              title={t("player.progress", "进度")}
            />
            <span className="w-[44px] font-mono text-[11px] tabular-nums text-muted-foreground">
              {formatTime(duration)}
            </span>
          </div>

          {/* 控制行 */}
          <div className="flex items-center gap-3">
            {/* 播放 / 暂停 */}
            <button
              onClick={togglePlay}
              className="flex h-8 w-8 items-center justify-center rounded-full bg-primary text-primary-foreground shadow-sm transition-transform hover:scale-105 active:scale-95"
              title={playing ? t("player.pause", "暂停") : t("player.play", "播放")}
            >
              {playing ? <Pause className="h-4 w-4" /> : <Play className="h-4 w-4 translate-x-[1px]" />}
            </button>

            {/* 音量 */}
            <div className="flex items-center gap-1.5">
              <button
                onClick={toggleMute}
                className="text-muted-foreground transition-colors hover:text-foreground"
                title={volume === 0 ? t("player.unmute", "取消静音") : t("player.mute", "静音")}
              >
                {volume === 0 ? <VolumeX className="h-4 w-4" /> : <Volume2 className="h-4 w-4" />}
              </button>
              <input
                type="range"
                min={0}
                max={100}
                value={volume}
                onChange={handleVolume}
                className="player-slider w-20"
                style={{ background: sliderBg(volume) }}
                title={t("player.volume", "音量")}
              />
            </div>

            {/* 音轨选择（独立 memo 子组件，避免 10Hz position 更新导致下拉关闭） */}
            <AudioTrackSelect
              audioStreams={probeResult.audio_streams}
              audioTrack={audioTrack}
              onChange={handleAudioTrackChange}
            />

            <div className="flex-1" />

            {/* 快捷操作图标组：文件夹 + 播放器图标（最多3个，超出折叠为展开箭头） */}
            <div className="flex items-center gap-1">
              {/* 打开视频所在文件夹 */}
              <button
                className="rounded p-1 text-muted-foreground hover:bg-accent hover:text-accent-foreground transition-colors"
                title={t("player.openFolder", "打开视频文件夹")}
                onClick={handleOpenFolder}
              >
                <FolderOpen className="h-4 w-4" />
              </button>
              {/* 播放器图标按钮：最多显示前 2 个，第 3 个位置为展开箭头（hover 显示剩余） */}
              {(() => {
                const maxVisible = 2; // 最多直接显示 2 个播放器图标
                const visiblePlayers = players.slice(0, maxVisible);
                const hiddenPlayers = players.slice(maxVisible);
                const renderPlayerBtn = (p: InstalledPlayer) => {
                  const iconUrl = iconMap.get(p.exe_path);
                  return (
                    <button
                      key={p.exe_path}
                      className="rounded p-1 hover:bg-accent transition-colors"
                      title={t("player.openWithName", "用 {{name}} 打开视频", { name: p.name })}
                      onClick={() => void handleOpenWithPlayer(p.exe_path, p.name)}
                    >
                      {iconUrl ? (
                        <img src={iconUrl} alt={p.name} className="h-4 w-4 object-contain" />
                      ) : (
                        <MonitorPlay className="h-4 w-4 text-muted-foreground" />
                      )}
                    </button>
                  );
                };
                return (
                  <>
                    {visiblePlayers.map(renderPlayerBtn)}
                    {hiddenPlayers.length > 0 && (
                      <div
                        className="relative"
                        onMouseEnter={() => setQuickPlayersExpanded(true)}
                        onMouseLeave={() => setQuickPlayersExpanded(false)}
                      >
                        {/* 展开箭头按钮 */}
                        <button
                          className="rounded p-1 text-muted-foreground hover:bg-accent hover:text-accent-foreground transition-colors"
                          title={t("player.morePlayers", "更多播放器")}
                        >
                          <ChevronRight className="h-4 w-4" />
                        </button>
                        {/* hover 展开的剩余播放器列表（向下弹出，紧贴箭头无间隙避免 mouseleave） */}
                        {quickPlayersExpanded && (
                          <div className="absolute top-full left-0 min-w-[140px] pt-1">
                            <div className="rounded-md border border-border bg-popover p-1 shadow-lg">
                              {hiddenPlayers.map(renderPlayerBtn)}
                            </div>
                          </div>
                        )}
                      </div>
                    )}
                  </>
                );
              })()}
            </div>

            {/* 倍速（hover 下拉框） */}
            <SpeedSelect
              speed={speed}
              options={speedOptions}
              onChange={applySpeed}
            />
          </div>
        </div>
      )}

      {/* === 右键上下文菜单 === */}
      {contextMenu && (
        <div
          className="fixed z-[100] min-w-[180px] rounded-md border border-border bg-popover p-1 shadow-lg select-none"
          style={{ left: contextMenu.x, top: contextMenu.y }}
          onClick={(e) => e.stopPropagation()}
          onContextMenu={(e) => { e.preventDefault(); e.stopPropagation(); }}
        >
          {/* 从本字幕开头播放 */}
          <button
            className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-xs hover:bg-accent disabled:opacity-40 disabled:cursor-not-allowed"
            onClick={() => void handlePlayFromCurrent()}
            disabled={!subtitleStore.file}
          >
            <Play className="h-3.5 w-3.5" />
            {t("player.playFromCurrent", "从本字幕开头播放")}
          </button>
          {/* 从下一句字幕播放 */}
          <button
            className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-xs hover:bg-accent disabled:opacity-40 disabled:cursor-not-allowed"
            onClick={() => void handlePlayFromNext()}
            disabled={!subtitleStore.file}
          >
            <Play className="h-3.5 w-3.5" />
            {t("player.playFromNext", "从下一句字幕播放")}
          </button>
          {/* 翻译本条字幕 */}
          <button
            className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-xs hover:bg-accent disabled:opacity-40 disabled:cursor-not-allowed"
            onClick={() => void handleTranslateCurrent()}
            disabled={!subtitleStore.file || translateStore.translating}
          >
            <Languages className="h-3.5 w-3.5" />
            {t("player.translateCurrent", "翻译本条字幕")}
          </button>

          <div className="my-1 h-px bg-border" />

          {/* 播放 / 暂停 */}
          <button
            className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-xs hover:bg-accent"
            onClick={() => { closeContextMenu(); void togglePlay(); }}
          >
            <Play className="h-3.5 w-3.5" />
            {playing ? t("player.pause", "暂停") : t("player.play", "播放")}
          </button>
          {/* 静音 / 取消静音 */}
          <button
            className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-xs hover:bg-accent"
            onClick={() => void contextToggleMute()}
          >
            {volume === 0 ? <VolumeX className="h-3.5 w-3.5" /> : <Volume2 className="h-3.5 w-3.5" />}
            {volume === 0 ? t("player.unmute", "取消静音") : t("player.mute", "静音")}
          </button>
          {/* 关闭视频 */}
          <button
            className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-xs text-destructive hover:bg-destructive/10"
            onClick={handleCloseVideo}
          >
            <X className="h-3.5 w-3.5" />
            {t("player.closeVideo", "关闭视频")}
          </button>

          <div className="my-1 h-px bg-border" />

          {/* 用播放器打开 ▸ 子菜单 */}
          <div className="relative">
            <button
              className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-xs hover:bg-accent"
              onMouseEnter={() => { setPlayersSubmenuOpen(true); void loadPlayers(); }}
            >
              <MonitorPlay className="h-3.5 w-3.5" />
              {t("player.openWith", "用播放器打开")}
              <ChevronRight className="ml-auto h-3.5 w-3.5" />
            </button>
            {playersSubmenuOpen && (
              <div
                className="absolute left-full top-0 ml-0.5 min-w-[160px] rounded-md border border-border bg-popover p-1 shadow-lg"
                onMouseLeave={() => setPlayersSubmenuOpen(false)}
              >
                {players.length === 0 && (
                  <div className="px-2 py-1.5 text-xs text-muted-foreground">
                    {t("player.noPlayers", "未找到播放器")}
                  </div>
                )}
                {players.map((p) => {
                  const iconUrl = iconMap.get(p.exe_path);
                  return (
                    <button
                      key={p.exe_path}
                      className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-xs hover:bg-accent"
                      onClick={() => void handleOpenWithPlayer(p.exe_path, p.name)}
                    >
                      {iconUrl ? (
                        <img src={iconUrl} alt="" className="h-4 w-4 shrink-0 object-contain" />
                      ) : (
                        <MonitorPlay className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
                      )}
                      <span className="truncate flex-1 text-left">{p.name}</span>
                      {p.is_default && (
                        <span className="shrink-0 text-[10px] text-primary">{t("player.default", "默认")}</span>
                      )}
                    </button>
                  );
                })}
              </div>
            )}
          </div>

          {/* 打开视频文件夹 */}
          <button
            className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-xs hover:bg-accent"
            onClick={handleOpenFolder}
          >
            <FolderOpen className="h-3.5 w-3.5" />
            {t("player.openFolder", "打开视频文件夹")}
          </button>
          {/* 视频信息 */}
          <button
            className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-xs hover:bg-accent"
            onClick={handleShowVideoInfo}
          >
            <Info className="h-3.5 w-3.5" />
            {t("player.videoInfo", "视频信息")}
          </button>
        </div>
      )}
    </div>
  );
}

// === SECTION 3 END ===
