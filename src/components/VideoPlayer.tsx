// libmpv 内嵌视频播放器组件
// 对应需求文档 §3.6 F-09：libmpv 子窗口嵌入 + 播控条 + 位置事件联动
import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "../lib/api";
import type { LibmpvStatus, ProbeResult } from "../lib/ipc-types";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Film, Play, Pause, Loader2, Download, Volume2, VolumeX } from "lucide-react";
import { Button } from "./ui/button";

interface VideoPlayerProps {
  probeResult: ProbeResult | null;
  onPositionUpdate?: (positionSec: number, durationSec: number, paused: boolean) => void;
}

// === SECTION 1 END ===

export function VideoPlayer({ probeResult, onPositionUpdate }: VideoPlayerProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [libmpvStatus, setLibmpvStatus] = useState<LibmpvStatus | null>(null);
  const [downloading, setDownloading] = useState(false);
  const [downloadProgress, setDownloadProgress] = useState(0);
  const [downloadStage, setDownloadStage] = useState<string>("");
  const [downloadMessage, setDownloadMessage] = useState<string>("");
  const [playerReady, setPlayerReady] = useState(false);
  const [playing, setPlaying] = useState(false);
  const [position, setPosition] = useState(0);
  const [duration, setDuration] = useState(0);
  const [volume, setVolume] = useState(100);
  const [speed, setSpeed] = useState(1);
  const [loadingVideo, setLoadingVideo] = useState(false);
  const speedOptions = [0.5, 0.75, 1, 1.25, 1.5, 2];

  // 获取 libmpv 下载状态
  const checkLibmpvStatus = useCallback(async () => {
    try {
      const status = await api.getLibmpvStatus();
      setLibmpvStatus(status);
    } catch (e) {
      console.error("获取 libmpv 状态失败:", e);
    }
  }, []);

  useEffect(() => {
    checkLibmpvStatus();
  }, [checkLibmpvStatus]);

  // 监听下载进度事件
  useEffect(() => {
    const unlisten = listen<{
      stage: string;
      progress: number;
      message?: string;
      downloaded?: number;
      total?: number;
    }>("libmpv_download_progress", (event) => {
      const { stage, progress, message } = event.payload;
      setDownloadProgress(progress);
      setDownloadStage(stage);
      if (message) setDownloadMessage(message);
    });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  // 下载 libmpv
  const handleDownload = useCallback(async () => {
    setDownloading(true);
    setDownloadProgress(0);
    setDownloadStage("fetching");
    setDownloadMessage("正在获取最新版本信息...");
    try {
      await api.downloadLibmpv();
      await checkLibmpvStatus();
    } catch (e) {
      console.error("下载 libmpv 失败:", e);
    } finally {
      setDownloading(false);
    }
  }, [checkLibmpvStatus]);

  // 初始化播放器并加载视频
  const initAndLoad = useCallback(async (videoPath: string, dllPath: string) => {
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
    setLoadingVideo(true);
    setPlayerReady(false);
    // 先销毁旧播放器
    api.playerDestroy().catch(() => {});
    // 延迟一帧让 DOM 更新后获取正确坐标
    const timer = setTimeout(() => {
      initAndLoad(probeResult.video_path, libmpvStatus.path!);
    }, 100);
    return () => clearTimeout(timer);
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
    const stageLabel = downloadStage === "fetching" ? "获取版本信息"
      : downloadStage === "downloading" ? "下载中"
      : downloadStage === "extracting" ? "解压安装"
      : downloadStage === "done" ? "完成"
      : "准备中";
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
              <p className="text-xs text-white/40 truncate">{downloadMessage}</p>
            </>
          ) : (
            <>
              <p className="text-xs mb-4">需要下载播放组件（约 30MB）</p>
              <Button size="sm" onClick={handleDownload}>
                <Download className="mr-1 h-4 w-4" />下载播放组件
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
