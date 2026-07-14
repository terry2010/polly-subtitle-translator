// SettingsView TranslateApiSettings 组件测试
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { TranslateApiSettings } from "../../views/SettingsView";
import { useDevModeStore } from "../../stores/devModeStore";

const {
  mockGetConfig,
  mockSetConfig,
  mockGetCredential,
  mockSaveCredential,
  mockDeleteCredential,
  mockGetTranslateUseProxy,
  mockGetProxy,
  mockListOpenaiModels,
  mockTestTranslateConnection,
  mockOpenUrl,
} = vi.hoisted(() => ({
  mockGetConfig: vi.fn(),
  mockSetConfig: vi.fn(),
  mockGetCredential: vi.fn(),
  mockSaveCredential: vi.fn(),
  mockDeleteCredential: vi.fn(),
  mockGetTranslateUseProxy: vi.fn(),
  mockGetProxy: vi.fn(),
  mockListOpenaiModels: vi.fn(),
  mockTestTranslateConnection: vi.fn(),
  mockOpenUrl: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-shell", () => ({
  open: mockOpenUrl,
}));

vi.mock("../../lib/api", () => ({
  api: {
    getConfig: mockGetConfig,
    setConfig: mockSetConfig,
    getCredential: mockGetCredential,
    saveCredential: mockSaveCredential,
    deleteCredential: mockDeleteCredential,
    getTranslateUseProxy: mockGetTranslateUseProxy,
    getProxy: mockGetProxy,
    listOpenaiModels: mockListOpenaiModels,
    testTranslateConnection: mockTestTranslateConnection,
  },
  formatIpcError: vi.fn((e: unknown) => String(e)),
}));

// Mock Tauri window API
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    scaleFactor: () => Promise.resolve(1),
    innerSize: () => Promise.resolve({ width: 1280, height: 800 }),
    outerSize: () => Promise.resolve({ width: 1280, height: 800 }),
    outerPosition: () => Promise.resolve({ x: 0, y: 0 }),
    setSize: () => Promise.resolve(),
    setPosition: () => Promise.resolve(),
  }),
  LogicalSize: class {},
  LogicalPosition: class {},
}));

function renderComponent() {
  // 创建一个容器 div 供 portal 渲染 API 列表
  const containerDiv = document.createElement("div");
  containerDiv.className = "w-48 flex flex-col border-r overflow-hidden flex-shrink-0";
  document.body.appendChild(containerDiv);

  const utils = render(
    <MemoryRouter>
      <TranslateApiSettings listContainer={containerDiv} />
    </MemoryRouter>
  );

  return {
    ...utils,
    cleanupContainer: () => {
      document.body.removeChild(containerDiv);
    },
  };
}

// === SECTION 1 END ===

beforeEach(() => {
  vi.clearAllMocks();
  useDevModeStore.setState({ devMode: false });
  // 默认所有 config 返回 null（未配置）
  mockGetConfig.mockResolvedValue(null);
  mockGetCredential.mockResolvedValue(null);
  mockSetConfig.mockResolvedValue(undefined);
  mockSaveCredential.mockResolvedValue(undefined);
  mockDeleteCredential.mockResolvedValue(undefined);
  mockGetTranslateUseProxy.mockResolvedValue(null);
  mockGetProxy.mockResolvedValue({ mode: "none", host: "", port: "", username: "", hasPassword: false });
  mockListOpenaiModels.mockResolvedValue([]);
  mockTestTranslateConnection.mockResolvedValue({ original: null, translated: null });
  mockOpenUrl.mockResolvedValue(undefined);
});

// === SECTION 2 END ===

describe("TranslateApiSettings - 左列表渲染", () => {
  it("默认显示官方 API 面板", async () => {
    useDevModeStore.setState({ devMode: true });
    renderComponent();
    await waitFor(() => {
      // 官方 API 同时出现在左列表和右面板标题
      expect(screen.getAllByText("settings.officialApi").length).toBeGreaterThanOrEqual(2);
    });
  });

  it("左列表显示官方 API 卡片", async () => {
    useDevModeStore.setState({ devMode: true });
    renderComponent();
    await waitFor(() => {
      expect(screen.getAllByText("settings.officialApi").length).toBeGreaterThanOrEqual(1);
    });
  });

  it("左列表显示添加 API 卡片（传统翻译 + AI 大模型）", async () => {
    renderComponent();
    await waitFor(() => {
      const addButtons = screen.getAllByText("settings.addApi");
      expect(addButtons.length).toBeGreaterThanOrEqual(2);
    });
  });

  it("官方 API 面板显示三个功能介绍", async () => {
    useDevModeStore.setState({ devMode: true });
    renderComponent();
    await waitFor(() => {
      expect(screen.getByText("settings.officialApiJingyi")).toBeInTheDocument();
      expect(screen.getByText("settings.officialApiZhongzhuan")).toBeInTheDocument();
      expect(screen.getByText("settings.officialApiMianfei")).toBeInTheDocument();
    });
  });

  it("官方 API 面板显示登陆按钮", async () => {
    useDevModeStore.setState({ devMode: true });
    renderComponent();
    await waitFor(() => {
      expect(screen.getByText("settings.officialApiLoginButton")).toBeInTheDocument();
    });
  });

  it("非开发者模式下隐藏官方 API 卡片", async () => {
    useDevModeStore.setState({ devMode: false });
    renderComponent();
    await waitFor(() => {
      expect(screen.queryByText("settings.officialApi")).not.toBeInTheDocument();
    });
  });
});

// === SECTION 3 END ===

describe("TranslateApiSettings - 添加 API 面板", () => {
  it("点击添加传统翻译 API 显示未配置服务列表", async () => {
    const user = userEvent.setup();
    renderComponent();
    await waitFor(() => {
      expect(screen.getAllByText("settings.addApi").length).toBeGreaterThanOrEqual(2);
    });
    // 点击传统翻译的"添加 API"
    const addButtons = screen.getAllByText("settings.addApi");
    await user.click(addButtons[0]);
    await waitFor(() => {
      expect(screen.getByText("settings.addTraditionalService")).toBeInTheDocument();
    });
  });

  it("点击添加 AI 大模型 API 显示未配置服务列表", async () => {
    const user = userEvent.setup();
    renderComponent();
    await waitFor(() => {
      expect(screen.getAllByText("settings.addApi").length).toBeGreaterThanOrEqual(2);
    });
    // 点击 AI 大模型的"添加 API"（第二个）
    const addButtons = screen.getAllByText("settings.addApi");
    await user.click(addButtons[1]);
    await waitFor(() => {
      expect(screen.getByText("settings.addAiService")).toBeInTheDocument();
    });
  });

  it("添加面板显示搜索框", async () => {
    const user = userEvent.setup();
    renderComponent();
    await waitFor(() => {
      expect(screen.getAllByText("settings.addApi").length).toBeGreaterThanOrEqual(2);
    });
    const addButtons = screen.getAllByText("settings.addApi");
    await user.click(addButtons[1]);
    await waitFor(() => {
      expect(screen.getByPlaceholderText("settings.searchServicePlaceholder")).toBeInTheDocument();
    });
  });
});

// === SECTION 4 END ===

describe("TranslateApiSettings - 官方 API 登陆按钮", () => {
  it("点击登陆按钮调用 openUrl", async () => {
    useDevModeStore.setState({ devMode: true });
    const user = userEvent.setup();
    renderComponent();
    await waitFor(() => {
      expect(screen.getByText("settings.officialApiLoginButton")).toBeInTheDocument();
    });
    await user.click(screen.getByText("settings.officialApiLoginButton"));
    expect(mockOpenUrl).toHaveBeenCalledWith("https://www.baidu.com");
  });
});

// === SECTION 5 END ===

describe("TranslateApiSettings - 已配置服务显示", () => {
  it("已配置的传统服务显示在左列表", async () => {
    // 模拟 baidu 已配置
    mockGetConfig.mockImplementation((key: string) => {
      if (key === "translate_baidu_app_id") return Promise.resolve("test_app_id");
      return Promise.resolve(null);
    });
    mockGetCredential.mockImplementation((provider: string) => {
      if (provider === "baidu") return Promise.resolve("test_secret");
      return Promise.resolve(null);
    });
    renderComponent();
    await waitFor(() => {
      expect(screen.getByText("百度翻译")).toBeInTheDocument();
    });
  });

  it("已配置的 AI 服务显示在左列表", async () => {
    // 模拟 deepseek 已配置
    mockGetConfig.mockImplementation((key: string) => {
      if (key === "translate_openai_deepseek_base_url") return Promise.resolve("https://api.deepseek.com/v1");
      if (key === "translate_openai_deepseek_selected_models") return Promise.resolve("deepseek-chat");
      return Promise.resolve(null);
    });
    mockGetCredential.mockImplementation((provider: string) => {
      if (provider === "openai_deepseek") return Promise.resolve("test_key");
      return Promise.resolve(null);
    });
    renderComponent();
    await waitFor(() => {
      expect(screen.getByText("DeepSeek")).toBeInTheDocument();
    });
  });
});

// === SECTION 6 END ===

describe("TranslateApiSettings - 保存后刷新左侧列表", () => {
  it("保存传统翻译服务后，左侧列表显示该服务", async () => {
    const user = userEvent.setup();
    renderComponent();
    await waitFor(() => {
      expect(screen.getAllByText("settings.addApi").length).toBeGreaterThanOrEqual(2);
    });
    // 进入添加传统翻译面板
    const addButtons = screen.getAllByText("settings.addApi");
    await user.click(addButtons[0]);
    await waitFor(() => {
      expect(screen.getByText("settings.addTraditionalService")).toBeInTheDocument();
    });
    // 点击百度翻译
    await user.click(screen.getByText("百度翻译"));
    await waitFor(() => {
      expect(screen.getByText("App ID")).toBeInTheDocument();
    });

    // 填写表单（使用 fireEvent.change 确保不受 loading 状态影响）
    const appIdInput = screen.getByPlaceholderText("百度翻译 App ID");
    const secretInput = screen.getByPlaceholderText("Secret Key");
    fireEvent.change(appIdInput, { target: { value: "test_app_id" } });
    fireEvent.change(secretInput, { target: { value: "test_secret" } });

    // 模拟保存后 getConfig / getCredential 返回已配置
    mockGetConfig.mockImplementation((key: string) => {
      if (key === "translate_baidu_app_id") return Promise.resolve("test_app_id");
      return Promise.resolve(null);
    });
    mockGetCredential.mockImplementation((provider: string) => {
      if (provider === "baidu") return Promise.resolve("test_secret");
      return Promise.resolve(null);
    });

    // 等待保存按钮可用
    await waitFor(() => {
      expect(screen.getByText("settings.save")).not.toBeDisabled();
    });

    // 点击保存
    await user.click(screen.getByText("settings.save"));

    // 验证配置确实写入了 config 和 keyring
    await waitFor(() => {
      expect(mockSetConfig).toHaveBeenCalledWith("translate_baidu_app_id", "test_app_id");
    });
    expect(mockSaveCredential).toHaveBeenCalledWith("baidu", "secret", "test_secret");

    // 验证保存后 checkAllServiceConfigs 被调用并刷新左侧列表
    await waitFor(() => {
      expect(mockGetConfig).toHaveBeenCalledWith("translate_baidu_app_id");
      expect(mockGetCredential).toHaveBeenCalledWith("baidu", "secret", expect.any(String));
    });

    await waitFor(() => {
      // 保存后左侧列表会新增一个"百度翻译"按钮，加上右面板标题至少 2 个
      expect(screen.getAllByText("百度翻译").length).toBeGreaterThanOrEqual(2);
    }, { timeout: 3000 });
  });
});

// === SECTION 7 END ===

describe("TranslateApiSettings - 服务配置面板", () => {
  it("点击未配置的传统服务显示配置表单", async () => {
    const user = userEvent.setup();
    renderComponent();
    await waitFor(() => {
      expect(screen.getAllByText("settings.addApi").length).toBeGreaterThanOrEqual(2);
    });
    // 点击添加传统翻译
    const addButtons = screen.getAllByText("settings.addApi");
    await user.click(addButtons[0]);
    await waitFor(() => {
      expect(screen.getByText("settings.addTraditionalService")).toBeInTheDocument();
    });
    // 点击百度翻译（未配置的服务）
    await user.click(screen.getByText("百度翻译"));
    await waitFor(() => {
      // 应该显示 App ID 输入框
      expect(screen.getByText("App ID")).toBeInTheDocument();
    });
  });

  it("点击未配置的 AI 服务显示配置表单", async () => {
    const user = userEvent.setup();
    renderComponent();
    await waitFor(() => {
      expect(screen.getAllByText("settings.addApi").length).toBeGreaterThanOrEqual(2);
    });
    // 点击添加 AI 大模型
    const addButtons = screen.getAllByText("settings.addApi");
    await user.click(addButtons[1]);
    await waitFor(() => {
      expect(screen.getByText("settings.addAiService")).toBeInTheDocument();
    });
    // 点击 DeepSeek（未配置的服务）
    await user.click(screen.getByText("DeepSeek"));
    await waitFor(() => {
      // 应该显示 API 地址输入框
      expect(screen.getByText("settings.openaiBaseUrl")).toBeInTheDocument();
    });
  });
});

// === SECTION 7 END ===



