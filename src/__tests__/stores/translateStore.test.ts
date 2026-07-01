// translateStore 单元测试
import { describe, it, expect, beforeEach, vi } from "vitest";
import { useTranslateStore } from "../../stores/translateStore";
import type { SubtitleEntry } from "../../lib/ipc-types";

// mock api — vi.hoisted 确保 mock 函数在 vi.mock 提升时已初始化
const { mockTranslateSubtitle, mockCancelTranslate, mockOnTranslateProgress, mockOnTranslateEntryDone } = vi.hoisted(() => ({
  mockTranslateSubtitle: vi.fn(),
  mockCancelTranslate: vi.fn(),
  mockOnTranslateProgress: vi.fn(),
  mockOnTranslateEntryDone: vi.fn(),
}));

vi.mock("../../lib/api", () => ({
  api: {
    translateSubtitle: mockTranslateSubtitle,
    cancelTranslate: mockCancelTranslate,
    onTranslateProgress: mockOnTranslateProgress,
    onTranslateEntryDone: mockOnTranslateEntryDone,
  },
  formatIpcError: vi.fn((e: unknown) => String(e)),
}));

function makeEntry(index: number, text: string): SubtitleEntry {
  return { index, start_ms: 0, end_ms: 1000, text, translated: "", style: null };
}

beforeEach(() => {
  vi.clearAllMocks();
  useTranslateStore.setState({
    translating: false, progress: 0, total: 0, result: null, error: null,
    sourceLang: "en", targetLang: "zh", provider: "baidu",
  });
  mockOnTranslateProgress.mockResolvedValue(() => {});
  mockOnTranslateEntryDone.mockResolvedValue(() => {});
});

// === SECTION 1 END ===

describe("translateStore - 设置", () => {
  it("setSourceLang", () => {
    useTranslateStore.getState().setSourceLang("ja");
    expect(useTranslateStore.getState().sourceLang).toBe("ja");
  });

  it("setTargetLang", () => {
    useTranslateStore.getState().setTargetLang("ko");
    expect(useTranslateStore.getState().targetLang).toBe("ko");
  });

  it("setProvider", () => {
    useTranslateStore.getState().setProvider("google");
    expect(useTranslateStore.getState().provider).toBe("google");
  });
});

// === SECTION 2 END ===

describe("translateStore - startTranslate", () => {
  it("成功翻译返回结果", async () => {
    const entries = [makeEntry(0, "hello"), makeEntry(1, "world")];
    const mockResult = {
      translations: [
        { index: 0, original: "hello", translated: "你好", from_cache: false, failed: false },
        { index: 1, original: "world", translated: "世界", from_cache: false, failed: false },
      ],
      provider: "baidu", cached_count: 0,
    };
    mockTranslateSubtitle.mockResolvedValue(mockResult);

    const result = await useTranslateStore.getState().startTranslate(entries);
    expect(result).toEqual(mockResult);
    expect(useTranslateStore.getState().translating).toBe(false);
    expect(useTranslateStore.getState().progress).toBe(2);
  });

  it("翻译中不允许启动新任务", async () => {
    const warnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});
    useTranslateStore.setState({ translating: true });
    const entries = [makeEntry(0, "hello")];
    const result = await useTranslateStore.getState().startTranslate(entries);
    expect(result).toBeNull();
    expect(mockTranslateSubtitle).not.toHaveBeenCalled();
    expect(warnSpy).toHaveBeenCalledWith("翻译正在进行中，跳过新任务");
    warnSpy.mockRestore();
  });

  it("翻译失败设置 error", async () => {
    mockTranslateSubtitle.mockRejectedValue(new Error("网络错误"));
    const entries = [makeEntry(0, "hello")];
    const result = await useTranslateStore.getState().startTranslate(entries);
    expect(result).toBeNull();
    expect(useTranslateStore.getState().error).toBeTruthy();
    expect(useTranslateStore.getState().translating).toBe(false);
  });

  it("onEntryDone 回调被注册", async () => {
    mockTranslateSubtitle.mockResolvedValue({ translations: [], provider: "baidu", cached_count: 0 });
    const onEntryDone = vi.fn();
    await useTranslateStore.getState().startTranslate([makeEntry(0, "a")], onEntryDone);
    expect(mockOnTranslateEntryDone).toHaveBeenCalledWith(expect.any(Function));
  });
});

// === SECTION 3 END ===

describe("translateStore - cancelTranslate", () => {
  it("调用 cancelTranslate IPC", async () => {
    mockCancelTranslate.mockResolvedValue(undefined);
    useTranslateStore.setState({ translating: true });
    await useTranslateStore.getState().cancelTranslate();
    expect(mockCancelTranslate).toHaveBeenCalled();
    expect(useTranslateStore.getState().translating).toBe(false);
  });
});

// === SECTION 4 END ===

describe("translateStore - reset", () => {
  it("重置所有状态", () => {
    useTranslateStore.setState({ progress: 5, total: 10, error: "err", result: {} as any });
    useTranslateStore.getState().reset();
    const state = useTranslateStore.getState();
    expect(state.progress).toBe(0);
    expect(state.total).toBe(0);
    expect(state.result).toBeNull();
    expect(state.error).toBeNull();
  });
});

// === SECTION 5 END ===
