/**
 * 应用组装器（集成负责人所有权）
 *
 * 五子项在此接线，组件本体各自归属见 ui-redesign.md。
 * 本文件只做：状态机 + 事件分发 + 组件组合，业务实现一律下沉到子项组件。
 */
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { listen } from '@tauri-apps/api/event'
import * as api from './api'
import type {
  DocTab,
  IndexRunState,
  IndexStats,
  MenuAction,
  PreviewViewOpts,
  RebuildProgress,
  SearchMode,
  SearchResult,
  SortKey,
  ThemeMode,
  ToolbarAction,
} from './types'
import { errMsg } from './util'
import AppToolbar from './components/shell/AppToolbar'
import MenuBar from './components/shell/MenuBar'
import SettingsDialog from './components/shell/SettingsDialog'
import AboutDialog from './components/shell/AboutDialog'
import StatusBar from './components/shell/StatusBar'
import ThemeProvider from './components/shell/ThemeProvider'
import WelcomePage from './components/shell/WelcomePage'
import FilterRow, { timeRangeBounds } from './components/search/FilterRow'
import SearchPanel from './components/search/SearchPanel'
import IndexAdminDialog from './components/index/IndexAdminDialog'
import PreviewPane from './components/preview/PreviewPane'
import ResultList from './components/results/ResultList'
import SelectionBar from './components/results/SelectionBar'
import BookmarkPanel from './components/results/BookmarkPanel'
import ExternalSearchListener from './components/shell/ExternalSearchListener'
import { useI18n } from './i18n'

const SEARCH_DEBOUNCE_MS = 150
const PAGE_SIZE = 50

/** 预览显示默认值（行号开、不换行、13px） */
const DEFAULT_PREVIEW_VIEW: PreviewViewOpts = { lineNumbers: true, wrap: false, fontSize: 13 }
/** 正文字号上下限 */
const FONT_SIZE_MIN = 8
const FONT_SIZE_MAX = 32

/** 数值夹取 */
function clamp(n: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, n))
}

export default function App() {
  /* ------------------------- 搜索条件 ------------------------- */
  const [query, setQuery] = useState('')
  const [mode, setMode] = useState<SearchMode>('fuzzy')
  const [reverse, setReverse] = useState(false)
  const [dir, setDir] = useState<string | null>(null)
  const [fileType, setFileType] = useState<string | null>(null)
  const [sort, setSort] = useState<SortKey>('relevance')
  /* R3：分页 / 时间区间 / 结果完整性 */
  const [page, setPage] = useState(0)
  const [timeRange, setTimeRange] = useState('all')
  const [total, setTotal] = useState(0)
  const [partial, setPartial] = useState(false)

  /* --------------------------- 结果 --------------------------- */
  const [results, setResults] = useState<SearchResult[]>([])
  const [activeIdx, setActiveIdx] = useState(-1)
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const [elapsedMs, setElapsedMs] = useState<number | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [stats, setStats] = useState<IndexStats | null>(null)

  /* --------------------------- 外壳 --------------------------- */
  const [theme, setTheme] = useState<ThemeMode>('system')
  const [indexState, setIndexState] = useState<IndexRunState>('idle')
  const [showIndexAdmin, setShowIndexAdmin] = useState(false)
  const [showSettings, setShowSettings] = useState(false)
  const [showAbout, setShowAbout] = useState(false)
  const [showWelcome, setShowWelcome] = useState(true)
  const [showBookmarks, setShowBookmarks] = useState(false)
  const [navStack, setNavStack] = useState<Array<{ dir: string | null; fileType: string | null }>>([])
  const [navPos, setNavPos] = useState(-1)

  /* -------------------------- 预览 Tab ------------------------- */
  const [tabs, setTabs] = useState<DocTab[]>([])
  const [activeTab, setActiveTab] = useState<string | null>(null)

  /* ----------------------- 预览显示选项 ----------------------- */
  const [view, setView] = useState<PreviewViewOpts>(DEFAULT_PREVIEW_VIEW)
  const patchView = useCallback((p: Partial<PreviewViewOpts>) => setView((v) => ({ ...v, ...p })), [])

  /* --------------------------- 内部 ref --------------------------- */
  const searchSeq = useRef(0)
  const manualRef = useRef(false)
  const [submitTick, setSubmitTick] = useState(0)
  /** 索引暂停态（工具栏与索引管理对话框共用；进度事件监听据此不回写） */
  const pausedRef = useRef(false)
  /** 文档内查找栏聚焦代际（菜单 edit.findInDoc → PreviewPane） */
  const [findTick, setFindTick] = useState(0)
  /** 导航历史应用守卫：正在把 navStack[navPos] 写回 dir/fileType 时屏蔽记录 effect，防回写死循环 */
  const applyingNavRef = useRef(false)

  /* ---------------- 索引运行态：唯一数据源为 index-progress 事件 ---------------- */
  useEffect(() => {
    let un: (() => void) | null = null
    let disposed = false
    void (async () => {
      const u = await listen<RebuildProgress>('index-progress', (ev) => {
        if (pausedRef.current) return // 暂停态由用户动作显式管理，不被进度事件覆盖
        const p = ev.payload
        setIndexState(p && p.current !== '' ? 'indexing' : 'idle')
      })
      if (disposed) u()
      else un = u
    })()
    return () => {
      disposed = true
      if (un) un()
    }
  }, [])

  const loadStats = useCallback(async () => {
    try {
      setStats(await api.getIndexStats())
    } catch {
      /* 索引统计失败不打扰主流程 */
    }
  }, [])

  useEffect(() => {
    void loadStats()
  }, [loadStats])

  /* 启动时读应用配置（皮肤等）+ 处理 CLI --open-dir（右击搜索入口，P5） */
  useEffect(() => {
    void (async () => {
      try {
        const cfg = await api.getAppConfig()
        setTheme(cfg.theme)
      } catch {
        /* 配置读取失败保持默认 */
      }
      try {
        const opened = await api.takeOpenDir()
        if (opened) {
          // 右击目录启动：预填目录过滤并触发一次搜索
          setDir(opened)
          setSubmitTick((t) => t + 1)
        }
      } catch {
        /* 无 --open-dir */
      }
    })()
  }, [])

  /* 主题落到 <html data-theme>；system 跟随系统 */
  useEffect(() => {
    const root = document.documentElement
    const mq = window.matchMedia('(prefers-color-scheme: dark)')
    const apply = () => {
      const resolved = theme === 'system' ? (mq.matches ? 'dark' : 'light') : theme
      root.dataset.theme = resolved
    }
    apply()
    mq.addEventListener('change', apply)
    return () => mq.removeEventListener('change', apply)
  }, [theme])

  /* 搜索条件变化 → 重置回第一页（R3 分页；置于搜索 effect 之前保证同批生效） */
  useEffect(() => {
    setPage(0)
  }, [query, mode, reverse, dir, fileType, sort, timeRange])

  /* 即输即搜：debounce 150ms（手动"开始"为 0 延迟）；
     空查询 + 全部时间 = 清空结果；空查询 + 时间档 = 浏览模式（按区间全库浏览） */
  useEffect(() => {
    const q = query.trim()
    const { from: mtimeFrom, to: mtimeTo } = timeRangeBounds(timeRange)
    if (!q && timeRange === 'all') {
      searchSeq.current++
      setResults([])
      setTotal(0)
      setPartial(false)
      setElapsedMs(null)
      setLoading(false)
      setError(null)
      setActiveIdx(-1)
      return
    }
    setLoading(true)
    const manual = manualRef.current
    manualRef.current = false
    const timer = setTimeout(() => {
      const seq = ++searchSeq.current
      const t0 = performance.now()
      api
        .search({
          query: q,
          mode,
          reverse,
          dir,
          fileType,
          sort,
          page,
          pageSize: PAGE_SIZE,
          mtimeFrom,
          mtimeTo,
        })
        .then((res) => {
          if (seq !== searchSeq.current) return
          setResults(res.hits)
          setTotal(res.total)
          setPartial(res.partial)
          setElapsedMs(performance.now() - t0)
          setError(null)
          setActiveIdx(res.hits.length > 0 ? 0 : -1)
        })
        .catch((e: unknown) => {
          if (seq !== searchSeq.current) return
          setResults([])
          setTotal(0)
          setPartial(false)
          setElapsedMs(null)
          setError(`搜索失败：${errMsg(e)}`)
          setActiveIdx(-1)
        })
        .finally(() => {
          if (seq === searchSeq.current) setLoading(false)
          void loadStats()
        })
    }, manual ? 0 : SEARCH_DEBOUNCE_MS)
    return () => clearTimeout(timer)
  }, [query, mode, reverse, dir, fileType, sort, page, timeRange, submitTick, loadStats])

  /* 选中行变化 → 打开/切换预览 Tab */
  useEffect(() => {
    if (activeIdx < 0 || activeIdx >= results.length) return
    const r = results[activeIdx]
    if (!r) return
    setShowWelcome(false)
    setTabs((prev) => (prev.some((t) => t.id === r.path) ? prev : [...prev, { id: r.path, path: r.path, title: r.name }]))
    setActiveTab(r.path)
  }, [activeIdx, results])

  const selectedPaths = useMemo(() => Array.from(selected), [selected])

  const { t } = useI18n()
  const statusText = loading
    ? t('status.searching')
    : !query.trim() && timeRange === 'all'
      ? t('status.ready')
      : elapsedMs !== null
        ? t('status.results', [total, (elapsedMs / 1000).toFixed(3)])
        : ''

  const statusPath = activeIdx >= 0 && activeIdx < results.length ? results[activeIdx].path : activeTab

  /* ----------------------- 事件：搜索面板 ----------------------- */

  const onSubmitSearch = useCallback(() => {
    manualRef.current = true
    setSubmitTick((t) => t + 1)
  }, [])

  const onArrowNav = useCallback(
    (step: 1 | -1) => {
      setActiveIdx((i) => {
        if (results.length === 0) return -1
        const next = i < 0 ? (step > 0 ? 0 : results.length - 1) : i + step
        if (next < 0) return results.length - 1
        if (next >= results.length) return 0
        return next
      })
    },
    [results.length],
  )

  /* ----------------------- 事件：预览 Tab ----------------------- */

  const closeTab = useCallback((id: string) => {
    setTabs((prev) => {
      const idx = prev.findIndex((t) => t.id === id)
      const next = prev.filter((t) => t.id !== id)
      setActiveTab((cur) => {
        if (cur !== id) return cur
        if (next.length === 0) return null
        return next[Math.min(idx, next.length - 1)].id
      })
      return next
    })
  }, [])

  /* ----------------------- 事件：导出 ----------------------- */

  const onExport = useCallback(async () => {
    if (results.length === 0) return
    try {
      await api.exportXlsx({ rows: results, dest: '' })
    } catch (e) {
      setError(`导出失败：${errMsg(e)}`)
    }
  }, [results])

  /* ------------------- 文件操作收尾：删除/移动后同步结果 ------------------- */

  /** 成功删除/移动的路径 → 本地过滤 results、清理 selected、重发搜索 */
  const onRefreshAfterOp = useCallback(
    (removed: string[]) => {
      if (removed.length === 0) return
      const removedSet = new Set(removed)
      setResults((prev) => prev.filter((r) => !removedSet.has(r.path)))
      setSelected((prev) => {
        if (prev.size === 0) return prev
        const next = new Set<string>()
        for (const p of prev) if (!removedSet.has(p)) next.add(p)
        return next
      })
      setActiveIdx((i) => {
        if (i < 0 || i >= results.length) return i
        let shift = 0
        for (let k = 0; k < i; k++) if (removedSet.has(results[k].path)) shift++
        return removedSet.has(results[i].path) ? -1 : i - shift
      })
      setSubmitTick((t) => t + 1)
    },
    [results],
  )

  /* ----------------------- 事件：书签（R3） ----------------------- */

  const onAddBookmark = useCallback((path: string) => {
    const name = path.split('/').pop() ?? path
    void api
      .addBookmark(path, name)
      .catch((e) => setError(`添加书签失败：${errMsg(e)}`))
  }, [])

  /* ----------------------- 事件：菜单 / 工具栏 ----------------------- */

  const applyTheme = useCallback((t: ThemeMode) => {
    setTheme(t)
    void api.setTheme(t).catch(() => {})
  }, [])

  const onMenuAction = useCallback(
    (a: MenuAction) => {
      switch (a) {
        case 'file.export':
          void onExport()
          break
        case 'file.exit':
          void import('@tauri-apps/api/window')
            .then((m) => m.appWindow.close())
            .catch(() => {})
          break
        case 'edit.copyPath':
          if (selectedPaths.length) void api.copyPaths(selectedPaths).catch(() => {})
          break
        case 'edit.copyName':
          if (selectedPaths.length) void api.copyFileNames(selectedPaths).catch(() => {})
          break
        case 'edit.selectAll':
          setSelected(new Set(results.map((r) => r.path)))
          break
        case 'edit.findInDoc':
          setFindTick((t) => t + 1)
          break
        case 'view.themeLight':
          applyTheme('light')
          break
        case 'view.themeDark':
          applyTheme('dark')
          break
        case 'view.themeSystem':
          applyTheme('system')
          break
        case 'view.lineNumbers':
          patchView({ lineNumbers: !view.lineNumbers })
          break
        case 'view.wrap':
          patchView({ wrap: !view.wrap })
          break
        case 'view.zoomIn':
          patchView({ fontSize: clamp(view.fontSize + 1, FONT_SIZE_MIN, FONT_SIZE_MAX) })
          break
        case 'view.zoomOut':
          patchView({ fontSize: clamp(view.fontSize - 1, FONT_SIZE_MIN, FONT_SIZE_MAX) })
          break
        case 'view.zoomReset':
          patchView({ fontSize: 13 })
          break
        case 'view.prevPage':
          onArrowNav(-1)
          break
        case 'view.nextPage':
          onArrowNav(1)
          break
        case 'settings.indexAdmin':
          setShowIndexAdmin(true)
          break
        case 'settings.options':
          setShowSettings(true)
          break
        case 'help.homepage':
          void api.openExternal('https://github.com/Eyzer').catch(() => {})
          break
        case 'help.about':
          setShowAbout(true)
          break
      }
    },
    [applyTheme, onArrowNav, onExport, selectedPaths, results, view, patchView],
  )

  const onToolbarAction = useCallback(
    (a: ToolbarAction) => {
      switch (a) {
        case 'nav.clear':
          setQuery('')
          setSelected(new Set())
          setTabs([])
          setActiveTab(null)
          setShowWelcome(true)
          break
        case 'nav.back':
          setNavPos((p) => Math.max(0, p - 1))
          break
        case 'nav.forward':
          setNavPos((p) => Math.min(navStack.length - 1, p + 1))
          break
        case 'nav.refresh':
          setSubmitTick((t) => t + 1)
          break
        case 'nav.up':
          setDir(null)
          break
        case 'edit.trash':
          if (selectedPaths.length) {
            void (async () => {
              try {
                const r = await api.moveToTrash(selectedPaths)
                const failed = new Set(r.failed.map((f) => f.path))
                if (r.failed.length > 0) {
                  const f = r.failed[0]
                  setError(`移入回收站：成功 ${r.ok} 项，失败 ${r.failed.length} 项。\n首个错误：${f.path}\n${f.error}`)
                }
                onRefreshAfterOp(selectedPaths.filter((p) => !failed.has(p)))
                void loadStats()
              } catch (e) {
                setError(`移入回收站失败：${errMsg(e)}`)
              }
            })()
          }
          break
        case 'view.palette':
          applyTheme(theme === 'dark' ? 'light' : 'dark')
          break
        case 'settings.options':
          setShowSettings(true)
          break
        case 'index.togglePause': {
          const next = indexState !== 'paused'
          void (async () => {
            try {
              await api.setIndexPaused(next)
              pausedRef.current = next
              setIndexState(next ? 'paused' : 'idle')
            } catch (e) {
              setError(`切换索引暂停状态失败：${errMsg(e)}`)
            }
          })()
          break
        }
        case 'help.info':
          setShowAbout(true)
          break
        case 'help.homepage':
          void api.openExternal('https://github.com/Eyzer').catch(() => {})
          break
        default:
          break
      }
    },
    [applyTheme, theme, selectedPaths, navStack.length, loadStats, indexState, onRefreshAfterOp],
  )

  /* 导航历史位置变化 → 应用到搜索条件（ref 守卫防记录 effect 回写形成死循环） */
  useEffect(() => {
    if (navPos < 0) return
    const e = navStack[navPos]
    if (!e) return
    if (e.dir === dir && e.fileType === fileType) return
    applyingNavRef.current = true
    setDir(e.dir)
    setFileType(e.fileType)
    // navPos 变化时 navStack 必已同批更新，闭包取值即最新
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [navPos])

  /* 记录导航上下文（供前进/后退）；新入栈时位置移到栈顶 */
  useEffect(() => {
    if (applyingNavRef.current) {
      // 本次 dir/fileType 变化来自前进/后退回写，不记录、不移动位置
      applyingNavRef.current = false
      return
    }
    const top = navStack[navStack.length - 1]
    if (top && top.dir === dir && top.fileType === fileType) {
      if (navPos < 0) setNavPos(navStack.length - 1)
      return
    }
    setNavStack((prev) => [...prev, { dir, fileType }])
    setNavPos(navStack.length)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [dir, fileType])

  /* --------------------- 全局快捷键 ---------------------
   * 统一在 window keydown 分派；除明确允许者外，焦点在 INPUT/TEXTAREA 时不拦截。
   * 显式不实现（TODO）：
   *   - Ctrl+N 新建搜索多窗口 → 后续 B1 批次
   *   - Ctrl+U 检测更新 → 后续 B6 批次
   *   - Alt+Q 全局唤醒 → 已由后端 global-shortcut 实现，前端不处理
   */
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const ctrl = e.ctrlKey || e.metaKey
      const key = e.key.toLowerCase()
      const tag = (e.target as HTMLElement | null)?.tagName
      const inField = tag === 'INPUT' || tag === 'TEXTAREA'

      /* 1. Ctrl+L 聚焦搜索框（输入框内亦允许） */
      if (ctrl && key === 'l') {
        e.preventDefault()
        document.querySelector<HTMLInputElement>('.search-input')?.focus()
        return
      }
      /* 2. Ctrl+S 导出 */
      if (ctrl && !e.shiftKey && key === 's') {
        e.preventDefault()
        void onExport()
        return
      }
      /* 2b. Ctrl+U 检测更新（7.9 / B6） */
      if (ctrl && !e.shiftKey && key === 'u') {
        e.preventDefault()
        void (async () => {
          try {
            const info = await api.checkUpdate()
            if (!info.available) {
              window.alert(t('update.latest', { version: info.currentVersion }))
              return
            }
            const notes = info.notes ? `\n${t('update.availableNotes', { notes: info.notes })}` : ''
            const ok = window.confirm(
              t('update.installConfirm', { version: info.version ?? '' }) + notes,
            )
            if (!ok) return
            await api.installUpdate()
          } catch (err) {
            const msg = errMsg(err)
            window.alert(msg.includes('未配置') ? t('update.notConfigured') : t('update.failed', { error: msg }))
          }
        })()
        return
      }
      /* （Ctrl+Shift+C 复制路径，见第 4 组；置于 Ctrl+C 之前） */
      if (ctrl && e.shiftKey && key === 'c') {
        e.preventDefault()
        if (selectedPaths.length) void api.copyPaths(selectedPaths).catch(() => {})
        return
      }
      /* 3. Ctrl+A 全选结果（排除输入框） */
      if (ctrl && !e.shiftKey && key === 'a') {
        if (!inField) {
          e.preventDefault()
          setSelected(new Set(results.map((r) => r.path)))
        }
        return
      }
      /* 4. Ctrl+C 复制路径（Linux 文件剪贴板降级，仅复制路径文本） */
      if (ctrl && !e.shiftKey && key === 'c') {
        if (inField) return // 输入框内保留系统文本复制
        e.preventDefault()
        if (selectedPaths.length) void api.copyPaths(selectedPaths).catch(() => {})
        setError('已复制路径到剪贴板（系统剪贴板文件复制在 Linux 下降级，见 R9）')
        return
      }
      /* 5. Ctrl+X 剪切（Linux 降级提示） */
      if (ctrl && key === 'x') {
        if (inField) return
        e.preventDefault()
        setError('剪切在 Linux 下降级，请使用「移动到…」(Ctrl+M)')
        return
      }
      /* 6. Ctrl+F 文档内查找 */
      if (ctrl && key === 'f') {
        e.preventDefault()
        setFindTick((t) => t + 1)
        return
      }
      /* 7. Ctrl+E 定位当前活动文件 */
      if (ctrl && key === 'e') {
        if (inField) return
        e.preventDefault()
        const p = results[activeIdx]?.path ?? activeTab
        if (p) void api.revealInFolder(p).catch(() => {})
        return
      }
      /* 8. Ctrl+T 复制到目录（dest 空串：后端弹目录框；复制不改原结果，仅报失败） */
      if (ctrl && key === 't') {
        if (inField) return
        e.preventDefault()
        if (selectedPaths.length) {
          void (async () => {
            try {
              const r = await api.transferFiles(selectedPaths, '', false)
              if (r.failed.length > 0) {
                const f = r.failed[0]
                setError(`复制到目录：成功 ${r.ok} 项，失败 ${r.failed.length} 项。\n首个错误：${f.path}\n${f.error}`)
              }
            } catch (err) {
              setError(`复制到目录失败：${errMsg(err)}`)
            }
          })()
        }
        return
      }
      /* 9. Ctrl+M 移动到目录（成功路径收尾，与 SelectionBar 语义一致） */
      if (ctrl && key === 'm') {
        if (inField) return
        e.preventDefault()
        if (selectedPaths.length) {
          void (async () => {
            try {
              const r = await api.transferFiles(selectedPaths, '', true)
              const failed = new Set(r.failed.map((f) => f.path))
              if (r.failed.length > 0) {
                const f = r.failed[0]
                setError(`移动到目录：成功 ${r.ok} 项，失败 ${r.failed.length} 项。\n首个错误：${f.path}\n${f.error}`)
              }
              onRefreshAfterOp(selectedPaths.filter((p) => !failed.has(p)))
              void loadStats()
            } catch (err) {
              setError(`移动到目录失败：${errMsg(err)}`)
            }
          })()
        }
        return
      }
      /* 10. Ctrl+↑ / Ctrl+↓ 缩放；Ctrl+0 重置字号 */
      if (ctrl && e.key === 'ArrowUp') {
        e.preventDefault()
        patchView({ fontSize: clamp(view.fontSize + 1, FONT_SIZE_MIN, FONT_SIZE_MAX) })
        return
      }
      if (ctrl && e.key === 'ArrowDown') {
        e.preventDefault()
        patchView({ fontSize: clamp(view.fontSize - 1, FONT_SIZE_MIN, FONT_SIZE_MAX) })
        return
      }
      if (ctrl && e.key === '0') {
        e.preventDefault()
        patchView({ fontSize: 13 })
        return
      }
      /* 11. Delete 移入回收站 */
      if (e.key === 'Delete') {
        if (inField) return
        if (selectedPaths.length) {
          e.preventDefault()
          void (async () => {
            try {
              const r = await api.moveToTrash(selectedPaths)
              const failed = new Set(r.failed.map((f) => f.path))
              if (r.failed.length > 0) {
                const f = r.failed[0]
                setError(`移入回收站：成功 ${r.ok} 项，失败 ${r.failed.length} 项。\n首个错误：${f.path}\n${f.error}`)
              }
              onRefreshAfterOp(selectedPaths.filter((p) => !failed.has(p)))
              void loadStats()
            } catch (err) {
              setError(`移入回收站失败：${errMsg(err)}`)
            }
          })()
        }
        return
      }
      /* 12. F3 / Shift+F3 结果导航 */
      if (e.key === 'F3') {
        e.preventDefault()
        onArrowNav(e.shiftKey ? -1 : 1)
        return
      }
      /* 13. Alt+U 手动更新索引（当前 dir 或首个索引根；fileTypeId 传 null） */
      if (e.altKey && key === 'u') {
        e.preventDefault()
        void (async () => {
          try {
            let target = dir
            if (!target) {
              const roots = await api.listIndexedRoots()
              target = roots[0] ?? null
            }
            if (!target) return
            await api.indexDirectory(target, null)
            void loadStats()
          } catch (err) {
            setError(`更新索引失败：${errMsg(err)}`)
          }
        })()
        return
      }
      /* Alt+S 搜索本文件（等同 Ctrl+F 文档内查找） */
      if (e.altKey && key === 's') {
        e.preventDefault()
        setFindTick((t) => t + 1)
        return
      }
      /* Alt+G 搜索全部 → 聚焦搜索框 */
      if (e.altKey && key === 'g') {
        e.preventDefault()
        document.querySelector<HTMLInputElement>('.search-input')?.focus()
        return
      }
      /* F1 帮助：打开项目主页。TODO：若 MenuBar 暴露 syntaxTick prop，可改为展开语法速查面板 */
      if (e.key === 'F1' && !e.shiftKey) {
        e.preventDefault()
        void api.openExternal('https://github.com/Eyzer').catch(() => {})
        return
      }
      /* Escape 清除选择 */
      if (e.key === 'Escape') {
        setSelected(new Set())
        return
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [
    onArrowNav,
    onExport,
    results,
    selectedPaths,
    view,
    patchView,
    activeIdx,
    activeTab,
    onRefreshAfterOp,
    loadStats,
    dir,
  ])

  /* ------------------------------- 渲染 ------------------------------- */

  return (
    <ThemeProvider theme={theme}>
      <div className="app">
        <MenuBar onAction={onMenuAction} view={view} />

        <div className="toolbar-row">
          <AppToolbar
            indexState={indexState}
            canBack={navPos > 0}
            canForward={navPos >= 0 && navPos < navStack.length - 1}
            onAction={onToolbarAction}
          />
        </div>

        <div className="search-area">
          <SearchPanel
            query={query}
            mode={mode}
            reverse={reverse}
            loading={loading}
            indexState={indexState}
            onQueryChange={setQuery}
            onModeChange={setMode}
            onReverseChange={setReverse}
            onSubmit={onSubmitSearch}
            onArrowNav={onArrowNav}
          />
          <FilterRow
            dir={dir}
            fileType={fileType}
            timeRange={timeRange}
            onDirChange={setDir}
            onFileTypeChange={setFileType}
            onTimeRangeChange={setTimeRange}
            onOpenBookmarks={() => setShowBookmarks(true)}
          />
        </div>

        {error && (
          <div className="error-bar" role="alert">
            <span className="error-text">{error}</span>
            <button className="error-close" onClick={() => setError(null)} title="关闭">
              ✕
            </button>
          </div>
        )}

        <main className={`app-main${showWelcome ? ' single' : ''}`}>
          <section className="result-area">
            <SelectionBar
              paths={selectedPaths}
              allPaths={results.map((r) => r.path)}
              onSelectAll={() => setSelected(new Set(results.map((r) => r.path)))}
              onClear={() => setSelected(new Set())}
              onRefresh={onRefreshAfterOp}
            />
            <ResultList
              results={results}
              activeIdx={activeIdx}
              selected={selected}
              sort={sort}
              query={query}
              mode={mode}
              page={page}
              pageSize={PAGE_SIZE}
              total={total}
              partial={partial}
              onSortChange={setSort}
              onPageChange={setPage}
              onAddBookmark={onAddBookmark}
              onActivate={setActiveIdx}
              onSelectionChange={setSelected}
              onOpenFile={(p) => void api.openFile(p).catch(() => {})}
              onRefresh={onRefreshAfterOp}
            />
          </section>

          {showWelcome ? (
            <WelcomePage
              onOpenHomepage={() => void api.openExternal('https://github.com/Eyzer').catch(() => {})}
            />
          ) : (
            <PreviewPane
              tabs={tabs}
              activeTab={activeTab}
              query={query}
              mode={mode}
              findTick={findTick}
              view={view}
              onViewChange={patchView}
              onTabActivate={setActiveTab}
              onTabClose={closeTab}
            />
          )}
        </main>

        <StatusBar
          text={statusText}
          path={statusPath}
          right={stats ? (stats.available ? `${stats.docs} 篇 · ${stats.size}` : '索引不可用，请重建') : '索引加载中…'}
        />

        <IndexAdminDialog
          open={showIndexAdmin}
          onClose={() => {
            setShowIndexAdmin(false)
            void loadStats()
          }}
          onPausedChange={(paused) => {
            pausedRef.current = paused
            setIndexState(paused ? 'paused' : 'idle')
          }}
        />
        <SettingsDialog
          open={showSettings}
          onClose={() => {
            setShowSettings(false)
            void api.getAppConfig().then((c) => setTheme(c.theme)).catch(() => {})
          }}
        />
        <BookmarkPanel open={showBookmarks} onClose={() => setShowBookmarks(false)} />
        <AboutDialog open={showAbout} onClose={() => setShowAbout(false)} />
        <ExternalSearchListener
          onSearch={(q, drive, ext) => {
            setQuery(q)
            if (ext) setFileType(ext)
            if (drive) setDir(drive)
            setSubmitTick((t2) => t2 + 1)
          }}
        />
      </div>
    </ThemeProvider>
  )
}
