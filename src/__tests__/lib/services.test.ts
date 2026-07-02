// services.ts 单元测试
import { describe, it, expect } from "vitest";
import {
  SERVICES,
  ServiceDef,
  encodeAiSelectValue,
  decodeAiSelectValue,
  matchesSearch,
  getServiceById,
  getServicesByCategory,
} from "../../lib/services";

// === SECTION 1 END ===

describe("services - ServiceDef 约束", () => {
  it("所有 id 匹配 /^[a-z0-9_]+$/（不含 | / : / / 等特殊字符）", () => {
    for (const s of SERVICES) {
      expect(s.id).toMatch(/^[a-z0-9_]+$/);
    }
  });

  it("所有 presetBaseUrl 不以 / 结尾", () => {
    for (const s of SERVICES) {
      if (s.presetBaseUrl !== undefined && s.presetBaseUrl !== "") {
        expect(s.presetBaseUrl).not.toMatch(/\/$/);
      }
    }
  });

  it("id 唯一", () => {
    const ids = SERVICES.map((s) => s.id);
    const unique = new Set(ids);
    expect(unique.size).toBe(ids.length);
  });

  it("comingSoon 传统翻译服务 requiresApiKey 为 false", () => {
    for (const s of SERVICES) {
      if (s.comingSoon && s.category === "traditional") {
        expect(s.requiresApiKey).toBe(false);
      }
    }
  });
});

// === SECTION 2 END ===

describe("services - encodeAiSelectValue / decodeAiSelectValue", () => {
  it("往返一致性", () => {
    const cases = [
      { serviceId: "deepseek", model: "deepseek-chat" },
      { serviceId: "zhipu", model: "glm-4-flash" },
      { serviceId: "ollama", model: "llama3:8b" },
    ];
    for (const c of cases) {
      const encoded = encodeAiSelectValue(c.serviceId, c.model);
      const decoded = decodeAiSelectValue(encoded);
      expect(decoded).toEqual(c);
    }
  });

  it("含特殊字符的 model 名能正确编解码", () => {
    const specialModels = [
      "model:with:colons",
      "model||with||pipes",
      "model/with/slashes",
      "模型名含中文",
      "model with spaces",
    ];
    for (const model of specialModels) {
      const encoded = encodeAiSelectValue("custom", model);
      const decoded = decodeAiSelectValue(encoded);
      expect(decoded).toEqual({ serviceId: "custom", model });
    }
  });

  it("编码结果以 ai: 前缀开头", () => {
    const encoded = encodeAiSelectValue("deepseek", "deepseek-chat");
    expect(encoded.startsWith("ai:")).toBe(true);
  });

  it("decodeAiSelectValue 对非 ai: 前缀返回 null", () => {
    expect(decodeAiSelectValue("baidu")).toBeNull();
    expect(decodeAiSelectValue("bing")).toBeNull();
    expect(decodeAiSelectValue("openai:model")).toBeNull();
  });

  it("decodeAiSelectValue 对无效 base64 返回 null", () => {
    expect(decodeAiSelectValue("ai:!!!invalidbase64!!!")).toBeNull();
  });

  it("不同输入产生不同编码", () => {
    const a = encodeAiSelectValue("deepseek", "deepseek-chat");
    const b = encodeAiSelectValue("zhipu", "glm-4-flash");
    expect(a).not.toBe(b);
  });
});

// === SECTION 3 END ===

describe("services - matchesSearch", () => {
  it("空查询匹配所有", () => {
    expect(matchesSearch(SERVICES[0], "")).toBe(true);
  });

  it("按 name 匹配", () => {
    const s = getServiceById("deepseek")!;
    expect(matchesSearch(s, "deep")).toBe(true);
    expect(matchesSearch(s, "Deep")).toBe(true); // 大小写不敏感
    expect(matchesSearch(s, "zhipu")).toBe(false);
  });

  it("按 freeQuota 匹配", () => {
    const s = getServiceById("groq")!;
    expect(matchesSearch(s, "免费")).toBe(true);
  });

  it("按 price 匹配", () => {
    const s = getServiceById("deepseek")!;
    expect(matchesSearch(s, "tokens")).toBe(true);
  });
});

// === SECTION 4 END ===

describe("services - getServiceById / getServicesByCategory", () => {
  it("getServiceById 返回正确的服务", () => {
    const s = getServiceById("baidu");
    expect(s).toBeDefined();
    expect(s!.name).toBe("百度翻译");
  });

  it("getServiceById 对不存在的 id 返回 undefined", () => {
    expect(getServiceById("nonexistent")).toBeUndefined();
  });

  it("getServicesByCategory 返回正确分类", () => {
    const traditional = getServicesByCategory("traditional");
    const ai = getServicesByCategory("ai");
    expect(traditional.every((s) => s.category === "traditional")).toBe(true);
    expect(ai.every((s) => s.category === "ai")).toBe(true);
    expect(traditional.length + ai.length).toBe(SERVICES.length);
  });

  it("SERVICES 包含 27 个服务", () => {
    expect(SERVICES.length).toBe(27);
  });
});

// === SECTION 5 END ===
