// themeStore 单元测试
import { describe, it, expect, beforeEach } from "vitest";
import { useThemeStore } from "../../stores/themeStore";

function getStore() {
  return useThemeStore.getState();
}

function resetStore() {
  useThemeStore.setState({ theme: "system", language: "zh" });
}

beforeEach(() => {
  resetStore();
});

// === SECTION 1 END ===

describe("themeStore - 主题", () => {
  it("默认主题为 system", () => {
    expect(getStore().theme).toBe("system");
  });

  it("setTheme 切换主题", () => {
    getStore().setTheme("dark");
    expect(getStore().theme).toBe("dark");

    getStore().setTheme("light");
    expect(getStore().theme).toBe("light");
  });
});

// === SECTION 2 END ===

describe("themeStore - 语言", () => {
  it("默认语言为 zh", () => {
    expect(getStore().language).toBe("zh");
  });

  it("setLanguage 切换语言", () => {
    getStore().setLanguage("en");
    expect(getStore().language).toBe("en");
  });
});

// === SECTION 3 END ===
