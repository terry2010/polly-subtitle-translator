// 批量翻译页面
import { useEffect, useState, useCallback } from "react";
import { useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { open } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import {
  ArrowLeft,
  FolderOpen,
  FileVideo,
  Play,
  Pause,
  Square,
  Trash2,
  X,
  CheckCircle2,
  XCircle,
  Settings as SettingsIcon,
  GripVertical,
  Plus,
  Search,
} from "lucide-react";
import {
  DndContext,
  closestCenter,
  KeyboardSensor,
  PointerSensor,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core";
import {
  arrayMove,
  SortableContext,
  sortableKeyboardCoordinates,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { SERVICES, encodeAiSelectValue, decodeAiSelectValue } from "../lib/services";
import { api } from "../lib/api";
import { Button } from "../components/ui/button";
import { Card, CardHeader, CardTitle, CardContent } from "../components/ui/card";
import { Progress } from "../components/ui/progress";
import {
  Select,
  SelectTrigger,
  SelectValue,
  SelectContent,
  SelectItem,
} from "../components/ui/select";
import { useBatchStore, getStatusText, getTaskProgress } from "../stores/batchStore";
import { toast } from "sonner";
import type { BatchTask, BatchStatus, BatchConfig, OutputMode } from "../lib/ipc-types";

/** 源语言选项（多选用） */
const LANG_OPTIONS = [
  { value: "en", label: "English" },
  { value: "ja", label: "日本語" },
  { value: "ko", label: "한국어" },
  { value: "zh", label: "中文" },
  { value: "fr", label: "Français" },
  { value: "de", label: "Deutsch" },
  { value: "es", label: "Español" },
  { value: "ru", label: "Русский" },
];

/** 语言代码 → 显示名称映射 */
const LANG_LABELS: Record<string, string> = Object.fromEntries(
  LANG_OPTIONS.map((l) => [l.value, l.label])
);

/** 不翻译的语言选项（多选用，含简体/繁体中文） */
const SKIP_LANG_OPTIONS = [
  { value: "zh", label: "简体中文" },
  { value: "zh-TW", label: "繁体中文" },
  { value: "ja", label: "日本語" },
  { value: "ko", label: "한국어" },
  { value: "en", label: "English" },
];

export default function BatchView({ embedded = false }: { embedded?: boolean }) {
  const navigate = useNavigate();
  const { t } = useTranslation();
  const store = useBatchStore();

  const [selectedPaths, setSelectedPaths] = useState<string[]>([]);

  useEffect(() => {
    void store.init();
    void store.loadConfig();
    void store.refreshStatus();
    return () => store.destroy();
  }, []);

  // === 文件选择 ===
  const handleSelectFiles = useCallback(async () => {
    const result = await open({
      multiple: true,
      filters: [{ name: "视频/字幕", extensions: ["mkv", "mp4", "avi", "mov", "wmv", "flv", "ts", "m2ts", "srt", "ass", "ssa", "vtt"] }],
    });
    if (result && result.length > 0) {
      setSelectedPaths(result as string[]);
    }
  }, []);

  const handleSelectFolder = useCallback(async () => {
    const result = await open({ directory: true, multiple: true });
    if (result && result.length > 0) {
      setSelectedPaths(result as string[]);
    }
  }, []);

  const handleSubmit = useCallback(async () => {
    if (selectedPaths.length === 0) return;
    await store.submitFiles(selectedPaths);
    setSelectedPaths([]);
  }, [selectedPaths, store]);

  // === 任务操作 ===
  const handleDelete = useCallback(
    (taskId: string) => { void store.deleteTask(taskId); },
    [store]
  );
  const handleCancel = useCallback(
    (taskId: string) => { void store.cancelTask(taskId); },
    [store]
  );
  const handleClear = useCallback(() => { void store.clearQueue(); }, [store]);
  const handlePause = useCallback(() => { void store.pauseQueue(); }, [store]);
  const handleResume = useCallback(() => { void store.resumeQueue(); }, [store]);
  const handleStart = useCallback(
    (taskId: string) => { void store.startTask(taskId); },
    [store]
  );

  // === 拖动排序 ===
  const taskSensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 5 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );
  const handleTaskDragEnd = useCallback((event: DragEndEvent) => {
    const { active, over } = event;
    if (!over || active.id === over.id) return;
    const oldIndex = store.tasks.findIndex((t) => t.id === active.id);
    const newIndex = store.tasks.findIndex((t) => t.id === over.id);
    if (oldIndex < 0 || newIndex < 0) return;
    const newOrder = arrayMove(store.tasks, oldIndex, newIndex);
    void store.reorderTasks(newOrder.map((t) => t.id));
  }, [store]);

  // === 统计 ===
  const stats = {
    total: store.tasks.length,
    queued: store.tasks.filter((t) => typeof t.status === "string" && t.status === "Queued").length,
    processing: store.tasks.filter((t) => {
      const s = t.status;
      return typeof s === "string"
        ? ["Probing", "CheckingSubtitle", "Parsing", "Exporting"].includes(s)
        : "Extracting" in s || "Translating" in s;
    }).length,
    done: store.tasks.filter((t) => typeof t.status === "string" && t.status === "Done").length,
    failed: store.tasks.filter((t) => typeof t.status === "object" && "Failed" in t.status).length,
    skipped: store.tasks.filter((t) => typeof t.status === "object" && "Skipped" in t.status).length,
    cancelled: store.tasks.filter((t) => typeof t.status === "string" && t.status === "Cancelled").length,
  };

  return (
    <div className={embedded ? "flex flex-col space-y-4" : "flex flex-col h-screen bg-background"}>
      {/* 顶部导航（仅独立页面模式显示） */}
      {!embedded && (
        <div className="flex items-center gap-2 p-3 border-b">
          <Button variant="ghost" size="icon" onClick={() => navigate("/")}>
            <ArrowLeft className="h-5 w-5" />
          </Button>
          <h1 className="text-lg font-semibold flex-1">批量翻译</h1>
          <Button variant="ghost" size="sm" onClick={() => navigate("/settings")}>
            <SettingsIcon className="h-4 w-4 mr-1" />
            设置
          </Button>
        </div>
      )}

      <div className={embedded ? "space-y-4" : "flex-1 overflow-auto p-4 space-y-4"}>
        {/* 文件选择区 */}
        <Card>
          <CardHeader>
            <CardTitle className="text-base">添加文件</CardTitle>
          </CardHeader>
          <CardContent className="space-y-3">
            <div className="flex gap-2">
              <Button variant="outline" size="sm" onClick={handleSelectFiles}>
                <FileVideo className="h-4 w-4 mr-1" />
                选择文件
              </Button>
              <Button variant="outline" size="sm" onClick={handleSelectFolder}>
                <FolderOpen className="h-4 w-4 mr-1" />
                选择文件夹
              </Button>
            </div>
            {selectedPaths.length > 0 && (
              <div className="space-y-2">
                <p className="text-sm text-muted-foreground">
                  已选择 {selectedPaths.length} 个文件/目录
                </p>
                <Button size="sm" onClick={handleSubmit}>
                  <Play className="h-4 w-4 mr-1" />
                  添加到队列
                </Button>
              </div>
            )}
          </CardContent>
        </Card>

        {/* 队列控制区 */}
        <Card>
          <CardHeader>
            <div className="flex items-center justify-between">
              <CardTitle className="text-base">
                队列（{stats.total}）
              </CardTitle>
              <div className="flex gap-1">
                {store.isPaused ? (
                  <Button variant="outline" size="sm" onClick={handleResume}>
                    <Play className="h-4 w-4 mr-1" />
                    启动
                  </Button>
                ) : (
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={handlePause}
                  >
                    <Pause className="h-4 w-4 mr-1" />
                    停止
                  </Button>
                )}
                <Button
                  variant="outline"
                  size="sm"
                  onClick={handleClear}
                  disabled={stats.queued + stats.failed + stats.skipped === 0}
                >
                  <Trash2 className="h-4 w-4 mr-1" />
                  清空列表
                </Button>
              </div>
            </div>
          </CardHeader>
          <CardContent>
            {/* 统计栏 */}
            <div className="flex gap-4 text-sm mb-3 text-muted-foreground">
              <span>排队: {stats.queued}</span>
              <span>处理中: {stats.processing}</span>
              <span className="text-green-500">完成: {stats.done}</span>
              <span className="text-orange-500">跳过: {stats.skipped}</span>
              <span className="text-red-500">失败: {stats.failed}</span>
              {stats.cancelled > 0 && <span className="text-muted-foreground">取消: {stats.cancelled}</span>}
            </div>

            {store.isPaused && (
              <div className="mb-3 p-2 bg-orange-50 dark:bg-orange-950/30 border border-orange-200 dark:border-orange-800 rounded text-sm">
                <span className="flex items-center gap-1">
                  <Pause className="h-4 w-4" />
                  队列已暂停{store.pauseReason ? `: ${store.pauseReason}` : ""}
                </span>
              </div>
            )}

            {/* 任务列表 */}
            {store.tasks.length === 0 ? (
              <div className="text-center py-8 text-muted-foreground">
                队列为空，请添加文件开始批量翻译
              </div>
            ) : (
              <DndContext sensors={taskSensors} collisionDetection={closestCenter} onDragEnd={handleTaskDragEnd}>
                <SortableContext items={store.tasks.map((t) => t.id)} strategy={verticalListSortingStrategy}>
                  <div className="space-y-2 max-h-[400px] overflow-auto">
                    {store.tasks.map((task) => (
                      <SortableTaskRow
                        key={task.id}
                        task={task}
                        onDelete={handleDelete}
                        onStart={handleStart}
                        onCancel={handleCancel}
                      />
                    ))}
                  </div>
                </SortableContext>
              </DndContext>
            )}
          </CardContent>
        </Card>

        {/* 文件夹监视区 */}
        <FolderWatchSection />

        {/* 批量翻译配置区 */}
        <BatchConfigSection />
      </div>
    </div>
  );
}

// === SECTION 1 END ===

// === SECTION 2: TaskRow 组件 ===

function SortableTaskRow({
  task,
  onDelete,
  onStart,
  onCancel,
}: {
  task: BatchTask;
  onDelete: (id: string) => void;
  onStart: (id: string) => void;
  onCancel: (id: string) => void;
}) {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({ id: task.id });
  const [menuOpen, setMenuOpen] = useState(false);
  const [menuPos, setMenuPos] = useState({ x: 0, y: 0 });
  const status = task.status;
  const statusText = getStatusText(status);
  const progress = getTaskProgress(status);
  const fileName = task.video_path.split(/[\\/]/).pop() ?? task.video_path;

  const isDone = typeof status === "string" && status === "Done";
  const isFailed = typeof status === "object" && "Failed" in status;
  const isSkipped = typeof status === "object" && "Skipped" in status;
  const isCancelled = typeof status === "string" && status === "Cancelled";
  const isProcessing = !isDone && !isFailed && !isSkipped && !isCancelled
    && !(typeof status === "string" && status === "Queued");
  const isQueued = typeof status === "string" && status === "Queued";

  const style = {
    transform: CSS.Transform.toString(transform),
    transition,
    opacity: isDragging ? 0.5 : 1,
  };

  const handleContextMenu = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    setMenuPos({ x: e.clientX, y: e.clientY });
    setMenuOpen(true);
  }, []);

  const closeMenu = useCallback(() => setMenuOpen(false), []);

  const handleCopyFileName = useCallback(() => {
    navigator.clipboard.writeText(fileName).catch(() => {});
    setMenuOpen(false);
  }, [fileName]);

  const handleCopyFilePath = useCallback(() => {
    navigator.clipboard.writeText(task.video_path).catch(() => {});
    setMenuOpen(false);
  }, [task.video_path]);

  const handleStart = useCallback(() => {
    onStart(task.id);
    setMenuOpen(false);
  }, [task.id, onStart]);

  const handleStop = useCallback(() => {
    onCancel(task.id);
    setMenuOpen(false);
  }, [task.id, onCancel]);

  const handleRemove = useCallback(() => {
    onDelete(task.id);
    setMenuOpen(false);
  }, [task.id, onDelete]);

  return (
    <>
      <div
        ref={setNodeRef}
        style={style}
        className="flex items-center gap-2 p-2 border rounded text-sm"
        onContextMenu={handleContextMenu}
      >
        {/* 拖拽手柄 */}
        <div className="flex-shrink-0 cursor-grab active:cursor-grabbing text-muted-foreground" {...attributes} {...listeners}>
          <GripVertical className="h-4 w-4" />
        </div>

        {/* 文件名 + 进度 + 状态 */}
        <div className="flex-1 min-w-0">
          <div className="truncate" title={task.video_path}>
            {fileName}
          </div>
          {isProcessing && (
            <Progress value={progress * 100} className="h-1 mt-1" />
          )}
          {task.total_entries > 0 && (
            <div className="text-xs text-muted-foreground mt-0.5">
              {task.done_entries}/{task.total_entries} 条
              {task.cached_entries > 0 && ` (缓存 ${task.cached_entries})`}
              {task.failed_entries > 0 && ` (失败 ${task.failed_entries})`}
            </div>
          )}
          <div className="text-xs text-muted-foreground mt-0.5">
            {statusText}
          </div>
        </div>

        {/* 操作按钮 */}
        <div className="flex-shrink-0 flex gap-1">
          {!isProcessing && (
            <Button
              variant="ghost"
              size="icon"
              className="h-7 w-7 text-green-600 hover:text-green-700"
              onClick={() => onStart(task.id)}
              title="启动翻译"
            >
              <Play className="h-3.5 w-3.5" />
            </Button>
          )}
          <Button
            variant="ghost"
            size="icon"
            className="h-7 w-7 text-muted-foreground hover:text-red-500"
            onClick={() => onDelete(task.id)}
            title="删除"
          >
            <X className="h-3.5 w-3.5" />
          </Button>
        </div>
      </div>

      {/* 右键菜单 */}
      {menuOpen && (
        <>
          <div className="fixed inset-0 z-40" onClick={closeMenu} onContextMenu={(e) => { e.preventDefault(); closeMenu(); }} />
          <div
            className="fixed z-50 min-w-[160px] bg-popover border border-border rounded-md shadow-md py-1 text-sm"
            style={{ left: menuPos.x, top: menuPos.y }}
          >
            <button
              className="w-full text-left px-3 py-1.5 hover:bg-accent"
              onClick={handleCopyFileName}
            >
              复制文件名
            </button>
            <button
              className="w-full text-left px-3 py-1.5 hover:bg-accent"
              onClick={handleCopyFilePath}
            >
              复制文件路径
            </button>
            <div className="border-t border-border my-1" />
            {!isProcessing && (
              <button
                className="w-full text-left px-3 py-1.5 hover:bg-accent text-green-600"
                onClick={handleStart}
              >
                开始翻译
              </button>
            )}
            {(isQueued || isProcessing) && (
              <button
                className="w-full text-left px-3 py-1.5 hover:bg-accent text-orange-600"
                onClick={handleStop}
              >
                停止翻译
              </button>
            )}
            <button
              className="w-full text-left px-3 py-1.5 hover:bg-accent text-red-500"
              onClick={handleRemove}
            >
              从队列中移除
            </button>
          </div>
        </>
      )}
    </>
  );
}

// === SECTION 2 END ===

// === SECTION 3: FolderWatchSection 组件 ===

function FolderWatchSection() {
  const store = useBatchStore();
  const [watchPaths, setWatchPaths] = useState<string[]>([]);
  const [recursive, setRecursive] = useState(true);
  const [scanProgress, setScanProgress] = useState<{ total: number; done: number; skipped: number } | null>(null);

  // 从后端 config 同步 watchPaths / recursive 到前端状态（首次加载时）
  useEffect(() => {
    if (store.config.watch_paths && store.config.watch_paths.length > 0) {
      setWatchPaths(store.config.watch_paths);
    }
    setRecursive(store.config.watch_recursive ?? true);
  }, [store.config.watch_paths, store.config.watch_recursive]);

  // 监听扫描进度事件
  useEffect(() => {
    const unlistenProgress = listen<{ total: number; done: number; skipped: number; cancelled: boolean }>(
      "batch-scan-progress",
      (e) => {
        const { total, done, skipped, cancelled } = e.payload;
        if (cancelled) {
          setScanProgress(null);
        } else {
          setScanProgress({ total, done, skipped });
        }
      }
    );
    const unlistenDone = listen<{ total: number; done: number; skipped: number; cancelled: boolean }>(
      "batch-scan-done",
      (e) => {
        const { total, done, skipped, cancelled } = e.payload;
        if (cancelled) {
          toast.info(`扫描已取消（已检查 ${done}/${total}，跳过 ${skipped}）`);
        } else if (total === 0) {
          toast.info("监视目录中未找到视频文件");
        } else {
          toast.success(`扫描完成：共 ${total} 个文件，跳过 ${skipped} 个`);
        }
        setScanProgress(null);
      }
    );
    return () => {
      unlistenProgress.then((fn) => fn()).catch(() => {});
      unlistenDone.then((fn) => fn()).catch(() => {});
    };
  }, []);

  const handleAddWatchFolder = useCallback(async () => {
    const result = await open({ directory: true, multiple: true });
    if (result && result.length > 0) {
      setWatchPaths((prev) => [...prev, ...(result as string[])]);
    }
  }, []);

  const handleAddFiles = useCallback(async () => {
    const result = await open({
      multiple: true,
      filters: [
        { name: "视频/字幕", extensions: ["mp4", "mkv", "avi", "mov", "flv", "webm", "m4v", "wmv", "srt", "ass", "vtt", "ssa"] },
      ],
    });
    if (result && result.length > 0) {
      await store.addFilesToQueue(result as string[]);
    }
  }, [store]);

  const handleStartWatch = useCallback(async () => {
    if (watchPaths.length === 0) return;
    await store.startWatch(watchPaths, recursive);
  }, [watchPaths, recursive, store]);

  const handleStopWatch = useCallback(async () => {
    await store.stopWatch();
  }, [store]);

  const handleRemovePath = useCallback((path: string) => {
    setWatchPaths((prev) => prev.filter((p) => p !== path));
  }, []);

  const handleScanExisting = useCallback(async () => {
    if (watchPaths.length === 0) {
      toast.error("请先添加监视目录");
      return;
    }
    await store.scanExistingFiles(watchPaths, recursive);
  }, [watchPaths, recursive, store]);

  const handleCancelScan = useCallback(async () => {
    await store.cancelScan();
  }, [store]);

  const scanPct = scanProgress && scanProgress.total > 0
    ? Math.round((scanProgress.done / scanProgress.total) * 100)
    : 0;

  return (
    <Card>
      <CardHeader>
        <div className="flex items-center justify-between">
          <CardTitle className="text-base">文件夹监视</CardTitle>
          <div className="flex gap-2">
            {scanProgress ? (
              <Button variant="outline" size="sm" onClick={handleCancelScan} title="取消扫描检查">
                <XCircle className="h-4 w-4 mr-1" />
                取消检查
              </Button>
            ) : (
              <Button
                variant="outline"
                size="sm"
                onClick={handleScanExisting}
                disabled={watchPaths.length === 0}
                title="扫描监视目录中所有已有文件，检查外挂/内嵌字幕，不需要翻译的自动跳过"
              >
                <Search className="h-4 w-4 mr-1" />
                检查已有文件
              </Button>
            )}
            {store.isWatching ? (
              <Button variant="outline" size="sm" onClick={handleStopWatch}>
                <Square className="h-4 w-4 mr-1" />
                停止监视
              </Button>
            ) : (
              <Button
                variant="outline"
                size="sm"
                onClick={handleStartWatch}
                disabled={watchPaths.length === 0}
              >
                <Play className="h-4 w-4 mr-1" />
                开始监视
              </Button>
            )}
          </div>
        </div>
      </CardHeader>
      <CardContent className="space-y-3">
        <div className="flex gap-2">
          <Button variant="outline" size="sm" onClick={handleAddWatchFolder}>
            <FolderOpen className="h-4 w-4 mr-1" />
            添加监视目录
          </Button>
          <Button variant="outline" size="sm" onClick={handleAddFiles} title="手动选取视频或字幕文件加入翻译队列">
            <FileVideo className="h-4 w-4 mr-1" />
            添加文件
          </Button>
        </div>

        {watchPaths.length > 0 && (
          <div className="space-y-1">
            {watchPaths.map((path) => (
              <div
                key={path}
                className="flex items-center gap-2 p-2 border rounded text-sm"
              >
                <FolderOpen className="h-4 w-4 flex-shrink-0 text-muted-foreground" />
                <span className="flex-1 truncate" title={path}>
                  {path}
                </span>
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-6 w-6 flex-shrink-0"
                  onClick={() => handleRemovePath(path)}
                >
                  <X className="h-3 w-3" />
                </Button>
              </div>
            ))}
          </div>
        )}

        <div className="flex items-center gap-2">
          <input
            type="checkbox"
            checked={recursive}
            onChange={(e) => setRecursive(e.target.checked)}
            id="recursive"
            className="h-4 w-4"
          />
          <label htmlFor="recursive" className="text-sm cursor-pointer">
            递归监视子目录
          </label>
        </div>

        {/* 扫描进度条 */}
        {scanProgress && (
          <div className="space-y-1">
            <div className="flex items-center justify-between text-xs text-muted-foreground">
              <span>正在检查 {scanProgress.done}/{scanProgress.total}（跳过 {scanProgress.skipped}）</span>
              <span>{scanPct}%</span>
            </div>
            <Progress value={scanPct} className="h-2" />
          </div>
        )}

        {store.isWatching && (
          <div className="p-2 bg-green-50 dark:bg-green-950/30 border border-green-200 dark:border-green-800 rounded text-sm">
            <span className="flex items-center gap-1">
              <CheckCircle2 className="h-4 w-4 text-green-500" />
              正在监视 {store.config.watch_paths.length} 个目录
            </span>
          </div>
        )}
      </CardContent>
    </Card>
  );
}

// === SECTION 3 END ===

// === SECTION 4: BatchConfigSection 组件 ===

function BatchConfigSection() {
  const store = useBatchStore();
  const [localConfig, setLocalConfig] = useState<BatchConfig | null>(null);

  // 从 store 同步配置到本地编辑状态
  useEffect(() => {
    setLocalConfig(store.config);
  }, [store.config]);

  const update = useCallback((patch: Partial<BatchConfig>) => {
    setLocalConfig((prev) => (prev ? { ...prev, ...patch } : prev));
  }, []);

  const handleSave = useCallback(async () => {
    if (localConfig) {
      await store.saveConfig(localConfig);
    }
  }, [localConfig, store]);

  if (!localConfig) return null;

  return (
    <Card>
      <CardHeader>
        <div className="flex items-center justify-between">
          <CardTitle className="text-base">翻译配置</CardTitle>
          <Button size="sm" onClick={handleSave}>
            保存配置
          </Button>
        </div>
      </CardHeader>
      <CardContent className="space-y-4">
        {/* 源语言优先级列表 / 目标语言 */}
        <div className="space-y-3">
          {/* 源语言：可拖动排序的优先级列表 */}
          <SourceLangPriorityList config={localConfig} update={update} />

          {/* 目标语言：单选 */}
          <div className="space-y-1">
            <label className="text-sm font-medium">目标语言</label>
            <Select
              value={localConfig.target_lang}
              onValueChange={(v) => update({ target_lang: v })}
            >
              <SelectTrigger className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="zh">中文</SelectItem>
                <SelectItem value="en">English</SelectItem>
                <SelectItem value="ja">日本語</SelectItem>
                <SelectItem value="ko">한국어</SelectItem>
              </SelectContent>
            </Select>
          </div>

          {/* 不翻译的语言：多选 toggle 按钮 */}
          <div className="space-y-1">
            <label className="text-sm font-medium">不翻译的语言（多选）</label>
            <p className="text-xs text-muted-foreground">
              检测到字幕含以下任一语言（外挂文件名/内嵌流/内容字符）则跳过翻译
            </p>
            <div className="flex flex-wrap gap-1.5">
              {SKIP_LANG_OPTIONS.map((lang) => {
                const selected = (localConfig.skip_langs ?? []).includes(lang.value);
                return (
                  <button
                    key={lang.value}
                    type="button"
                    onClick={() => {
                      const cur = localConfig.skip_langs ?? [];
                      const next = selected
                        ? cur.filter((v) => v !== lang.value)
                        : [...cur, lang.value];
                      update({ skip_langs: next });
                    }}
                    className={`px-2.5 py-1 rounded-md text-xs border transition-colors ${
                      selected
                        ? "bg-primary text-primary-foreground border-primary"
                        : "bg-background text-muted-foreground border-input hover:bg-accent"
                    }`}
                  >
                    {lang.label}
                  </button>
                );
              })}
            </div>
          </div>
        </div>

        {/* 翻译引擎（与字幕编辑页相同逻辑：动态加载已配置引擎+AI模型） */}
        <BatchEngineSelect config={localConfig} update={update} />

        {/* 输出模式：单语/双语 */}
        <div className="space-y-1">
          <label className="text-sm font-medium">输出模式</label>
          <Select
            value={localConfig.output_mode}
            onValueChange={(v) => update({ output_mode: v as OutputMode })}
          >
            <SelectTrigger className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="Monolingual">单语（仅译文）</SelectItem>
              <SelectItem value="Bilingual">双语（原文+译文）</SelectItem>
            </SelectContent>
          </Select>
        </div>

        {/* 输出格式：多选 toggle 按钮 */}
        <div className="space-y-1">
          <label className="text-sm font-medium">输出格式（可多选，同时生成多种格式）</label>
          <div className="flex flex-wrap gap-1.5">
            {([
              { value: "srt", label: "SRT" },
              { value: "ass", label: "ASS" },
              { value: "vtt", label: "VTT" },
            ] as const).map((fmt) => {
              const selected = (localConfig.output_formats ?? []).includes(fmt.value);
              return (
                <button
                  key={fmt.value}
                  type="button"
                  onClick={() => {
                    const cur = localConfig.output_formats ?? [];
                    const next = selected
                      ? cur.filter((v) => v !== fmt.value)
                      : [...cur, fmt.value];
                    // 至少保留一个格式
                    const final = next.length > 0 ? next : cur;
                    update({
                      output_formats: final,
                      output_format: final[0] ?? localConfig.output_format,
                    });
                  }}
                  className={`px-2.5 py-1 rounded-md text-xs border transition-colors ${
                    selected
                      ? "bg-primary text-primary-foreground border-primary"
                      : "bg-background text-muted-foreground border-input hover:bg-accent"
                  }`}
                >
                  {fmt.label}
                </button>
              );
            })}
          </div>
        </div>

        {/* 嵌入视频：独立 checkbox */}
        <div className="flex items-center gap-2">
          <input
            type="checkbox"
            checked={localConfig.embed_to_video ?? false}
            onChange={(e) => update({ embed_to_video: e.target.checked })}
            id="embed_to_video"
            className="h-4 w-4"
          />
          <label htmlFor="embed_to_video" className="text-sm cursor-pointer">
            嵌入视频（将字幕合并到 mkv 文件）
          </label>
        </div>

        {/* 并发数 */}
        <div className="grid grid-cols-2 gap-3">
          <div className="flex items-center gap-2">
            <label className="text-sm font-medium whitespace-nowrap">文件并发数</label>
            <input
              type="number"
              value={localConfig.file_concurrency}
              onChange={(e) => update({ file_concurrency: parseInt(e.target.value) || 1 })}
              min={1}
              max={5}
              className="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm"
            />
          </div>
          <div className="flex items-center gap-2">
            <label className="text-sm font-medium whitespace-nowrap">条目并发数</label>
            <input
              type="number"
              value={localConfig.entry_concurrency}
              onChange={(e) => update({ entry_concurrency: parseInt(e.target.value) || 1 })}
              min={1}
              max={20}
              className="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm"
            />
          </div>
        </div>
        <div className="grid grid-cols-2 gap-3 -mt-1">
          <p className="text-xs text-muted-foreground">同时处理几个文件</p>
          <p className="text-xs text-muted-foreground">单文件内翻译并发</p>
        </div>

        {/* 假文件检测 */}
        <div className="grid grid-cols-2 gap-3">
          <div className="flex items-center gap-2">
            <label className="text-sm font-medium whitespace-nowrap">最小文件大小(MB)</label>
            <input
              type="number"
              value={localConfig.min_file_size_mb}
              onChange={(e) => update({ min_file_size_mb: parseInt(e.target.value) || 1 })}
              min={0}
              className="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm"
            />
          </div>
          <div className="flex items-center gap-2">
            <label className="text-sm font-medium whitespace-nowrap">最小时长(秒)</label>
            <input
              type="number"
              value={localConfig.min_duration_secs}
              onChange={(e) => update({ min_duration_secs: parseFloat(e.target.value) || 10 })}
              min={0}
              step={1}
              className="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm"
            />
          </div>
        </div>

        {/* 选项开关 */}
        <div className="space-y-2">
          <div className="flex items-center gap-2">
            <input
              type="checkbox"
              checked={localConfig.check_external}
              onChange={(e) => update({ check_external: e.target.checked })}
              id="check_external"
              className="h-4 w-4"
            />
            <label htmlFor="check_external" className="text-sm cursor-pointer">
              检查外挂字幕（已有目标语言字幕则跳过）
            </label>
          </div>
          <div className="flex items-center gap-2">
            <input
              type="checkbox"
              checked={localConfig.check_embedded}
              onChange={(e) => update({ check_embedded: e.target.checked })}
              id="check_embedded"
              className="h-4 w-4"
            />
            <label htmlFor="check_embedded" className="text-sm cursor-pointer">
              检查内嵌字幕（已有目标语言字幕流则跳过）
            </label>
          </div>
          <div className="flex items-center gap-2">
            <input
              type="checkbox"
              checked={localConfig.skip_cache}
              onChange={(e) => update({ skip_cache: e.target.checked })}
              id="skip_cache"
              className="h-4 w-4"
            />
            <label htmlFor="skip_cache" className="text-sm cursor-pointer">
              跳过缓存（强制重新翻译）
            </label>
          </div>
          <div className="flex items-center gap-2">
            <input
              type="checkbox"
              checked={localConfig.scan_on_start}
              onChange={(e) => update({ scan_on_start: e.target.checked })}
              id="scan_on_start"
              className="h-4 w-4"
            />
            <label htmlFor="scan_on_start" className="text-sm cursor-pointer">
              启动监视时扫描已有文件
            </label>
          </div>
        </div>

        {/* 工作时间设定 */}
        <ScheduleSection config={localConfig} update={update} />
      </CardContent>
    </Card>
  );
}

// === SECTION 4 END ===

// === SECTION 4.1: SourceLangPriorityList 可拖动排序的源语言优先级列表 ===

function SortableLangItem({ lang, index, onRemove }: {
  lang: string;
  index: number;
  onRemove: () => void;
}) {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({ id: lang });
  const style = {
    transform: CSS.Transform.toString(transform),
    transition,
    opacity: isDragging ? 0.5 : 1,
  };
  return (
    <div ref={setNodeRef} style={style} className="flex items-center gap-2 px-2 py-1.5 rounded-md border bg-background">
      <button type="button" {...attributes} {...listeners} className="cursor-grab active:cursor-grabbing text-muted-foreground">
        <GripVertical className="h-4 w-4" />
      </button>
      <span className="text-xs text-muted-foreground w-5">{index + 1}.</span>
      <span className="text-sm flex-1">{LANG_LABELS[lang] ?? lang}</span>
      <button type="button" onClick={onRemove} className="text-muted-foreground hover:text-destructive">
        <X className="h-3.5 w-3.5" />
      </button>
    </div>
  );
}

function SourceLangPriorityList({ config, update }: {
  config: BatchConfig;
  update: (patch: Partial<BatchConfig>) => void;
}) {
  const sensors = useSensors(
    useSensor(PointerSensor),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates })
  );
  const sourceLangs = config.source_langs ?? [];
  const availableToAdd = LANG_OPTIONS.filter((l) => !sourceLangs.includes(l.value));

  const handleDragEnd = useCallback((event: DragEndEvent) => {
    const { active, over } = event;
    if (over && active.id !== over.id) {
      const oldIndex = sourceLangs.indexOf(active.id as string);
      const newIndex = sourceLangs.indexOf(over.id as string);
      const next = arrayMove(sourceLangs, oldIndex, newIndex);
      update({ source_langs: next, source_lang: next[0] ?? "auto" });
    }
  }, [sourceLangs, update]);

  const handleAdd = useCallback((lang: string) => {
    const next = [...sourceLangs, lang];
    update({ source_langs: next, source_lang: next[0] ?? lang });
  }, [sourceLangs, update]);

  const handleRemove = useCallback((lang: string) => {
    const next = sourceLangs.filter((v) => v !== lang);
    update({ source_langs: next, source_lang: next[0] ?? "auto" });
  }, [sourceLangs, update]);

  return (
    <div className="space-y-1">
      <label className="text-sm font-medium">源语言（优先级从上到下）</label>
      <p className="text-xs text-muted-foreground">
        翻译时按优先级顺序检测字幕内容，传第一个匹配的语言给引擎
      </p>
      <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={handleDragEnd}>
        <SortableContext items={sourceLangs} strategy={verticalListSortingStrategy}>
          <div className="space-y-1.5">
            {sourceLangs.map((lang, i) => (
              <SortableLangItem
                key={lang}
                lang={lang}
                index={i}
                onRemove={() => handleRemove(lang)}
              />
            ))}
          </div>
        </SortableContext>
      </DndContext>
      {availableToAdd.length > 0 && (
        <div className="flex flex-wrap gap-1.5 pt-1">
          {availableToAdd.map((lang) => (
            <button
              key={lang.value}
              type="button"
              onClick={() => handleAdd(lang.value)}
              className="px-2 py-0.5 rounded-md text-xs border border-dashed border-input text-muted-foreground hover:bg-accent"
            >
              + {lang.label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

// === SECTION 4.1 END ===

// === SECTION 4.2: BatchEngineSelect 翻译引擎下拉框（与字幕编辑页相同逻辑）===

function BatchEngineSelect({ config, update }: {
  config: BatchConfig;
  update: (patch: Partial<BatchConfig>) => void;
}) {
  const navigate = useNavigate();
  const [providerConfigured, setProviderConfigured] = useState<Record<string, boolean>>({});
  const [aiServiceModels, setAiServiceModels] = useState<{ serviceId: string; serviceName: string; model: string; modelType: string }[]>([]);

  // 加载已配置的引擎和 AI 模型
  useEffect(() => {
    const traditionalProviders = ["baidu", "bing", "google"];
    const aiServices = SERVICES.filter((s) => s.category === "ai");

    const traditionalPromise = Promise.all(traditionalProviders.map(async (p) => {
      try {
        const [appId, secretKeyring, secretConfig] = await Promise.all([
          api.getConfig(`translate_${p}_app_id`).catch(() => null),
          api.getCredential(p, "secret", `批量翻译检查配置状态(${p})`).catch(() => null),
          api.getConfig(`translate_${p}_secret`).catch(() => null),
        ]);
        const configured = !!(appId && (secretKeyring || secretConfig));
        return [p, configured] as [string, boolean];
      } catch {
        return [p, false] as [string, boolean];
      }
    }));

    const aiPromise = Promise.all(aiServices.map(async (s) => {
      try {
        const [baseUrl, selectedModels] = await Promise.all([
          api.getConfig(`translate_openai_${s.id}_base_url`).catch(() => null),
          api.getConfig(`translate_openai_${s.id}_selected_models`).catch(() => null),
        ]);
        if (!baseUrl || !selectedModels) return [s.id, false] as [string, boolean];
        if (s.requiresApiKey) {
          const apiKey = await api.getCredential(`openai_${s.id}`, "secret", `批量翻译检查配置状态(${s.id})`).catch(() => null);
          if (!apiKey) return [s.id, false] as [string, boolean];
        }
        return [s.id, true] as [string, boolean];
      } catch {
        return [s.id, false] as [string, boolean];
      }
    }));

    Promise.all([traditionalPromise, aiPromise]).then(([tradResults, aiResults]) => {
      setProviderConfigured(Object.fromEntries([...tradResults, ...aiResults]));
    });

    // 加载 AI 模型列表
    Promise.all(aiServices.map(async (s) => {
      const [baseUrl, selectedModels, modelTypes] = await Promise.all([
        api.getConfig(`translate_openai_${s.id}_base_url`).catch(() => null),
        api.getConfig(`translate_openai_${s.id}_selected_models`).catch(() => null),
        api.getConfig(`translate_openai_${s.id}_selected_model_types`).catch(() => null),
      ]);
      if (!baseUrl || !selectedModels) return [];
      const ids = selectedModels.split(",").filter(Boolean);
      let typeMap: Record<string, string> = {};
      try { typeMap = JSON.parse(modelTypes || "{}"); } catch { /* ignore */ }
      return ids.map((id) => ({
        serviceId: s.id,
        serviceName: s.name,
        model: id,
        modelType: typeMap[id] || "generic",
      }));
    })).then((results) => {
      setAiServiceModels(results.flat());
    });
  }, []);

  // 当前选中值编码
  const selectValue = config.provider === "openai" && config.model
    ? encodeAiSelectValue(config.service_id || "openai", config.model)
    : config.provider === "openai" ? "" : (config.provider || undefined);

  const handleValueChange = useCallback((val: string) => {
    if (val === "__add_more__") {
      navigate("/settings?tab=translate");
      return;
    }
    const decoded = decodeAiSelectValue(val);
    if (decoded) {
      const { serviceId, model } = decoded;
      const found = aiServiceModels.find((m) => m.serviceId === serviceId && m.model === model);
      update({
        provider: "openai",
        service_id: serviceId,
        model: model,
        model_type: found?.modelType || "generic",
      });
    } else {
      update({
        provider: val,
        service_id: null,
        model: null,
        model_type: null,
      });
    }
  }, [aiServiceModels, update, navigate]);

  return (
    <div className="space-y-1">
      <label className="text-sm font-medium">翻译引擎</label>
      <Select value={selectValue} onValueChange={handleValueChange}>
        <SelectTrigger className="w-full">
          <SelectValue placeholder="无可用引擎" />
        </SelectTrigger>
        <SelectContent>
          {providerConfigured["baidu"] && (
            <SelectItem value="baidu">百度翻译</SelectItem>
          )}
          {providerConfigured["bing"] && (
            <SelectItem value="bing">Bing</SelectItem>
          )}
          {providerConfigured["google"] && (
            <SelectItem value="google">Google</SelectItem>
          )}
          {aiServiceModels.map((m) => {
            const value = encodeAiSelectValue(m.serviceId, m.model);
            return (
              <SelectItem key={value} value={value}>
                <span className="block truncate" title={`AI模型 - ${m.serviceName} - ${m.model}`}>
                  AI模型 - {m.serviceName} - {m.model}
                </span>
              </SelectItem>
            );
          })}
          <SelectItem value="__add_more__">
            <span className="flex items-center gap-1 text-primary">
              <Plus className="h-3 w-3" />
              添加更多引擎
            </span>
          </SelectItem>
        </SelectContent>
      </Select>
    </div>
  );
}

// === SECTION 4.2 END ===

// === SECTION 5: ScheduleSection 工作时间设定 ===

function ScheduleSection({
  config,
  update,
}: {
  config: BatchConfig;
  update: (patch: Partial<BatchConfig>) => void;
}) {
  const isAlways = config.schedule === "Always";

  // 从 TimeWindow 提取当前值
  const tw = !isAlways && typeof config.schedule === "object" && "TimeWindow" in config.schedule
    ? config.schedule.TimeWindow
    : null;
  const windows = tw ? tw.windows : [];
  const weekdays = tw ? tw.weekdays : [];

  const setScheduleMode = (always: boolean) => {
    if (always) {
      update({ schedule: "Always" });
    } else {
      update({
        schedule: {
          TimeWindow: {
            windows: windows.length > 0 ? windows : [[9, 17]],
            weekdays: weekdays.length > 0 ? weekdays : [0, 1, 2, 3, 4, 5, 6],
          },
        },
      });
    }
  };

  const addWindow = () => {
    const newWindows = [...windows, [9, 17] as [number, number]];
    update({
      schedule: {
        TimeWindow: { windows: newWindows, weekdays },
      },
    });
  };

  const removeWindow = (idx: number) => {
    const newWindows = windows.filter((_, i) => i !== idx);
    update({
      schedule: {
        TimeWindow: { windows: newWindows, weekdays },
      },
    });
  };

  const updateWindow = (idx: number, field: 0 | 1, value: number) => {
    const newWindows = windows.map((w, i) =>
      i === idx ? (field === 0 ? [value, w[1]] : [w[0], value]) as [number, number] : w
    );
    update({
      schedule: {
        TimeWindow: { windows: newWindows, weekdays },
      },
    });
  };

  const toggleWeekday = (day: number) => {
    const newWeekdays = weekdays.includes(day)
      ? weekdays.filter((d) => d !== day)
      : [...weekdays, day].sort();
    update({
      schedule: {
        TimeWindow: { windows, weekdays: newWeekdays },
      },
    });
  };

  const dayNames = ["日", "一", "二", "三", "四", "五", "六"];

  return (
    <div className="space-y-3 border-t pt-3">
      <div className="flex items-center gap-2">
        <input
          type="checkbox"
          checked={isAlways}
          onChange={(e) => setScheduleMode(e.target.checked)}
          id="schedule_always"
          className="h-4 w-4"
        />
        <label htmlFor="schedule_always" className="text-sm font-medium cursor-pointer">
          全天运行（不限制工作时间）
        </label>
      </div>

      {!isAlways && (
        <div className="space-y-3 pl-4 border-l-2 ml-2">
          {/* 时间窗口 */}
          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <label className="text-sm font-medium">工作时间窗口</label>
              <Button variant="outline" size="sm" onClick={addWindow} type="button">
                + 添加时段
              </Button>
            </div>
            {windows.map((w, idx) => (
              <div key={idx} className="flex items-center gap-2">
                <input
                  type="number"
                  value={w[0]}
                  onChange={(e) => updateWindow(idx, 0, parseInt(e.target.value) || 0)}
                  min={0}
                  max={23}
                  className="w-16 h-8 rounded-md border border-input bg-transparent px-2 text-sm text-center"
                />
                <span className="text-sm">:00 ～</span>
                <input
                  type="number"
                  value={w[1]}
                  onChange={(e) => updateWindow(idx, 1, parseInt(e.target.value) || 0)}
                  min={0}
                  max={23}
                  className="w-16 h-8 rounded-md border border-input bg-transparent px-2 text-sm text-center"
                />
                <span className="text-sm">:00</span>
                {w[1] <= w[0] && (
                  <span className="text-xs text-orange-500">（跨午夜）</span>
                )}
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-7 w-7"
                  onClick={() => removeWindow(idx)}
                  type="button"
                >
                  <X className="h-3 w-3" />
                </Button>
              </div>
            ))}
            <p className="text-xs text-muted-foreground">
              当结束时间 ≤ 开始时间时视为跨午夜（如 22:00～2:00 = 22-24 + 0-2）
            </p>
          </div>

          {/* 星期选择 */}
          <div className="space-y-2">
            <label className="text-sm font-medium">运行日期</label>
            <div className="flex gap-1">
              {dayNames.map((name, day) => (
                <button
                  key={day}
                  type="button"
                  onClick={() => toggleWeekday(day)}
                  className={`w-8 h-8 rounded-full text-sm border transition-colors ${
                    weekdays.includes(day)
                      ? "bg-primary text-primary-foreground border-primary"
                      : "bg-transparent border-input hover:bg-accent"
                  }`}
                >
                  {name}
                </button>
              ))}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

// === SECTION 5 END ===
