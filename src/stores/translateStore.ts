// 翻译状态 store
import { create } from "zustand";
import { persist } from "zustand/middleware";
import { toast } from "sonner";
import type { TranslateResult, SubtitleEntry } from "../lib/ipc-types";
import { api, formatIpcError } from "../lib/api";
import { warn, error, log } from "../lib/logger";

/** 译名表条目（含候选译名，用于弹窗编辑） */
export interface GlossaryEntry {
  english: string;
  chinese: string;
  alternatives: string[]; // 候选译名（按频率降序）
}

interface TranslateState {
  translating: boolean;
  progress: number;
  total: number;
  result: TranslateResult | null;
  error: string | null;
  sourceLang: string;
  targetLang: string;
  provider: string;
  model: string;
  modelType: string;
  serviceId: string | null; // AI 服务 ID（如 "deepseek"），传统翻译为 null
  // 翻译统计
  totalChars: number;       // 总字符数
  translatedChars: number;  // 已翻译字符数
  startTime: number;        // 翻译开始时间戳（performance.now()）
  lastProgressTime: number; // 上次进度更新时间戳，用于 EMA 速度计算
  speed: number;            // EMA 速度（字符/秒）
  eta: number;              // 剩余时间（秒），-1=计算中
  // 最后一次翻译统计（用于开发模式显示）
  lastTranslateTime: number;    // 最后一次翻译耗时（毫秒）
  lastTranslateChars: number;   // 最后一次翻译字数
  lastTranslateTokens: number | null; // 最后一次翻译 token 数（仅 AI 翻译）
  // 人名精译
  glossary: [string, string][]; // 译名表 [(EnglishName, ChineseTranslation)]，传给翻译 API
  extractingNames: boolean;     // 是否正在预扫描提取人名
  glossaryDialogOpen: boolean;  // 译名表确认弹窗是否打开
  glossaryDraft: GlossaryEntry[]; // 译名表草稿（弹窗中编辑的，含候选译名）
  // 人名预扫描进度
  extractNamesProgress: number;  // 已完成段数
  extractNamesTotal: number;    // 总段数
  extractNamesStartTime: number; // 开始时间戳（performance.now()）
  extractNamesLastProgressTime: number; // 上次进度更新时间戳
  extractNamesSpeed: number;    // EMA 速度（段/秒）
  extractNamesEta: number;      // 剩余时间（秒），-1=计算中

  setSourceLang: (lang: string) => void;
  setTargetLang: (lang: string) => void;
  setProvider: (provider: string) => void;
  setModel: (model: string) => void;
  setModelType: (modelType: string) => void;
  setServiceId: (id: string | null) => void;
  setGlossary: (g: [string, string][]) => void;
  setGlossaryDialogOpen: (open: boolean) => void;
  setGlossaryDraft: (draft: GlossaryEntry[]) => void;
  setExtractingNames: (v: boolean) => void;
  extractNames: (entries: SubtitleEntry[]) => Promise<GlossaryEntry[] | null>;
  resetExtractNamesProgress: () => void;
  startTranslate: (entries: SubtitleEntry[], onEntryDone?: (index: number, translated: string, failed: boolean) => void, skipCache?: boolean, glossary?: [string, string][], nameTagging?: boolean, fileHash?: string) => Promise<TranslateResult | null>;
  cancelTranslate: () => Promise<void>;
  reset: () => void;
}

export const useTranslateStore = create<TranslateState>()(
  persist(
    (set, get) => ({
      translating: false,
      progress: 0,
      total: 0,
      result: null,
      error: null,
      sourceLang: "en",
      targetLang: "zh",
      provider: "",
      model: "",
      modelType: "",
      serviceId: null,
      totalChars: 0,
      translatedChars: 0,
      startTime: 0,
      lastProgressTime: 0,
      speed: 0,
      eta: -1,
      lastTranslateTime: 0,
      lastTranslateChars: 0,
      lastTranslateTokens: null,
      glossary: [],
      extractingNames: false,
      glossaryDialogOpen: false,
      glossaryDraft: [],
      extractNamesProgress: 0,
      extractNamesTotal: 0,
      extractNamesStartTime: 0,
      extractNamesLastProgressTime: 0,
      extractNamesSpeed: 0,
      extractNamesEta: -1,

      setSourceLang: (lang) => set({ sourceLang: lang }),
      setTargetLang: (lang) => set({ targetLang: lang }),
      setProvider: (provider) => set({ provider }),
      setModel: (model) => set({ model }),
      setModelType: (modelType) => set({ modelType }),
      setServiceId: (id) => set({ serviceId: id }),
      setGlossary: (g) => set({ glossary: g }),
      setGlossaryDialogOpen: (open) => set({ glossaryDialogOpen: open }),
      setGlossaryDraft: (draft) => set({ glossaryDraft: draft }),
      setExtractingNames: (v) => set({ extractingNames: v }),
      resetExtractNamesProgress: () => set({ extractNamesProgress: 0, extractNamesTotal: 0, extractNamesStartTime: 0, extractNamesLastProgressTime: 0, extractNamesSpeed: 0, extractNamesEta: -1 }),

      extractNames: async (entries: SubtitleEntry[]) => {
        const { sourceLang, targetLang, provider, model, modelType, serviceId } = get();
        if (provider !== "openai") return null; // 仅 AI 翻译支持
        const startTime = performance.now();
        set({ extractingNames: true, extractNamesProgress: 0, extractNamesTotal: 0, extractNamesStartTime: startTime, extractNamesLastProgressTime: startTime, extractNamesSpeed: 0, extractNamesEta: -1 });

        // 监听进度事件
        let unlistenProgress: (() => void) | null = null;
        try {
          unlistenProgress = await api.onExtractNamesProgress((progress, total, done) => {
            const state = get();
            const now = performance.now();
            const dt = (now - (state.extractNamesLastProgressTime || now)) / 1000;
            let speed = state.extractNamesSpeed;
            if (dt > 0.1 && progress > state.extractNamesProgress) {
              const deltaSegments = progress - state.extractNamesProgress;
              const instantSpeed = deltaSegments / dt;
              speed = speed > 0 ? speed * 0.7 + instantSpeed * 0.3 : instantSpeed;
            }
            const remaining = total - progress;
            const eta = speed > 0 ? remaining / speed : -1;
            set({ extractNamesProgress: progress, extractNamesTotal: total, extractNamesSpeed: speed, extractNamesEta: eta, extractNamesLastProgressTime: now });
          });
        } catch (e) {
          warn("人名预扫描进度监听失败:", e);
        }

        try {
          const texts = entries.map((e) => e.text);
          const names = await api.extractNames(texts, sourceLang, targetLang, provider, model || undefined, modelType || undefined, serviceId || undefined);
          const glossaryEntries: GlossaryEntry[] = names.map((n) => ({
            english: n.english,
            chinese: n.chinese,
            alternatives: n.alternatives ?? [],
          }));
          const glossary: [string, string][] = glossaryEntries.map((g) => [g.english, g.chinese]);
          set({ glossary, extractingNames: false });
          return glossaryEntries;
        } catch (e: any) {
          error("人名预扫描失败:", e);
          set({ extractingNames: false });
          return null;
        } finally {
          if (unlistenProgress) unlistenProgress();
        }
      },

      startTranslate: async (entries: SubtitleEntry[], onEntryDone?: (index: number, translated: string, failed: boolean) => void, skipCache?: boolean, glossary?: [string, string][], nameTagging?: boolean, fileHash?: string) => {
        // 如果正在翻译，不允许启动新的翻译任务
        if (get().translating) {
          warn("翻译正在进行中，跳过新任务");
          return null;
        }
        const { sourceLang, targetLang, provider, model, modelType, serviceId } = get();
        // 计算总字符数（用于速度和剩余时间估算）
        const totalChars = entries.reduce((sum, e) => sum + e.text.length, 0);
        const startTime = performance.now();
        set({ translating: true, progress: 0, total: entries.length, error: null, result: null, totalChars, translatedChars: 0, startTime, lastProgressTime: startTime, speed: 0, eta: -1 });

        // 监听进度事件
        let unlistenProgress: (() => void) | null = null;
        let unlistenEntry: (() => void) | null = null;
        try {
          unlistenProgress = await api.onTranslateProgress((progress, total, done) => {
            // 按条目数计算进度，用指数移动平均（EMA）计算速度和 ETA，避免反复跳
            const state = get();
            const now = performance.now();
            const elapsedSec = (now - state.startTime) / 1000;
            // 用条目数比例估算，比字符数更稳定
            const ratio = total > 0 ? progress / total : 0;
            const translatedChars = Math.round(state.totalChars * ratio);
            const remainingChars = state.totalChars - translatedChars;
            // EMA 速度：用最近一段时间的速度，而非全局平均
            // 全局平均在缓存命中时偏高、API 慢时偏低，导致 ETA 反复跳
            const prevProgress = state.progress;
            const prevTime = state.lastProgressTime || now;
            const dt = (now - prevTime) / 1000;
            let speed = state.speed;
            if (dt > 0.1 && progress > prevProgress) {
              const deltaChars = Math.round(state.totalChars * (progress - prevProgress) / Math.max(total, 1));
              const instantSpeed = deltaChars / dt;
              // EMA 平滑因子 0.3：新值权重 30%，旧值权重 70%
              speed = speed > 0 ? speed * 0.7 + instantSpeed * 0.3 : instantSpeed;
            }
            // ETA 基于剩余字符数和 EMA 速度
            const eta = speed > 0 ? remainingChars / speed : -1;
            set({ progress, total, translatedChars, speed, eta, lastProgressTime: now });
            // 注意：不在 done 事件中设置 translating: false
            // done 事件可能先于 IPC 返回到达前端，此时翻译结果还没回填到 subtitleStore
            // translating: false 由 api.translateSubtitle resolve 后统一设置
          });
        } catch (e) {
          warn("进度监听失败:", e);
        }

        // 监听单条翻译完成事件，逐条回调
        if (onEntryDone) {
          try {
            unlistenEntry = await api.onTranslateEntryDone((entry) => {
              onEntryDone(entry.index, entry.translated, entry.failed);
            });
          } catch (e) {
            warn("单条监听失败:", e);
          }
        }

        try {
          const result = await api.translateSubtitle(entries, sourceLang, targetLang, provider, model || undefined, modelType || undefined, serviceId || undefined, skipCache, glossary, nameTagging, fileHash);
          const endTime = performance.now();
          const totalMs = endTime - startTime;
          
          // 计算提交和返回的文字数
          const inputChars = entries.reduce((sum, e) => sum + e.text.length, 0);
          const outputChars = result.translations.reduce((sum, t) => sum + t.translated.length, 0);
          const totalSec = totalMs / 1000;
          const avgSpeed = totalSec > 0 ? (inputChars / totalSec).toFixed(1) : "0";

          // 记录最后一次翻译统计（用于开发模式显示）
          const totalTokens = result.token_usage?.total_tokens || null;
          set({ 
            translating: false, 
            progress: entries.length, 
            result,
            lastTranslateTime: totalMs,
            lastTranslateChars: inputChars,
            lastTranslateTokens: totalTokens
          });

          // 格式化时间戳
          const fmtTime = (ms: number) => {
            const d = new Date(ms);
            return d.toLocaleTimeString("zh-CN", { hour12: false }) + "." + d.getMilliseconds().toString().padStart(3, "0");
          };

          const tu = result.token_usage;
          const tokenInfo = tu
            ? `token: prompt=${tu.prompt_tokens}, completion=${tu.completion_tokens}, total=${tu.total_tokens}`
            : "token: N/A";

          log(`[翻译完成]` +
            ` 开始: ${fmtTime(startTime)}` +
            `, 结束: ${fmtTime(endTime)}` +
            `, 总时间: ${totalSec.toFixed(2)}s` +
            `, 条目: ${entries.length}` +
            `, 缓存: ${result.cached_count}` +
            `, 提交字数: ${inputChars}` +
            `, 返回字数: ${outputChars}` +
            `, 平均速度: ${avgSpeed} 字/s` +
            `, ${tokenInfo}`);
          return result;
        } catch (e: any) {
          const errMsg = formatIpcError(e);
          set({ translating: false, error: errMsg });
          return null;
        } finally {
          if (unlistenProgress) unlistenProgress();
          if (unlistenEntry) unlistenEntry();
        }
      },

      cancelTranslate: async () => {
        try {
          const state = get();
          const endTime = performance.now();
          const totalMs = state.startTime > 0 ? endTime - state.startTime : 0;
          const inputChars = state.totalChars;
          // 取消时无法获取准确的 token 数，设为 null
          
          await api.cancelTranslate();
          set({ 
            translating: false,
            lastTranslateTime: totalMs,
            lastTranslateChars: inputChars,
            lastTranslateTokens: null
          });
        } catch (e: any) {
          error("取消翻译失败:", e);
          toast.error(formatIpcError(e));
        }
      },

      reset: () => set({ translating: false, progress: 0, total: 0, result: null, error: null, totalChars: 0, translatedChars: 0, startTime: 0, lastProgressTime: 0, speed: 0, eta: -1 }),
    }),
    {
      name: "zimufan-translate-settings",
      // 持久化语言设置 + 翻译引擎选择（provider/serviceId/model）
      // provider/serviceId/model 同时由后端 db 管理，persist 用于初始渲染立即显示上次选择
      // db 加载后会覆盖确认，避免 persist 与 db 不一致
      partialize: (state) => ({
        sourceLang: state.sourceLang,
        targetLang: state.targetLang,
        provider: state.provider,
        serviceId: state.serviceId,
        model: state.model,
        modelType: state.modelType,
      }),
      version: 2,
      migrate: (persisted: any, version: number) => {
        if (version < 2) {
          // v1 只持久化了语言设置，v2 加回 provider/serviceId/model
          // 不需要清除旧数据，新字段不存在时用默认值
        }
        return persisted;
      },
    }
  )
);
