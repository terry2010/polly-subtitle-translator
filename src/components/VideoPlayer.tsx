// libmpv 内嵌视频播放器组件
// 对应需求文档 §3.6 F-09：libmpv 子窗口嵌入 + 播控条 + 位置事件联动
import { memo, useCallback, useEffect, useRef, useState } from "react";
import { api } from "../lib/api";
import type { AudioStream, ProbeResult } from "../lib/ipc-types";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Film, Play, Pause, Loader2, Download, Volume2, VolumeX } from "lucide-react";
import { Button } from "./ui/button";
import { uiState } from "../lib/utils";
import { useTranslation } from "react-i18next";
import { useLibmpvStore } from "../stores/libmpvStore";

interface VideoPlayerProps {
  probeResult: ProbeResult | null;
  onPositionUpdate?: (positionSec: number, durationSec: number, paused: boolean) => void;
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
  if (audioStreams.length <= 1) return null;
  return (
    <select
      value={audioTrack}
      onChange={(e) => onChange(parseInt(e.target.value))}
      onFocus={() => { uiState.selectOpen = true; }}
      onBlur={() => { uiState.selectOpen = false; }}
      className="max-w-[140px] truncate rounded-md border border-border bg-background px-1.5 py-1 text-[11px] text-foreground outline-none transition-colors hover:bg-muted focus:border-primary"
      title="音轨"
    >
      {audioStreams.map((a, i) => {
        const lang = a.language || "未知";
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

/// 格式化剩余时间
function formatEta(secs: number): string {
  if (secs <= 0) return "--";
  if (secs < 60) return `${Math.ceil(secs)}秒`;
  const m = Math.floor(secs / 60);
  const s = Math.ceil(secs % 60);
  return `${m}分${s}秒`;
}

export function VideoPlayer({ probeResult, onPositionUpdate }: VideoPlayerProps) {
  const { t } = useTranslation();
  const containerRef = useRef<HTMLDivElement>(null);
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
  useEffect(() => {
    const unlisten = listen<{ position: number; duration: number; paused: boolean }>(
      "player_position",
      (event) => {
        const { position: pos, duration: dur, paused } = event.payload;
        console.log("[VideoPlayer] player_position:", { pos, dur, paused });
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
    const syncPosition = () => {
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
    };
    const win = getCurrentWindow();
    // 只监听 resize（大小变化），不监听 move（位置由后端 delta 处理）
    const unlistenResize = win.onResized(() => syncPosition());
    const onScroll = () => syncPosition();
    window.addEventListener("resize", onScroll);
    window.addEventListener("scroll", onScroll, true);
    syncPosition();
    return () => {
      unlistenResize.then((fn) => fn());
      window.removeEventListener("resize", onScroll);
      window.removeEventListener("scroll", onScroll, true);
    };
  }, [playerReady]);

  // 组件卸载时销毁播放器
  useEffect(() => {
    return () => { api.playerDestroy().catch(() => {}); };
  }, []);

  // 播控操作
  const togglePlay = useCallback(async () => {
    if (playing) {
      await api.playerPause();
      setPlaying(false);
    } else {
      await api.playerPlay();
      setPlaying(true);
    }
  }, [playing]);

  // 监听子窗口点击事件（WS_EX_TRANSPARENT 穿透不可靠，
  // 改由后端 child_wnd_proc 捕获 WM_LBUTTONDOWN 并 emit "player-click"）
  useEffect(() => {
    const unlisten = listen("player-click", () => {
      togglePlay();
    });
    return () => { unlisten.then((fn) => fn()); };
  }, [togglePlay]);

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
                <span className="truncate">{downloadMessage}</span>
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
          <p className="text-xs">打开视频后在此播放</p>
        </div>
      </div>
    );
  }

  // 有视频 + libmpv 已下载：显示播放器
  return (
    <div className="relative bg-black rounded overflow-hidden">
      {/* 视频渲染区（libmpv 子窗口会覆盖在此区域） */}
      <div
        ref={containerRef}
        className="w-full bg-black flex items-center justify-center"
        style={{ aspectRatio, maxHeight: "40vh" }}
        onClick={() => { if (playerReady) togglePlay(); }}
        title={playerReady ? (playing ? "点击暂停" : "点击播放") : undefined}
      >
        {loadingVideo && (
          <div className="absolute inset-0 flex items-center justify-center text-white/50">
            <Loader2 className="h-8 w-8 animate-spin" />
          </div>
        )}
      </div>

      {/* 独立播放控制器（视频下方，统一主题风格） */}
      {playerReady && (
        <div className="select-none border-t border-border bg-card px-3 pt-2.5 pb-2">
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
              title="进度"
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
              title={playing ? "暂停" : "播放"}
            >
              {playing ? <Pause className="h-4 w-4" /> : <Play className="h-4 w-4 translate-x-[1px]" />}
            </button>

            {/* 音量 */}
            <div className="flex items-center gap-1.5">
              <button
                onClick={toggleMute}
                className="text-muted-foreground transition-colors hover:text-foreground"
                title={volume === 0 ? "取消静音" : "静音"}
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
                title="音量"
              />
            </div>

            {/* 音轨选择（独立 memo 子组件，避免 10Hz position 更新导致下拉关闭） */}
            <AudioTrackSelect
              audioStreams={probeResult.audio_streams}
              audioTrack={audioTrack}
              onChange={handleAudioTrackChange}
            />

            <div className="flex-1" />

            {/* 倍速（分段控件） */}
            <div className="flex items-center gap-0.5 rounded-md bg-muted p-0.5">
              {speedOptions.map((s) => (
                <button
                  key={s}
                  onClick={() => applySpeed(s)}
                  className={`rounded px-1.5 py-0.5 text-[11px] font-medium tabular-nums transition-colors ${
                    speed === s
                      ? "bg-background text-foreground shadow-sm"
                      : "text-muted-foreground hover:text-foreground"
                  }`}
                  title={`${s}x 倍速`}
                >
                  {s}x
                </button>
              ))}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

// === SECTION 3 END ===
