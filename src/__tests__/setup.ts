// vitest 全局 setup
import "@testing-library/jest-dom/vitest";
import { vi } from "vitest";

// mock Tauri core API（所有测试共用）
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  convertFileSrc: vi.fn((path: string) => `asset://localhost/${path}`),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
  emit: vi.fn(() => Promise.resolve()),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(() => Promise.resolve(null)),
  save: vi.fn(() => Promise.resolve(null)),
}));

vi.mock("@tauri-apps/plugin-process", () => ({
  relaunch: vi.fn(() => Promise.resolve()),
}));

vi.mock("@tauri-apps/plugin-shell", () => ({
  open: vi.fn(() => Promise.resolve()),
}));

vi.mock("@tauri-apps/plugin-os", () => ({
  locale: vi.fn(() => "zh-CN"),
  type: vi.fn(() => "windows"),
}));

vi.mock("@tauri-apps/plugin-notification", () => ({
  sendNotification: vi.fn(),
  requestPermission: vi.fn(() => Promise.resolve("granted")),
  isPermissionGranted: vi.fn(() => Promise.resolve(true)),
}));

// mock sonner toast（避免 jsdom 下无 DOM 渲染问题）
vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
    info: vi.fn(),
    warning: vi.fn(),
  },
  Toaster: () => null,
}));
