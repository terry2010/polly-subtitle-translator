// utils 单元测试
import { describe, it, expect } from "vitest";
import {
  cn,
  formatTime,
  formatBytes,
  formatDuration,
  toFileLangCode,
  buildSubtitleTitle,
  stripExt,
  fileDir,
  buildExportFileName,
  buildExportFilePath,
  assColorToCss,
  hexToAssColor,
} from "../../lib/utils";

// === SECTION 1 END ===

describe("utils - cn", () => {
  it("合并类名", () => {
    expect(cn("a", "b", "c")).toBe("a b c");
  });

  it("处理条件类名", () => {
    expect(cn("base", { active: true, disabled: false })).toBe("base active");
  });

  it("合并 tailwind 冲突类", () => {
    expect(cn("px-2 py-1", "px-4")).toBe("py-1 px-4");
  });
});

// === SECTION 2 END ===

describe("utils - formatTime", () => {
  it("格式化毫秒", () => {
    expect(formatTime(3661234)).toBe("01:01:01.234");
  });

  it("补零", () => {
    expect(formatTime(0)).toBe("00:00:00.000");
    expect(formatTime(5000)).toBe("00:00:05.000");
  });
});

// === SECTION 3 END ===

describe("utils - formatBytes", () => {
  it("0 B", () => {
    expect(formatBytes(0)).toBe("0 B");
  });

  it("KB / MB", () => {
    expect(formatBytes(1024)).toBe("1 KB");
    expect(formatBytes(1024 * 1024 * 1.5)).toBe("1.5 MB");
  });
});

// === SECTION 4 END ===

describe("utils - formatDuration", () => {
  it("秒", () => {
    expect(formatDuration(5000)).toBe("5s");
  });

  it("分秒", () => {
    expect(formatDuration(90000)).toBe("1m 30s");
  });

  it("时分秒", () => {
    expect(formatDuration(3661000)).toBe("1h 1m 1s");
  });
});

// === SECTION 5 END ===

describe("utils - 语言代码", () => {
  it("toFileLangCode", () => {
    expect(toFileLangCode("zh")).toBe("zhs");
    expect(toFileLangCode("zh-Hant")).toBe("zht");
    expect(toFileLangCode("en")).toBe("eng");
    expect(toFileLangCode("unknown")).toBe("unknown");
  });

  it("buildSubtitleTitle 单语", () => {
    const options = {
      mode: "monolingual" as const,
      format: "ass" as const,
      monolingual_lang: "translated" as const,
      bilingual_translated_first: true,
    };
    expect(buildSubtitleTitle(options, "en", "zh")).toBe("中文");
  });

  it("buildSubtitleTitle 双语", () => {
    const options = {
      mode: "bilingual" as const,
      format: "ass" as const,
      monolingual_lang: "translated" as const,
      bilingual_translated_first: true,
    };
    expect(buildSubtitleTitle(options, "en", "zh")).toBe("中文English双语");
  });
});

// === SECTION 6 END ===

describe("utils - 路径", () => {
  it("stripExt", () => {
    expect(stripExt("/path/to/video.mkv")).toBe("video");
    expect(stripExt("C:\\Users\\video.mp4")).toBe("video");
  });

  it("fileDir", () => {
    expect(fileDir("/path/to/video.mkv")).toBe("/path/to/");
    expect(fileDir("video.mkv")).toBe("");
  });

  it("buildExportFileName", () => {
    const options = {
      mode: "bilingual" as const,
      format: "srt" as const,
      monolingual_lang: "translated" as const,
      bilingual_translated_first: true,
    };
    expect(buildExportFileName(options, "en", "zh", "movie")).toBe("movie.zhs-eng.srt");
  });

  it("buildExportFilePath", () => {
    const options = {
      mode: "monolingual" as const,
      format: "ass" as const,
      monolingual_lang: "source" as const,
      bilingual_translated_first: true,
    };
    expect(buildExportFilePath("/path/to/video.mkv", null, options, "en", "zh")).toBe("/path/to/video.eng.ass");
  });
});

// === SECTION 7 END ===

describe("utils - ASS 颜色", () => {
  it("assColorToCss", () => {
    expect(assColorToCss("&HFFFFFF&")).toBe("#FFFFFF");
    expect(assColorToCss("&HFF0000&")).toBe("#0000FF");
    expect(assColorToCss("invalid")).toBe("#ffffff");
  });

  it("hexToAssColor", () => {
    expect(hexToAssColor("#ffffff")).toBe("&Hffffff&");
    expect(hexToAssColor("#0000ff")).toBe("&Hff0000&");
  });
});

// === SECTION 8 END ===
