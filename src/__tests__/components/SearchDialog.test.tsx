// SearchDialog 组件测试
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SearchDialog } from "../../components/SearchDialog";

const { mockOpenUrl, mockPlayerHide, mockPlayerShow, mockSimplifyKeyword } = vi.hoisted(() => ({
  mockOpenUrl: vi.fn(),
  mockPlayerHide: vi.fn(),
  mockPlayerShow: vi.fn(),
  mockSimplifyKeyword: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-shell", () => ({
  open: mockOpenUrl,
}));

vi.mock("../../lib/api", () => ({
  api: {
    playerHide: mockPlayerHide,
    playerShow: mockPlayerShow,
    devLog: vi.fn(),
    simplifySearchKeyword: mockSimplifyKeyword,
    devLog: vi.fn(() => Promise.resolve()),
  },
  formatIpcError: vi.fn((e: unknown) => String(e)),
}));

function renderDialog(props: { open?: boolean; videoName?: string } = {}) {
  const onOpenChange = vi.fn();
  const { open = true, videoName } = props;
  render(<SearchDialog open={open} onOpenChange={onOpenChange} videoName={videoName} />);
  return { onOpenChange };
}

beforeEach(() => {
  vi.clearAllMocks();
  mockPlayerHide.mockResolvedValue(undefined);
  mockPlayerShow.mockResolvedValue(undefined);
  mockSimplifyKeyword.mockResolvedValue("Simplified Name");
  mockOpenUrl.mockResolvedValue(undefined);
});

// === SECTION 1 END ===

describe("SearchDialog - 渲染", () => {
  it("渲染搜索标题", () => {
    renderDialog();
    expect(screen.getByText("search.title")).toBeInTheDocument();
  });

  it("打开弹窗时隐藏播放器子窗口", () => {
    renderDialog();
    expect(mockPlayerHide).toHaveBeenCalled();
  });

  it("有 videoName 时自动简化关键词", async () => {
    renderDialog({ videoName: "Movie.Name.2024.1080p.mkv" });
    await waitFor(() => {
      expect(mockSimplifyKeyword).toHaveBeenCalledWith("Movie.Name.2024.1080p.mkv");
    });
    expect(screen.getByDisplayValue("Simplified Name")).toBeInTheDocument();
  });

  it("简化关键词失败时回退原值", async () => {
    mockSimplifyKeyword.mockRejectedValue(new Error("fail"));
    renderDialog({ videoName: "Movie.Name.2024.mkv" });
    await waitFor(() => {
      expect(screen.getByDisplayValue("Movie.Name.2024.mkv")).toBeInTheDocument();
    });
  });
});

// === SECTION 2 END ===

describe("SearchDialog - 搜索源", () => {
  it("默认选中 opensubtitles", () => {
    renderDialog();
    expect(screen.getByText("search.source.opensubtitles")).toBeInTheDocument();
  });

  it("切换搜索源", async () => {
    const user = userEvent.setup();
    renderDialog();
    const subhd = screen.getByText("search.source.subhd");
    await user.click(subhd);
    expect(subhd).toBeInTheDocument();
  });
});

// === SECTION 3 END ===

describe("SearchDialog - 搜索行为", () => {
  it("输入空查询不触发搜索", async () => {
    const user = userEvent.setup();
    renderDialog();
    const button = screen.getByRole("button", { name: "" });
    await user.click(button);
    expect(mockOpenUrl).not.toHaveBeenCalled();
  });

  it("点击搜索按钮打开对应 URL", async () => {
    const user = userEvent.setup();
    renderDialog({ videoName: "test" });
    const button = screen.getByRole("button", { name: "" });
    await user.click(button);
    expect(mockOpenUrl).toHaveBeenCalledWith(
      expect.stringContaining("opensubtitles.com/search?q=Simplified%20Name"),
    );
  });

  it("按 Enter 触发搜索", async () => {
    const user = userEvent.setup();
    renderDialog({ videoName: "test" });
    const input = await waitFor(() => screen.getByDisplayValue("Simplified Name"));
    await user.type(input, "{enter}");
    expect(mockOpenUrl).toHaveBeenCalled();
  });

  it("搜索出错时显示错误", async () => {
    mockOpenUrl.mockRejectedValue(new Error("open failed"));
    const user = userEvent.setup();
    renderDialog({ videoName: "test" });
    const button = screen.getByRole("button", { name: "" });
    await user.click(button);
    await waitFor(() => {
      expect(screen.getByText("Error: open failed")).toBeInTheDocument();
    });
  });
});

// === SECTION 4 END ===
