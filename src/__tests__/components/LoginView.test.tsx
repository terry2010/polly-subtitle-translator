// LoginView 组件测试（FP-P0-6）
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { LoginView } from "../../views/LoginView";
import { useAuthStore } from "../../stores/authStore";

const translations: Record<string, string> = {
  "auth.loginTitle": "登录",
  "auth.waitingForLogin": "等待浏览器登录...",
  "auth.browserOpenedHint": "已在浏览器中打开登录页面，请在浏览器中完成登录",
  "auth.offlineMode": "离线模式（网络不可用）",
  "auth.loginFailed": "登录失败",
  "auth.retryLogin": "重新登录",
  "auth.retry": "重试",
  "auth.loginPrompt": "请登录以使用 AI 精译功能",
  "auth.startLogin": "开始登录",
};

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => translations[key] ?? key,
  }),
}));

vi.mock("../../components/ui/button", () => ({
  Button: ({ children, onClick, disabled }: any) => (
    <button onClick={onClick} disabled={disabled}>{children}</button>
  ),
}));

vi.mock("../../components/ui/card", () => ({
  Card: ({ children }: any) => <div data-testid="card">{children}</div>,
  CardHeader: ({ children }: any) => <div>{children}</div>,
  CardTitle: ({ children }: any) => <h2>{children}</h2>,
  CardContent: ({ children }: any) => <div>{children}</div>,
}));

vi.mock("../../lib/api", () => ({
  api: {},
  formatIpcError: (e: any) => e?.code ?? String(e),
}));

const mockLogin = vi.fn(() => Promise.resolve());
const mockClearError = vi.fn();

beforeEach(() => {
  vi.clearAllMocks();
  useAuthStore.setState({
    status: "idle",
    user: null,
    offline: false,
    error: null,
    initialized: false,
    login: mockLogin,
    clearError: mockClearError,
  });
});

// === SECTION 1 END ===

describe("LoginView - idle 状态", () => {
  it("显示登录提示和开始登录按钮", () => {
    render(<LoginView />);
    expect(screen.getByText("请登录以使用 AI 精译功能")).toBeInTheDocument();
    expect(screen.getByText("开始登录")).toBeInTheDocument();
  });

  it("点击开始登录调用 login", () => {
    render(<LoginView />);
    fireEvent.click(screen.getByText("开始登录"));
    expect(mockLogin).toHaveBeenCalledTimes(1);
  });
});

// === SECTION 2 END ===

describe("LoginView - logging_in 状态", () => {
  it("显示等待提示和旋转图标", () => {
    useAuthStore.setState({ status: "logging_in" });
    render(<LoginView />);
    expect(screen.getByText("等待浏览器登录...")).toBeInTheDocument();
    expect(screen.getByText("已在浏览器中打开登录页面，请在浏览器中完成登录")).toBeInTheDocument();
  });
});

// === SECTION 3 END ===

describe("LoginView - offline 状态", () => {
  it("显示离线提示和重新登录按钮", () => {
    useAuthStore.setState({ status: "offline", error: "网络不可用" });
    render(<LoginView />);
    expect(screen.getByText("离线模式（网络不可用）")).toBeInTheDocument();
    expect(screen.getByText("网络不可用")).toBeInTheDocument();
    expect(screen.getByText("重新登录")).toBeInTheDocument();
  });

  it("点击重新登录调用 login", () => {
    useAuthStore.setState({ status: "offline" });
    render(<LoginView />);
    fireEvent.click(screen.getByText("重新登录"));
    expect(mockLogin).toHaveBeenCalledTimes(1);
  });
});

// === SECTION 4 END ===

describe("LoginView - error 状态", () => {
  it("显示错误信息和重试按钮", () => {
    useAuthStore.setState({ status: "error", error: "登录失败原因" });
    render(<LoginView />);
    expect(screen.getByText("登录失败原因")).toBeInTheDocument();
    expect(screen.getByText("重试")).toBeInTheDocument();
  });

  it("点击重试先 clearError 再 login", () => {
    useAuthStore.setState({ status: "error", error: "some error" });
    render(<LoginView />);
    fireEvent.click(screen.getByText("重试"));
    expect(mockClearError).toHaveBeenCalledTimes(1);
    expect(mockLogin).toHaveBeenCalledTimes(1);
  });
});

// === SECTION 5 END ===
