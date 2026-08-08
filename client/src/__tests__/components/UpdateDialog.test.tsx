// UpdateDialog 组件测试
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import { act } from "react";
import userEvent from "@testing-library/user-event";
import { UpdateDialog } from "../../components/UpdateDialog";

const { mockDownloadAndInstall, mockRelaunch, mockUnlisten } = vi.hoisted(() => ({
  mockDownloadAndInstall: vi.fn(),
  mockRelaunch: vi.fn(),
  mockUnlisten: vi.fn(),
}));

let eventHandler: ((event: any) => void) | null = null;

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn((_event: string, handler: (event: any) => void) => {
    eventHandler = handler;
    return Promise.resolve(mockUnlisten);
  }),
}));

vi.mock("@tauri-apps/plugin-process", () => ({
  relaunch: mockRelaunch,
}));

vi.mock("../../lib/api", () => ({
  api: {
    downloadAndInstallUpdate: mockDownloadAndInstall,
  },
  formatIpcError: vi.fn((e: unknown) => String(e)),
}));

function renderDialog(props: { open?: boolean; version?: string; notes?: string } = {}) {
  const onClose = vi.fn();
  const { open = true, version = "1.0.1", notes = "修复 bug" } = props;
  const { unmount } = render(<UpdateDialog open={open} version={version} notes={notes} onClose={onClose} />);
  return { onClose, unmount };
}

beforeEach(() => {
  vi.clearAllMocks();
  eventHandler = null;
  mockDownloadAndInstall.mockResolvedValue(undefined);
  mockRelaunch.mockResolvedValue(undefined);
});

// === SECTION 1 END ===

describe("UpdateDialog - 渲染", () => {
  it("显示新版本信息", () => {
    renderDialog({ version: "2.0.0", notes: "新增功能" });
    expect(screen.getByText("update.title")).toBeInTheDocument();
    expect(screen.getByText("新增功能")).toBeInTheDocument();
  });

  it("注册下载进度监听", () => {
    renderDialog();
    expect(eventHandler).not.toBeNull();
  });

  it("关闭时取消监听", async () => {
    const { unmount } = renderDialog();
    unmount();
    await waitFor(() => {
      expect(mockUnlisten).toHaveBeenCalled();
    });
  });
});

// === SECTION 2 END ===

describe("UpdateDialog - 安装", () => {
  it("点击安装开始下载", async () => {
    const user = userEvent.setup();
    renderDialog();
    const installButton = screen.getByRole("button", { name: "update.installNow" });
    await user.click(installButton);
    expect(mockDownloadAndInstall).toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "update.downloading" })).toBeDisabled();
  });

  it("下载进度更新 UI", async () => {
    const user = userEvent.setup();
    renderDialog();
    const installButton = screen.getByRole("button", { name: "update.installNow" });
    await user.click(installButton);
    await act(async () => {
      eventHandler?.({ payload: { stage: "downloading", progress: 50, message: "", speed_mbps: 1.5, eta_secs: 30 } });
    });
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "update.downloading" })).toBeInTheDocument();
    });
  });

  it("下载完成显示重启按钮", async () => {
    const user = userEvent.setup();
    renderDialog();
    const installButton = screen.getByText("update.installNow");
    await user.click(installButton);
    await act(async () => {
      eventHandler?.({ payload: { stage: "done", progress: 100 } });
    });
    await waitFor(() => {
      expect(screen.getByText("update.relaunch")).toBeInTheDocument();
    });
  });

  it("下载失败显示重试按钮", async () => {
    mockDownloadAndInstall.mockRejectedValue(new Error("network error"));
    const user = userEvent.setup();
    renderDialog();
    const installButton = screen.getByText("update.installNow");
    await user.click(installButton);
    await waitFor(() => {
      expect(screen.getByText("update.retry")).toBeInTheDocument();
    });
  });

  it("点击重启调用 relaunch", async () => {
    const user = userEvent.setup();
    renderDialog();
    await user.click(screen.getByText("update.installNow"));
    await act(async () => {
      eventHandler?.({ payload: { stage: "done", progress: 100 } });
    });
    await waitFor(() => screen.getByText("update.relaunch"));
    await user.click(screen.getByText("update.relaunch"));
    expect(mockRelaunch).toHaveBeenCalled();
  });
});

// === SECTION 3 END ===
