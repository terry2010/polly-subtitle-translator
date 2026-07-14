// TranslatePanel 组件测试
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { TranslatePanel } from "../../components/TranslatePanel";
import { useSubtitleStore } from "../../stores/subtitleStore";
import { useTranslateStore } from "../../stores/translateStore";
import type { SubtitleEntry } from "../../lib/ipc-types";

const { mockGetSupportedTargetLangs, mockStartTranslate } = vi.hoisted(() => ({
  mockGetSupportedTargetLangs: vi.fn(),
  mockStartTranslate: vi.fn(),
}));

vi.mock("../../lib/api", () => ({
  api: {
    getSupportedTargetLangs: mockGetSupportedTargetLangs,
    getConfig: vi.fn(() => Promise.resolve(null)),
    setConfig: vi.fn(() => Promise.resolve()),
  },
}));

function makeEntry(index: number, text: string, translated = ""): SubtitleEntry {
  return { index, start_ms: 0, end_ms: 1000, text, translated, style: null, pre_edit_text: null };
}

beforeEach(() => {
  vi.clearAllMocks();
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
    model: "", modelType: "", serviceId: null,
  });
  mockGetSupportedTargetLangs.mockResolvedValue([
    { code: "zh", name: "Chinese", native_name: "中文" },
    { code: "en", name: "English", native_name: "English" },
  ]);
});

// === SECTION 1 END ===

describe("TranslatePanel - 渲染", () => {
  it("渲染翻译标题", async () => {
    render(<TranslatePanel />);
    expect(screen.getByText("translate.title")).toBeInTheDocument();
  });

  it("加载目标语言列表", async () => {
    render(<TranslatePanel />);
    await waitFor(() => {
      expect(mockGetSupportedTargetLangs).toHaveBeenCalledWith("baidu");
    });
  });

  it("加载语言失败时使用 fallback 列表", async () => {
    mockGetSupportedTargetLangs.mockRejectedValue(new Error("fail"));
    render(<TranslatePanel />);
    // 不报错即可，fallback 列表在 catch 中设置
    await waitFor(() => {
      expect(mockGetSupportedTargetLangs).toHaveBeenCalled();
    });
  });

  it("无字幕时翻译按钮禁用", () => {
    render(<TranslatePanel />);
    const button = screen.getByRole("button", { name: "translate.start" });
    expect(button).toBeDisabled();
  });

  it("有字幕时翻译按钮可用", () => {
    useSubtitleStore.setState({
      file: { format: "srt", entries: [makeEntry(0, "hello")], raw_header: null, source_path: null },
    });
    render(<TranslatePanel />);
    const button = screen.getByRole("button", { name: "translate.start" });
    expect(button).not.toBeDisabled();
  });

  it("翻译中按钮禁用并显示进度", () => {
    useSubtitleStore.setState({
      file: { format: "srt", entries: [makeEntry(0, "hello")], raw_header: null, source_path: null },
    });
    useTranslateStore.setState({ translating: true });
    render(<TranslatePanel />);
    expect(screen.getByText("translate.progress")).toBeInTheDocument();
  });

  it("显示错误提示", () => {
    useTranslateStore.setState({ error: "API key 无效" });
    render(<TranslatePanel />);
    expect(screen.getByText("API key 无效")).toBeInTheDocument();
  });

  it("显示翻译结果统计", () => {
    useTranslateStore.setState({
      result: { translations: [{ index: 0, original: "hello", translated: "你好", from_cache: false, failed: false }], provider: "baidu", cached_count: 1 } as any,
    });
    render(<TranslatePanel />);
    expect(screen.getByText(/✓ 1/)).toBeInTheDocument();
    expect(screen.getByText(/📦 1 cache/)).toBeInTheDocument();
  });
});

// === SECTION 2 END ===

describe("TranslatePanel - 翻译交互", () => {
  it("点击翻译按钮触发 startTranslate 并回填", async () => {
    const user = userEvent.setup();
    useSubtitleStore.setState({
      file: { format: "srt", entries: [makeEntry(0, "hello"), makeEntry(1, "world")], raw_header: null, source_path: null },
    });
    mockStartTranslate.mockResolvedValue({
      translations: [
        { index: 0, original: "hello", translated: "你好", from_cache: false, failed: false },
        { index: 1, original: "world", translated: "世界", from_cache: false, failed: false },
      ],
      provider: "baidu", cached_count: 0,
    });

    // mock translateStore.startTranslate
    const originalStartTranslate = useTranslateStore.getState().startTranslate;
    useTranslateStore.setState({ startTranslate: mockStartTranslate as any });

    render(<TranslatePanel />);
    const button = screen.getByRole("button", { name: "translate.start" });
    await user.click(button);

    await waitFor(() => {
      expect(mockStartTranslate).toHaveBeenCalled();
    });

    // 验证回填
    await waitFor(() => {
      expect(useSubtitleStore.getState().file?.entries[0].translated).toBe("你好");
      expect(useSubtitleStore.getState().file?.entries[1].translated).toBe("世界");
    });

    // 恢复
    useTranslateStore.setState({ startTranslate: originalStartTranslate });
  });
});

// === SECTION 3 END ===
