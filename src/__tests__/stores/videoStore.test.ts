// videoStore 单元测试
import { describe, it, expect, beforeEach, vi } from "vitest";
import { useVideoStore } from "../../stores/videoStore";
import type { ProbeResult, SubtitleStream } from "../../lib/ipc-types";

const { mockProbeVideo } = vi.hoisted(() => ({
  mockProbeVideo: vi.fn(),
}));

vi.mock("../../lib/api", () => ({
  api: { probeVideo: mockProbeVideo },
  formatIpcError: vi.fn((e: unknown) => String(e)),
}));

function makeSub(index: number, lang: string, opts: Partial<SubtitleStream> = {}): SubtitleStream {
  return {
    index, codec_name: "subrip", codec_long_name: "SubRip",
    duration: null, language: lang, title: null,
    disposition_default: false, disposition_forced: false,
    disposition_hearing_impaired: false, is_graphic: false,
    ...opts,
  };
}

function makeProbe(subs: SubtitleStream[]): ProbeResult {
  return {
    video_path: "/test/video.mkv",
    format: { format_name: "matroska", format_long_name: "Matroska", duration: 120, size: 1000, bit_rate: 8000 },
    video_stream: null,
    audio_streams: [],
    subtitle_streams: subs,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  useVideoStore.setState({ probeResult: null, loading: false, error: null, selectedSubtitleStream: null });
});

// === SECTION 1 END ===

describe("videoStore - openVideo", () => {
  it("成功探测并自动选择英文字幕流", async () => {
    const subs = [makeSub(0, "spa"), makeSub(1, "eng")];
    mockProbeVideo.mockResolvedValue(makeProbe(subs));
    await useVideoStore.getState().openVideo("/test/video.mkv");
    const state = useVideoStore.getState();
    expect(state.probeResult).toBeTruthy();
    expect(state.loading).toBe(false);
    expect(state.selectedSubtitleStream?.index).toBe(1);
  });

  it("优先选择英文 SDH 字幕流", async () => {
    const subs = [
      makeSub(0, "eng"),
      makeSub(1, "eng", { disposition_hearing_impaired: true }),
    ];
    mockProbeVideo.mockResolvedValue(makeProbe(subs));
    await useVideoStore.getState().openVideo("/test/video.mkv");
    expect(useVideoStore.getState().selectedSubtitleStream?.index).toBe(1);
  });

  it("SDH 标题匹配（title 含 SDH）", async () => {
    const subs = [
      makeSub(0, "eng"),
      makeSub(1, "eng", { title: "English SDH" }),
    ];
    mockProbeVideo.mockResolvedValue(makeProbe(subs));
    await useVideoStore.getState().openVideo("/test/video.mkv");
    expect(useVideoStore.getState().selectedSubtitleStream?.index).toBe(1);
  });

  it("无英文字幕时选第一条非图形字幕", async () => {
    const subs = [makeSub(0, "spa"), makeSub(1, "jpn")];
    mockProbeVideo.mockResolvedValue(makeProbe(subs));
    await useVideoStore.getState().openVideo("/test/video.mkv");
    expect(useVideoStore.getState().selectedSubtitleStream?.index).toBe(0);
  });

  it("图形字幕被排除", async () => {
    const subs = [
      makeSub(0, "eng", { is_graphic: true, codec_name: "hdmv_pgs_subtitle" }),
      makeSub(1, "eng", { is_graphic: false }),
    ];
    mockProbeVideo.mockResolvedValue(makeProbe(subs));
    await useVideoStore.getState().openVideo("/test/video.mkv");
    expect(useVideoStore.getState().selectedSubtitleStream?.index).toBe(1);
  });

  it("无字幕流时 selectedSubtitleStream 为 null", async () => {
    mockProbeVideo.mockResolvedValue(makeProbe([]));
    await useVideoStore.getState().openVideo("/test/video.mkv");
    expect(useVideoStore.getState().selectedSubtitleStream).toBeNull();
  });

  it("探测失败设置 error", async () => {
    mockProbeVideo.mockRejectedValue(new Error("FFmpeg not found"));
    await useVideoStore.getState().openVideo("/test/video.mkv");
    expect(useVideoStore.getState().error).toBeTruthy();
    expect(useVideoStore.getState().loading).toBe(false);
  });
});

// === SECTION 2 END ===

describe("videoStore - selectSubtitleStream", () => {
  it("手动选择字幕流", () => {
    const sub = makeSub(2, "fra");
    useVideoStore.getState().selectSubtitleStream(sub);
    expect(useVideoStore.getState().selectedSubtitleStream?.index).toBe(2);
  });

  it("取消选择（传 null）", () => {
    useVideoStore.setState({ selectedSubtitleStream: makeSub(0, "eng") });
    useVideoStore.getState().selectSubtitleStream(null);
    expect(useVideoStore.getState().selectedSubtitleStream).toBeNull();
  });
});

// === SECTION 3 END ===

describe("videoStore - clearVideo", () => {
  it("清空所有视频状态", () => {
    useVideoStore.setState({
      probeResult: makeProbe([]),
      selectedSubtitleStream: makeSub(0, "eng"),
      error: "some error",
    });
    useVideoStore.getState().clearVideo();
    expect(useVideoStore.getState().probeResult).toBeNull();
    expect(useVideoStore.getState().selectedSubtitleStream).toBeNull();
    expect(useVideoStore.getState().error).toBeNull();
  });
});

// === SECTION 4 END ===
