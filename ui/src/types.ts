/**
 * 与后端约定的 invoke 接口类型（后端按此实现，勿擅自改动字段名）。
 *
 * ⚠️ 本文件是 P0+ 五子项的共享契约，由集成负责人在并行开发前写死；
 *    子项 agent 只读不改，避免 5 路并行改同一文件冲突。
 */

/* ============================ 通用 ============================ */

export type SearchMode = 'fuzzy' | 'exact' | 'dsl' | 'regex'
export type SortKey = 'relevance' | 'mtime' | 'name'

/** 结果行 */
export interface SearchResult {
  /** 文件名 */
  name: string
  /** 绝对路径 */
  path: string
  /** 修改时间（unix 秒） */
  mtime: number
  /** 文件大小（字节） */
  size: number
  /** 命中次数 */
  hits: number
  /** 首个匹配片断（含上下文） */
  snippet: string
}

/** 索引统计（状态栏显示） */
export interface IndexStats {
  /** 文档数 */
  docs: number
  /** 索引体积（人类可读，如 "12.3 MB"） */
  size: string
  /** 索引是否可用；false = 索引缺失/损坏，UI 提示重建 */
  available: boolean
}

/* ==================== 子项 1：搜索模式与 DSL ==================== */

export interface SearchRequest {
  query: string
  mode: SearchMode
  /** 反向结果 */
  reverse: boolean
  /** 指定目录；null = 全部目录 */
  dir: string | null
  /** 指定文件类型名；null = 全部文件类型 */
  fileType: string | null
  sort: SortKey
  /** 页码（0 起，R3 分页） */
  page: number
  /** 每页条数（R3：替代旧 limit） */
  pageSize: number
  /** 修改时间下界（unix 秒，闭区间）；null = 不限（R3） */
  mtimeFrom: number | null
  /** 修改时间上界（unix 秒，闭区间）；null = 不限（R3） */
  mtimeTo: number | null
}

/** 分页搜索响应（R3） */
export interface SearchResponse {
  hits: SearchResult[]
  total: number
  page: number
  pageSize: number
  /** true = 熔断截断（regex 三上限），结果可能不完整 */
  partial: boolean
}

/* ==================== R3：搜索历史 / 书签 ==================== */

export interface HistoryEntry {
  id: number
  query: string
  mode: SearchMode
  resultCount: number
  ts: number
}

export interface Bookmark {
  id: number
  path: string
  name: string
  created: number
}

/* ================== 子项 2：结果交互与文件操作 ================== */

/** 批量文件操作结果（部分失败不中断其余项） */
export interface FileOpResult {
  /** 成功条数 */
  ok: number
  /** 失败明细 */
  failed: Array<{ path: string; error: string }>
}

/** 导出 xlsx 的请求（行数据由后端搜索层再取一次，前端只传查询与目标列） */
export interface ExportRequest {
  rows: SearchResult[]
  /** 目标文件绝对路径 */
  dest: string
}

/* ====================== 子项 3：索引管理 ====================== */

export interface FileType {
  id: number
  /** 类型名，如 "文本" / "Office 文档" */
  name: string
  /** 扩展名列表（不含点，小写），如 ["txt", "md"] */
  exts: string[]
  /** 内置类型不可删 */
  builtin: boolean
}

/** per-format 目录规则三态（S12；语义与 ignore/globset 对齐） */
export type RuleState = 'include' | 'exclude' | 'inherit'

export interface DirRule {
  id: number
  /** 归属文件类型；null = 对所有类型生效 */
  fileTypeId: number | null
  /** glob 模式，如 "**\/node_modules\/**" */
  pattern: string
  state: RuleState
}

/** 定时更新三态 */
export type ScheduleMode = 'never' | 'interval' | 'daily'

export interface ScheduleConfig {
  mode: ScheduleMode
  /** mode=interval 时的间隔分钟数 */
  intervalMinutes: number
  /** mode=daily 时的触发时刻，格式 "HH:mm" */
  dailyAt: string
}

export interface StorageInfo {
  current: string
  docs: number
  size: string
  /** 索引是否可用；false = 索引缺失/损坏，UI 提示重建 */
  available: boolean
}

/** 全量索引一个目录的回报（index_directory 命令） */
export interface ScanReport {
  /** 扫描到的文件数 */
  scanned: number
  /** 实际入库的文档数 */
  indexed: number
}

/** 重建索引进度事件负载 */
export interface RebuildProgress {
  done: number
  total: number
  /** 当前正在处理的文件 */
  current: string
  /** 阶段标记（R2）：indexing=索引中 / skipped=本轮定时更新被跳过 */
  phase?: string
}

/* ---------------- P3-B：PDF 页视图 ---------------- */

/** PDF 页内文本块（归一化坐标 0..1，原点左上） */
export interface PageTextBox {
  text: string
  x: number
  y: number
  w: number
  h: number
}

/* ================== 子项 4：预览交互全家桶 ================== */

export interface DocumentPayload {
  path: string
  /** 全文纯文本（可能被截断） */
  text: string
  totalLines: number
  totalBytes: number
  /** 是否因体积上限被截断 */
  truncated: boolean
}

/** 词组统计行（jieba 词频） */
export interface WordStat {
  word: string
  count: number
}

/** 单条匹配（offset 为 UTF-8 字节偏移，与高亮/minimap 三段共用） */
export interface MatchItem {
  /** 1-based 行号 */
  line: number
  /** 行内起始字节偏移 */
  start: number
  /** 行内结束字节偏移 */
  end: number
  /** 该匹配所在行的完整文本（供匹配列表展示） */
  lineText: string
}

export interface MatchGroup {
  term: string
  count: number
  matches: MatchItem[]
}

export interface FileInfo {
  name: string
  path: string
  size: number
  mtime: number
  ext: string
  /** 人类可读类型描述，如 "纯文本" / "ZIP 压缩包" */
  kind: string
  lines: number
  /** 检测到的编码，如 "UTF-8" / "GBK"；二进制/未知为 null */
  encoding: string | null
}

/** 文档内再搜索选项 */
export interface FindOptions {
  caseSensitive: boolean
  wholeWord: boolean
  regex: boolean
}

export interface FindMatch {
  /** 全文起始字节偏移 */
  start: number
  end: number
  /** 1-based 行号 */
  line: number
}

/** 预览多文档 Tab */
export interface DocTab {
  /** 唯一 id（用路径） */
  id: string
  path: string
  title: string
}

/**
 * 预览显示选项
 *
 * 所有权：App.tsx 持有（查看菜单 + 全局快捷键驱动），
 * 由 props 下发给 PreviewPane；PreviewPane 只在 Ctrl+滚轮时经 onViewChange 回写。
 */
export interface PreviewViewOpts {
  /** 显示行号槽 */
  lineNumbers: boolean
  /** 自动换行；false = 长行横向滚动 */
  wrap: boolean
  /** 正文字号 px */
  fontSize: number
}

/* ===================== 子项 5：应用外壳 ===================== */

export type ThemeMode = 'light' | 'dark' | 'system'

export interface HotkeyBinding {
  /** 动作标识，如 "focusSearch" */
  action: string
  /** 展示名，如 "聚焦搜索框" */
  label: string
  /** 加速键串，如 "Ctrl+L" */
  accel: string
}

export interface AppConfig {
  theme: ThemeMode
  language: 'zh-CN' | 'en-US'
  autostart: boolean
  /** 关闭主窗口时最小化到托盘 */
  closeToTray: boolean
  /** 右击目录搜索（shell 集成）开关（P5） */
  rightClickSearch: boolean
  /**
   * OCR 识别开关（图片 / 扫描型 PDF）。**默认关闭**。
   * 开启后，无文字层的扫描件才走 OCR；代价是索引耗时明显增加。
   */
  ocrEnabled: boolean
  hotkeys: HotkeyBinding[]
}

/** OCR 运行状态（设置页提示用） */
export interface OcrStatus {
  /** 当前开关值 */
  enabled: boolean
  /** helper 二进制是否就位（缺失时开关打开也不会有识别结果） */
  helperReady: boolean
}

/** 应用名与版本（「关于」面板） */
export interface AppInfo {
  name: string
  version: string
}

/* ================== 应用外壳：菜单/工具栏动作枚举 ================== */

/** 菜单栏动作（App.tsx 统一分发；子项 5 负责发出，App 负责响应） */
export type MenuAction =
  | 'file.export'
  | 'file.exit'
  | 'edit.copyPath'
  | 'edit.copyName'
  | 'edit.selectAll'
  | 'edit.findInDoc'
  | 'view.themeLight'
  | 'view.themeDark'
  | 'view.themeSystem'
  | 'view.prevPage'
  | 'view.nextPage'
  // 预览显示
  | 'view.lineNumbers'
  | 'view.wrap'
  | 'view.zoomIn'
  | 'view.zoomOut'
  | 'view.zoomReset'
  | 'settings.indexAdmin'
  | 'settings.options'
  | 'help.about'
  | 'help.homepage'

/** 工具栏图标动作 */
export type ToolbarAction =
  | 'nav.back'
  | 'nav.forward'
  | 'nav.refresh'
  | 'nav.clear'
  | 'edit.trash'
  | 'nav.up'
  | 'view.palette'
  | 'help.info'
  | 'help.homepage'
  | 'settings.options'
  | 'index.togglePause'

/** 索引运行态（工具栏指示灯） */
export type IndexRunState = 'idle' | 'indexing' | 'paused'
