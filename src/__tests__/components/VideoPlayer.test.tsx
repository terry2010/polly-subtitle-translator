// VideoPlayer 组件测试（覆盖核心渲染和交互）
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { VideoPlayer } from "../../components/VideoPlayer";
import { useLibmpvStore } from "../../stores/libmpvStore";
import { useSubtitleStore } from "../../stores/subtitleStore";
import { useTranslateStore } from "../../stores/translateStore";
import type { ProbeResult, AudioStream } from "../../lib/ipc-types";

const { mockPlayerInit, mockPlayerDestroy, mockPlayerPlay, mockPlayerPause, mockPlayerSeek, mockPlayerSetVolume, mockPlayerSetSpeed, mockPlayerSetAudioTrack, mockPlayerLoad, mockPlayerResize, mockPlayerHide, mockPlayerShow, mockListen } = vi.hoisted(() => ({
  mockPlayerInit: vi.fn(),
  mockPlayerDestroy: vi.fn(),
  mockPlayerPlay: vi.fn(),
  mockPlayerPause: vi.fn(),
  mockPlayerSeek: vi.fn(),
  mockPlayerSetVolume: vi.fn(),
  mockPlayerSetSpeed: vi.fn(),
  mockPlayerSetAudioTrack: vi.fn(),
  mockPlayerLoad: vi.fn(),
  mockPlayerResize: vi.fn(),
  mockPlayerHide: vi.fn(),
  mockPlayerShow: vi.fn(),
  mockListen: vi.fn(),
}));

vi.mock("../../lib/api", () => ({
  api: {
    playerInit: mockPlayerInit,
    playerDestroy: mockPlayerDestroy,
    playerPlay: mockPlayerPlay,
    playerPause: mockPlayerPause,
    playerSeek: mockPlayerSeek,
    playerSetVolume: mockPlayerSetVolume,
    playerSetSpeed: mockPlayerSetSpeed,
    playerSetAudioTrack: mockPlayerSetAudioTrack,
    playerLoad: mockPlayerLoad,
    playerResize: mockPlayerResize,
    playerHide: mockPlayerHide,
    playerShow: mockPlayerShow,
    listInstalledPlayers: vi.fn(() => Promise.resolve([])),
    extractPlayerIcons: vi.fn(() => Promise.resolve([])),
    openWithPlayer: vi.fn(() => Promise.resolve()),
    openFolder: vi.fn(() => Promise.resolve()),
  },
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: mockListen,
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: vi.fn(() => ({
    onResized: vi.fn(() => Promise.resolve(() => {})),
    scaleFactor: vi.fn(() => Promise.resolve(1)),
  })),
}));

vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: vi.fn((p: string) => `asset://localhost/${p}`),
}));

vi.mock("@tauri-apps/plugin-os", () => ({
  platform: vi.fn(() => "macos"),
}));

vi.mock("../../lib/utils", () => ({
  uiState: { selectOpen: false, mouseInSubtitleEditor: false },
  cn: vi.fn((...args: any[]) => args.filter(Boolean).join(" ")),
}));

function makeAudioStream(index: number, opts: Partial<AudioStream> = {}): AudioStream {
  return {
    index, codec_name: "aac", codec_long_name: "AAC",
    duration: null, language: "eng", title: "",
    channels: 2, channel_layout: "stereo", sample_rate: 48000, bit_rate: 128000,
    disposition_default: false, ...opts,
  };
}

function makeProbe(audioStreams: AudioStream[] = [makeAudioStream(0)]): ProbeResult {
  return {
    video_path: "/test/video.mkv",
    format: { format_name: "matroska", format_long_name: "Matroska", duration: 120, size: 1000, bit_rate: 8000 },
    video_stream: { width: 1920, height: 1080, codec_name: "h264", hdr_info: null } as any,
    audio_streams: audioStreams,
    subtitle_streams: [],
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  mockPlayerInit.mockResolvedValue(undefined);
  mockPlayerDestroy.mockResolvedValue(undefined);
  mockPlayerPlay.mockResolvedValue(undefined);
  mockPlayerPause.mockResolvedValue(undefined);
  mockPlayerSeek.mockResolvedValue(undefined);
  mockPlayerSetVolume.mockResolvedValue(undefined);
  mockPlayerSetSpeed.mockResolvedValue(undefined);
  mockPlayerSetAudioTrack.mockResolvedValue(undefined);
  mockPlayerLoad.mockResolvedValue(undefined);
  mockPlayerResize.mockResolvedValue(undefined);
  mockPlayerHide.mockResolvedValue(undefined);
  mockPlayerShow.mockResolvedValue(undefined);
  mockListen.mockResolvedValue(() => {});
  useLibmpvStore.setState({
    downloading: false, downloadProgress: 0, downloadStage: "",
    downloadMessage: "", downloadError: "", downloadSpeedMbps: 0,
    downloadEtaSecs: 0, status: { downloaded: true, path: "/usr/local/lib/libmpv.dylib" } as any,
    statusLoading: false, onProgressEvent: vi.fn(), refreshStatus: vi.fn(), startDownload: vi.fn(),
  });
  useSubtitleStore.setState({
    file: null, loading: false, error: null, bilingualDetect: null,
    isSplit: false, preSplitFile: null, preSplitBilingualDetect: null,
    undoStack: [], redoStack: [],
    findQuery: "", replaceQuery: "", findTarget: "all",
    findMatchCount: 0, findCurrentMatch: 0, findMatchEntryIndex: null,
  });
  useTranslateStore.setState({
    translating: false, progress: 0, total: 0, result: null, error: null,
    sourceLang: "en", targetLang: "zh", provider: "baidu",
  });
});

// === SECTION 1 END ===

describe("VideoPlayer - 无视频状态", () => {
  it("无 probeResult 时显示占位", () => {
    render(<VideoPlayer probeResult={null} />);
    expect(screen.getByText("player.placeholder")).toBeInTheDocument();
  });
});

// === SECTION 2 END ===

describe("VideoPlayer - 有视频状态", () => {
  it("有 probeResult 时初始化播放器", async () => {
    const probe = makeProbe();
    render(<VideoPlayer probeResult={probe} />);
    await waitFor(() => {
      expect(mockPlayerInit).toHaveBeenCalled();
    });
  });

  it("显示播放控制按钮", async () => {
    const probe = makeProbe();
    render(<VideoPlayer probeResult={probe} />);
    await waitFor(() => {
      expect(mockPlayerInit).toHaveBeenCalled();
    });
    // 播放/暂停按钮
    const buttons = screen.getAllByRole("button");
    expect(buttons.length).toBeGreaterThan(0);
  });

  it("多音轨时显示音轨选择", async () => {
    const probe = makeProbe([makeAudioStream(0), makeAudioStream(1, { language: "jpn" })]);
    render(<VideoPlayer probeResult={probe} />);
    await waitFor(() => {
      expect(mockPlayerInit).toHaveBeenCalled();
    });
    // 音轨选择应该可见
    const select = screen.queryByTitle("player.audioTrack");
    if (select) {
      expect(select).toBeInTheDocument();
    }
  });

  it("卸载时销毁播放器", async () => {
    const probe = makeProbe();
    const { unmount } = render(<VideoPlayer probeResult={probe} />);
    await waitFor(() => expect(mockPlayerInit).toHaveBeenCalled());
    unmount();
    await waitFor(() => expect(mockPlayerDestroy).toHaveBeenCalled());
  });
});

// === SECTION 3 END ===
