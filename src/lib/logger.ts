// 日志工具：只有开启开发者模式后才输出到 console
// devModeStore 在初始化/切换时调用 setDevModeEnabled 同步状态
// 避免循环依赖：logger 不导入任何 store，由 store 主动推送状态

let devModeEnabled = false;

/** 设置开发者模式开关（由 devModeStore 调用） */
export function setDevModeEnabled(v: boolean): void {
  devModeEnabled = v;
}

/** 开发者模式下输出日志 */
export function log(...args: unknown[]): void {
  if (devModeEnabled) console.log(...args);
}

/** 开发者模式下输出警告 */
export function warn(...args: unknown[]): void {
  if (devModeEnabled) console.warn(...args);
}

/** 开发者模式下输出错误 */
export function error(...args: unknown[]): void {
  if (devModeEnabled) console.error(...args);
}

/** 开发者模式下输出调试信息 */
export function debug(...args: unknown[]): void {
  if (devModeEnabled) console.debug(...args);
}
