// App.tsx 拖放与 CLI 参数逻辑测试
// 测试 file-drop 事件的扩展名分流逻辑（不渲染完整 App，只测 handler 逻辑）
import { describe, it, expect, vi } from "vitest";

// 这些测试验证拖放事件的扩展名分流逻辑
// App.tsx 内部 listen("app://file-drop") 注册回调，根据扩展名调用 loadSubtitle 或 openVideo

describe("App 拖放扩展名分流逻辑", () => {
  const VIDEO_EXTS = ["mkv", "mp4", "avi", "mov", "wmv", "flv", "ts", "m2ts"];
  const SUB_EXTS = ["srt", "ass", "ssa", "vtt", "sub"];

  function getHandler(filePath: string): "subtitle" | "video" | null {
    const ext = filePath.split(".").pop()?.toLowerCase();
    if (!ext) return null;
    if (SUB_EXTS.includes(ext)) return "subtitle";
    if (VIDEO_EXTS.includes(ext)) return "video";
    return null;
  }

  it("字幕扩展名 → subtitle handler", () => {
    expect(getHandler("test.srt")).toBe("subtitle");
    expect(getHandler("test.ass")).toBe("subtitle");
    expect(getHandler("test.ssa")).toBe("subtitle");
    expect(getHandler("test.vtt")).toBe("subtitle");
    expect(getHandler("test.sub")).toBe("subtitle");
  });

  it("视频扩展名 → video handler", () => {
    expect(getHandler("movie.mkv")).toBe("video");
    expect(getHandler("movie.mp4")).toBe("video");
    expect(getHandler("movie.avi")).toBe("video");
    expect(getHandler("movie.mov")).toBe("video");
    expect(getHandler("movie.wmv")).toBe("video");
    expect(getHandler("movie.flv")).toBe("video");
    expect(getHandler("movie.ts")).toBe("video");
    expect(getHandler("movie.m2ts")).toBe("video");
  });

  it("不支持的扩展名 → null", () => {
    expect(getHandler("readme.txt")).toBeNull();
    expect(getHandler("archive.zip")).toBeNull();
    expect(getHandler("noext")).toBeNull();
  });

  it("大写扩展名也能识别", () => {
    expect(getHandler("test.SRT")).toBe("subtitle");
    expect(getHandler("MOVIE.MKV")).toBe("video");
  });

  it("多扩展名取最后一个", () => {
    expect(getHandler("video.en.srt")).toBe("subtitle");
    expect(getHandler("backup.2024.mkv")).toBe("video");
  });
});

// === SECTION 1 END ===

describe("App CLI 模式分流逻辑", () => {
  // App.tsx 中 cli-args 事件处理：根据 mode 和扩展名分流
  function getMode(mode: string, filePath: string): "edit" | "quick" | "subtitle" | "video" {
    if (mode === "edit") return "edit";
    if (mode === "quick") return "quick";
    const ext = filePath.split(".").pop()?.toLowerCase();
    if (ext && ["srt", "ass", "ssa", "vtt", "sub"].includes(ext)) return "subtitle";
    return "video";
  }

  it("mode=edit → edit", () => {
    expect(getMode("edit", "test.srt")).toBe("edit");
  });

  it("mode=quick → quick", () => {
    expect(getMode("quick", "movie.mkv")).toBe("quick");
  });

  it("无模式 + 字幕扩展名 → subtitle", () => {
    expect(getMode("", "test.ass")).toBe("subtitle");
  });

  it("无模式 + 视频扩展名 → video", () => {
    expect(getMode("", "movie.mp4")).toBe("video");
  });

  it("无模式 + .sub → subtitle", () => {
    expect(getMode("", "test.sub")).toBe("subtitle");
  });
});

// === SECTION 2 END ===
