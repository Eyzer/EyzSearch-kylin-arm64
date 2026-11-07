/**
 * 索引管理对话框（子项 3 所有权）
 *
 *类型增删 + per-format 三态目录规则 + 存储目录迁移 +
 *          定时更新三态 + 重建索引（进度/取消/暂停）。
 *
 * 数据全部经 api/indexAdmin.ts；重建进度订阅后端 `index-progress` 事件
 * （负载 {done,total,current}，`current === ''` 表示本轮结束 → 收尾刷新）。
 */
import { useEffect, useRef, useState, type ChangeEvent } from 'react'
import { listen } from '@tauri-apps/api/event'
import type {
  DirRule,
  FileType,
  RebuildProgress,
  RuleState,
  ScheduleConfig,
  ScheduleMode,
  StorageInfo,
} from '../../types'
import * as api from '../../api/indexAdmin'
import * as filesApi from '../../api/files'
import { errMsg } from '../../util'
import { useI18n } from '../../i18n'
import { entryForExt, filterCatalog, SUPPORTED_EXTS } from '../../fileTypeCatalog'
import './index.css'

export interface IndexAdminDialogProps {
  open: boolean
  onClose: () => void
  /** 暂停/恢复动作成功后同步回 App（工具栏指示灯与进度监听共用） */
  onPausedChange?: (paused: boolean) => void
}

type Section = 'types' | 'rules' | 'storage' | 'schedule' | 'build'
type Msg = { kind: 'ok' | 'err'; text: string } | null
type Unlisten = () => void

const SECTIONS: Array<{ id: Section; label: string }> = [
  { id: 'types', label: '文件类型' },
  { id: 'rules', label: '目录规则' },
  { id: 'storage', label: '存储' },
  { id: 'schedule', label: '定时更新' },
  { id: 'build', label: '索引构建' },
]

const RULE_STATES: Array<{ value: RuleState; label: string }> = [
  { value: 'include', label: '强制包含' },
  { value: 'exclude', label: '强制排除' },
  { value: 'inherit', label: '继承' },
]

const SCHEDULE_MODES: Array<{ value: ScheduleMode; label: string }> = [
  { value: 'never', label: '从不' },
  { value: 'interval', label: '每 N 分钟' },
  { value: 'daily', label: '每天 HH:mm' },
]

/** 逗号分隔扩展名 → 规范化数组 */
function parseExts(raw: string): string[] {
  return raw
    .split(/[,，\s]+/)
    .map((s) => s.trim().replace(/^\./, '').toLowerCase())
    .filter(Boolean)
}

/** 文件名 → 小写扩展名（无点；无扩展名返回空串） */
function extOfFileName(name: string): string {
  const i = name.lastIndexOf('.')
  return i > 0 ? name.slice(i + 1).toLowerCase() : ''
}

export default function IndexAdminDialog({ open, onClose, onPausedChange }: IndexAdminDialogProps) {
  const { t } = useI18n()
  const [section, setSection] = useState<Section>('types')
  const [msg, setMsg] = useState<Msg>(null)
  const [confirm, setConfirm] = useState<{ text: string; onOk: () => void } | null>(null)

  /* 区块 1：文件类型 */
  const [types, setTypes] = useState<FileType[]>([])
  const [newTypeName, setNewTypeName] = useState('')
  const [newTypeExts, setNewTypeExts] = useState('')
  const [editing, setEditing] = useState<{ id: number; name: string; exts: string } | null>(null)

  /* B4：「查找添加」内联子面板 + 「浏览添加」隐藏 file input */
  const [findOpen, setFindOpen] = useState(false)
  const [findQuery, setFindQuery] = useState('')
  const [stagedExts, setStagedExts] = useState<string[]>([])
  const browseInputRef = useRef<HTMLInputElement>(null)

  /* 区块 2：目录规则 */
  const [ruleTypeId, setRuleTypeId] = useState<number | null>(null)
  const [rules, setRules] = useState<DirRule[]>([])
  const [newRulePattern, setNewRulePattern] = useState('')
  const [newRuleState, setNewRuleState] = useState<RuleState>('exclude')

  /* 区块 3：存储 */
  const [storage, setStorage] = useState<StorageInfo | null>(null)
  const [migratePath, setMigratePath] = useState('')

  /* 区块 4：定时更新 */
  const [schedule, setSchedule] = useState<ScheduleConfig | null>(null)

  /* 区块 5：索引构建 */
  const [roots, setRoots] = useState<string[]>([])
  const [newRoot, setNewRoot] = useState('')
  const [indexTypes, setIndexTypes] = useState<number[]>([])
  const [progress, setProgress] = useState<RebuildProgress | null>(null)
  const [busy, setBusy] = useState(false)
  const [paused, setPaused] = useState(false)

  const ok = (text: string) => setMsg({ kind: 'ok', text })
  const fail = (e: unknown, prefix = '') => setMsg({ kind: 'err', text: `${prefix}${errMsg(e)}` })

  /* ------------------------------- 加载 ------------------------------- */

  const loadTypes = () => api.listFileTypes().then(setTypes)

  const loadRules = (typeId: number | null) => api.listDirRules(typeId).then(setRules)

  const loadStorage = () => api.getStorageInfo().then(setStorage)

  const loadSchedule = () => api.getScheduleConfig().then(setSchedule)

  const loadRoots = () => api.listIndexedRoots().then(setRoots)

  const loadIndexTypes = () => api.getIndexTypes().then(setIndexTypes)

  /* 打开时初始化；同时订阅索引进度事件 */
  useEffect(() => {
    if (!open) return
    setMsg(null)
    setConfirm(null)
    setSection('types')
    setEditing(null)
    setProgress(null)
    setBusy(false)
    setPaused(false)
    Promise.all([loadTypes(), loadStorage(), loadSchedule(), loadRoots(), loadIndexTypes()]).catch(
      (e: unknown) => fail(e, '读取配置失败：'),
    )

    let un: Unlisten | null = null
    let disposed = false
    void (async () => {
      const u = await listen<RebuildProgress>('index-progress', (ev) => {
        const p = ev.payload
        if (!p || p.current === '') {
          // 本轮结束（完成或取消）：收尾并刷新统计
          setBusy(false)
          setProgress(null)
          void loadStorage()
          void loadRoots()
          return
        }
        setProgress(p)
      })
      if (disposed) u()
      else un = u
    })()
    return () => {
      disposed = true
      if (un) un()
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open])

  /* Escape 关闭 */
  useEffect(() => {
    if (!open) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [open, onClose])

  /* 切换选中类型时刷新规则列表 */
  useEffect(() => {
    if (!open || section !== 'rules') return
    loadRules(ruleTypeId).catch((e: unknown) => fail(e, '读取目录规则失败：'))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, section, ruleTypeId])

  const askConfirm = (text: string, onOk: () => void) => setConfirm({ text, onOk })

  /* --------------------------- 区块 1：类型 --------------------------- */

  const onAddType = async () => {
    const name = newTypeName.trim()
    const exts = parseExts(newTypeExts)
    if (!name) return fail('请填写类型名称')
    if (exts.length === 0) return fail('请至少填写一个扩展名')
    try {
      await api.addFileType(name, exts)
      setNewTypeName('')
      setNewTypeExts('')
      await loadTypes()
      ok(`已新增类型「${name}」`)
    } catch (e) {
      fail(e, '新增类型失败：')
    }
  }

  const onSaveEdit = async () => {
    if (!editing) return
    const name = editing.name.trim()
    const exts = parseExts(editing.exts)
    if (!name || exts.length === 0) return fail('类型名称与扩展名均不能为空')
    try {
      await api.updateFileType(editing.id, name, exts)
      setEditing(null)
      await loadTypes()
      ok('类型已更新')
    } catch (e) {
      fail(e, '更新类型失败：')
    }
  }

  const onRemoveType = (t: FileType) => {
    askConfirm(`确定删除类型「${t.name}」及其目录规则？`, async () => {
      try {
        await api.removeFileType(t.id)
        await loadTypes()
        if (ruleTypeId === t.id) setRuleTypeId(null)
        ok(`已删除类型「${t.name}」`)
      } catch (e) {
        fail(e, '删除类型失败：')
      }
    })
  }

  /* B4-查找添加：暂存/取消暂存候选；确定后逐个 addFileType */
  const toggleStage = (ext: string) =>
    setStagedExts((prev) => (prev.includes(ext) ? prev.filter((e) => e !== ext) : [...prev, ext]))

  const onConfirmFind = async () => {
    if (stagedExts.length === 0) return
    let done = 0
    try {
      for (const ext of stagedExts) {
        const entry = entryForExt(ext)
        // 目录条目必然存在（候选列表只来自目录）；兜底用大写 ext 作名称
        await api.addFileType(entry?.name ?? ext.toUpperCase(), [ext])
        done++
      }
      setStagedExts([])
      setFindOpen(false)
      await loadTypes()
      ok(t('idx.addedTypes', [done]))
    } catch (e) {
      fail(e, '新增类型失败：')
    }
  }

  /* B4-浏览添加：取所选文件扩展名；不在提取器支持列表 → 提示不支持 */
  const onBrowseFile = async (e: ChangeEvent<HTMLInputElement>) => {
    // 重置 value：允许重复选择同一文件（否则同一路径不再触发 change）
    e.target.value = ''
    const file = e.target.files?.[0]
    if (!file) return
    const ext = extOfFileName(file.name)
    if (!ext) return fail(t('idx.noExt'))
    if (!SUPPORTED_EXTS.has(ext)) return fail(t('idx.unsupported', [ext]))
    try {
      await api.addFileType(entryForExt(ext)?.name ?? ext.toUpperCase(), [ext])
      await loadTypes()
      ok(t('idx.addedTypes', [1]))
    } catch (err) {
      fail(err, '新增类型失败：')
    }
  }

  /* --------------------------- 区块 2：规则 --------------------------- */

  const onAddRule = async () => {
    const pattern = newRulePattern.trim()
    if (!pattern) return fail('请填写 glob 模式，如 **/node_modules/**')
    try {
      await api.setDirRule(ruleTypeId, pattern, newRuleState)
      setNewRulePattern('')
      await loadRules(ruleTypeId)
      ok('目录规则已保存')
    } catch (e) {
      fail(e, '保存规则失败（模式非法？）：')
    }
  }

  const onChangeRuleState = async (r: DirRule, state: RuleState) => {
    try {
      await api.setDirRule(r.fileTypeId, r.pattern, state)
      await loadRules(ruleTypeId)
      ok('规则状态已更新')
    } catch (e) {
      fail(e, '更新规则失败：')
    }
  }

  const onRemoveRule = async (r: DirRule) => {
    try {
      await api.removeDirRule(r.id)
      await loadRules(ruleTypeId)
      ok('规则已删除')
    } catch (e) {
      fail(e, '删除规则失败：')
    }
  }

  /* --------------------------- 区块 3：存储 --------------------------- */

  const onMigrate = () => {
    const target = migratePath.trim()
    if (!target) return fail('请填写迁移目标目录')
    askConfirm(`将索引存储迁移到「${target}」？迁移期间请勿关闭应用。`, async () => {
      try {
        const info = await api.migrateStorage(target)
        setStorage(info)
        setMigratePath('')
        ok(`已迁移到 ${info.current}（${info.docs} 篇 · ${info.size}），迁移后立即生效`)
      } catch (e) {
        fail(e, '迁移失败：')
      }
    })
  }

  /* --------------------------- 区块 4：定时 --------------------------- */

  const onSaveSchedule = async () => {
    if (!schedule) return
    try {
      await api.setScheduleConfig(schedule)
      ok('定时更新配置已保存并生效')
    } catch (e) {
      fail(e, '保存定时配置失败：')
    }
  }

  const patchSchedule = (p: Partial<ScheduleConfig>) =>
    setSchedule((s) => (s ? { ...s, ...p } : s))

  /* --------------------------- 区块 5：构建 --------------------------- */

  const onAddRoot = async () => {
    const dir = newRoot.trim()
    if (!dir) return fail('请填写或选择要索引的目录')
    setBusy(true)
    try {
      const report = await api.indexDirectory(dir, null)
      setNewRoot('')
      await loadRoots()
      ok(`已添加并索引目录：${dir}（扫描 ${report.scanned} 个文件，索引 ${report.indexed} 篇文档）`)
    } catch (e) {
      fail(e, '索引目录失败：')
    } finally {
      setBusy(false)
    }
  }

  /** 系统目录选择框（替代手输路径） */
  const pickDir = async (apply: (p: string) => void) => {
    try {
      const p = await filesApi.pickDirectory('选择目录')
      if (p) apply(p)
    } catch (e) {
      fail(e, '打开目录选择框失败：')
    }
  }

  /** 移除已索引根目录（连同索引里该目录下的文档） */
  const onRemoveRoot = (r: string) => {
    askConfirm(`从索引中移除「${r}」？该目录下已建立的索引文档会一并删除（磁盘文件不动）。`, async () => {
      try {
        const n = await api.removeIndexedRoot(r)
        await loadRoots()
        await loadStorage()
        ok(`已移除 ${r}（清理索引文档 ${n} 篇）`)
      } catch (e) {
        fail(e, '移除失败：')
      }
    })
  }

  /** 勾选/取消某个索引类型；空集合 = 全部类型 */
  const onToggleIndexType = async (id: number) => {
    const next = indexTypes.includes(id) ? indexTypes.filter((x) => x !== id) : [...indexTypes, id]
    try {
      await api.setIndexTypes(next)
      setIndexTypes(next)
      ok('索引类型已保存，重建索引后生效')
    } catch (e) {
      fail(e, '保存索引类型失败：')
    }
  }

  /** 切换「全部类型」 */
  const onAllIndexTypes = async () => {
    try {
      await api.setIndexTypes([])
      setIndexTypes([])
      ok('已设为全部类型，重建索引后生效')
    } catch (e) {
      fail(e, '保存索引类型失败：')
    }
  }

  const onRebuild = () => {
    askConfirm('重建将清空现有索引并对全部根目录全量重扫，确定继续？', async () => {
      setBusy(true)
      setProgress(null)
      try {
        await api.rebuildIndex()
        ok('重建完成')
      } catch (e) {
        // 后端取消时返回 Err("已取消")：给出明确提示而非笼统失败
        if (errMsg(e).includes('取消')) ok('重建已取消（已写入的索引保留，可再次重建续上）')
        else fail(e, '重建失败：')
      } finally {
        setBusy(false)
        void loadStorage()
      }
    })
  }

  const onCancel = async () => {
    try {
      await api.cancelRebuild()
      ok('已请求取消（当前批次完成后停止，已写入索引不损坏）')
    } catch (e) {
      fail(e, '取消失败：')
    }
  }

  const onTogglePause = async () => {
    const next = !paused
    try {
      await api.setIndexPaused(next)
      setPaused(next)
      onPausedChange?.(next)
      ok(next ? '已暂停索引' : '已恢复索引')
    } catch (e) {
      fail(e, '切换暂停状态失败：')
    }
  }

  /* ------------------------------- 渲染 ------------------------------- */

  if (!open) return null

  const pct = progress && progress.total > 0 ? Math.floor((progress.done / progress.total) * 100) : 0

  /* B4：查找候选（打开子面板才算；按 name/ext 过滤） */
  const candidates = findOpen ? filterCatalog(findQuery) : []

  return (
    <div className="idx-overlay" onMouseDown={onClose}>
      <div
        className="idx-dialog"
        role="dialog"
        aria-label="索引管理"
        onMouseDown={(e) => e.stopPropagation()}
      >
        <header className="idx-head">
          <span className="idx-title">索引管理</span>
          <button type="button" className="idx-close" title="关闭" onClick={onClose}>
            ✕
          </button>
        </header>

        <div className="idx-body">
          <nav className="idx-nav">
            {SECTIONS.map((s) => (
              <button
                key={s.id}
                type="button"
                className={`idx-nav-item${section === s.id ? ' active' : ''}`}
                onClick={() => setSection(s.id)}
              >
                {s.label}
              </button>
            ))}
          </nav>

          <div className="idx-content scroll-y">
            {/* ---------------- 区块 1：文件类型 ---------------- */}
            {section === 'types' && (
              <section className="idx-section">
                <h3>文件类型</h3>
                <table className="idx-table">
                  <thead>
                    <tr>
                      <th>名称</th>
                      <th>扩展名</th>
                      <th className="idx-col-act">操作</th>
                    </tr>
                  </thead>
                  <tbody>
                    {types.map((t) => (
                      <tr key={t.id}>
                        {editing?.id === t.id ? (
                          <>
                            <td>
                              <input
                                className="idx-input"
                                value={editing.name}
                                onChange={(e) => setEditing({ ...editing, name: e.target.value })}
                              />
                            </td>
                            <td>
                              <input
                                className="idx-input"
                                value={editing.exts}
                                onChange={(e) => setEditing({ ...editing, exts: e.target.value })}
                              />
                            </td>
                            <td className="idx-col-act">
                              <button type="button" className="btn primary" onClick={() => void onSaveEdit()}>
                                保存
                              </button>
                              <button type="button" className="btn" onClick={() => setEditing(null)}>
                                取消
                              </button>
                            </td>
                          </>
                        ) : (
                          <>
                            <td>
                              {t.name}
                              {t.builtin && <span className="idx-badge">内置</span>}
                            </td>
                            <td className="idx-exts">{t.exts.join(', ')}</td>
                            <td className="idx-col-act">
                              <button
                                type="button"
                                className="btn"
                                onClick={() => setEditing({ id: t.id, name: t.name, exts: t.exts.join(', ') })}
                              >
                                编辑
                              </button>
                              <button
                                type="button"
                                className="btn danger"
                                disabled={t.builtin}
                                title={t.builtin ? '内置类型不可删除' : '删除'}
                                onClick={() => onRemoveType(t)}
                              >
                                删除
                              </button>
                            </td>
                          </>
                        )}
                      </tr>
                    ))}
                  </tbody>
                </table>

                <div className="idx-add-row">
                  <input
                    className="idx-input"
                    placeholder="类型名称，如：电子表格"
                    value={newTypeName}
                    onChange={(e) => setNewTypeName(e.target.value)}
                  />
                  <input
                    className="idx-input grow"
                    placeholder="扩展名，逗号分隔：xls, xlsx, csv"
                    value={newTypeExts}
                    onChange={(e) => setNewTypeExts(e.target.value)}
                  />
                  <button type="button" className="btn primary" onClick={() => void onAddType()}>
                    新增
                  </button>
                </div>

                <div className="idx-add-row">
                  <button type="button" className="btn" onClick={() => setFindOpen((v) => !v)}>
                    {t('idx.addFind')}
                  </button>
                  <button
                    type="button"
                    className="btn"
                    onClick={() => browseInputRef.current?.click()}
                  >
                    {t('idx.addBrowse')}
                  </button>
                  {/* Tauri webview 的 input[type=file] 只给文件名不给路径；本场景只需扩展名 */}
                  <input
                    ref={browseInputRef}
                    type="file"
                    hidden
                    onChange={(e) => void onBrowseFile(e)}
                  />
                </div>

                {findOpen && (
                  <div className="idx-find-panel">
                    <input
                      className="idx-input grow"
                      placeholder={t('idx.findTypePlaceholder')}
                      value={findQuery}
                      onChange={(e) => setFindQuery(e.target.value)}
                    />
                    <div className="idx-find-list">
                      {candidates.length === 0 ? (
                        <div className="idx-find-empty">{t('idx.noMatch')}</div>
                      ) : (
                        candidates.map((c) => (
                          <div
                            key={c.ext}
                            className={`idx-find-item${stagedExts.includes(c.ext) ? ' staged' : ''}`}
                            title={c.desc}
                            onClick={() => toggleStage(c.ext)}
                          >
                            <span className="idx-find-name">{c.name}</span>
                            <span className="idx-find-ext">.{c.ext}</span>
                            <span className="idx-find-desc">{c.desc}</span>
                          </div>
                        ))
                      )}
                    </div>
                    <div className="idx-find-staged">
                      <span className="idx-label">{t('idx.stagedTypes')}</span>
                      {stagedExts.length === 0 ? (
                        <span className="idx-find-hint">{t('idx.stagedEmpty')}</span>
                      ) : (
                        stagedExts.map((ext) => (
                          <span key={ext} className="idx-chip">
                            .{ext}
                            <button type="button" title={t('idx.unstage')} onClick={() => toggleStage(ext)}>
                              ✕
                            </button>
                          </span>
                        ))
                      )}
                    </div>
                    <div className="idx-find-actions">
                      <button
                        type="button"
                        className="btn"
                        onClick={() => {
                          setFindOpen(false)
                          setStagedExts([])
                        }}
                      >
                        {t('idx.cancel')}
                      </button>
                      <button
                        type="button"
                        className="btn primary"
                        disabled={stagedExts.length === 0}
                        onClick={() => void onConfirmFind()}
                      >
                        {t('idx.confirmAddTypes')}
                      </button>
                    </div>
                  </div>
                )}
              </section>
            )}

            {/* ---------------- 区块 2：目录规则 ---------------- */}
            {section === 'rules' && (
              <section className="idx-section">
                <h3>目录规则（glob 模式 + 三态）</h3>
                <div className="idx-add-row">
                  <label className="idx-label">适用类型</label>
                  <select
                    className="idx-select"
                    value={ruleTypeId === null ? 'all' : String(ruleTypeId)}
                    onChange={(e) => setRuleTypeId(e.target.value === 'all' ? null : Number(e.target.value))}
                  >
                    <option value="all">对所有类型</option>
                    {types.map((t) => (
                      <option key={t.id} value={String(t.id)}>
                        {t.name}
                      </option>
                    ))}
                  </select>
                </div>

                <table className="idx-table">
                  <thead>
                    <tr>
                      <th>模式</th>
                      <th className="idx-col-state">状态</th>
                      <th className="idx-col-act">操作</th>
                    </tr>
                  </thead>
                  <tbody>
                    {rules.length === 0 && (
                      <tr>
                        <td colSpan={3} className="idx-empty">
                          该类型暂无目录规则
                        </td>
                      </tr>
                    )}
                    {rules.map((r) => (
                      <tr key={r.id}>
                        <td className="idx-mono">{r.pattern}</td>
                        <td className="idx-col-state">
                          <select
                            className="idx-select"
                            value={r.state}
                            onChange={(e) => void onChangeRuleState(r, e.target.value as RuleState)}
                          >
                            {RULE_STATES.map((s) => (
                              <option key={s.value} value={s.value}>
                                {s.label}
                              </option>
                            ))}
                          </select>
                        </td>
                        <td className="idx-col-act">
                          <button type="button" className="btn danger" onClick={() => void onRemoveRule(r)}>
                            删除
                          </button>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>

                <div className="idx-add-row">
                  <input
                    className="idx-input grow"
                    placeholder="glob 模式，如 **/node_modules/**"
                    value={newRulePattern}
                    onChange={(e) => setNewRulePattern(e.target.value)}
                  />
                  <select
                    className="idx-select"
                    value={newRuleState}
                    onChange={(e) => setNewRuleState(e.target.value as RuleState)}
                  >
                    {RULE_STATES.map((s) => (
                      <option key={s.value} value={s.value}>
                        {s.label}
                      </option>
                    ))}
                  </select>
                  <button type="button" className="btn primary" onClick={() => void onAddRule()}>
                    新增
                  </button>
                </div>
              </section>
            )}

            {/* ---------------- 区块 3：存储 ---------------- */}
            {section === 'storage' && (
              <section className="idx-section">
                <h3>存储</h3>
                {storage && !storage.available && (
                  <div className="idx-msg err" role="status">
                    索引不可用，请重建
                  </div>
                )}
                {storage ? (
                  <dl className="idx-kv">
                    <dt>当前目录</dt>
                    <dd className="idx-mono">{storage.current}</dd>
                    <dt>文档数</dt>
                    <dd>{storage.docs}</dd>
                    <dt>占用体积</dt>
                    <dd>{storage.size}</dd>
                  </dl>
                ) : (
                  <div className="empty">读取中…</div>
                )}
                <div className="idx-add-row">
                  <input
                    className="idx-input grow"
                    placeholder="迁移目标目录（绝对路径，或点「选择…」）"
                    value={migratePath}
                    onChange={(e) => setMigratePath(e.target.value)}
                  />
                  <button type="button" className="btn" onClick={() => void pickDir(setMigratePath)}>
                    选择…
                  </button>
                  <button type="button" className="btn primary" onClick={onMigrate}>
                    迁移到…
                  </button>
                </div>
                <p className="idx-hint">
                  迁移会复制索引段 / 配置 / 状态库到新目录并校验文档数；目标需为空目录。迁移后立即生效，失败自动回退。
                </p>
              </section>
            )}

            {/* ---------------- 区块 4：定时更新 ---------------- */}
            {section === 'schedule' && (
              <section className="idx-section">
                <h3>定时更新</h3>
                {schedule ? (
                  <>
                    <div className="idx-radio-col">
                      {SCHEDULE_MODES.map((m) => (
                        <label key={m.value} className="idx-radio">
                          <input
                            type="radio"
                            name="sched-mode"
                            checked={schedule.mode === m.value}
                            onChange={() => patchSchedule({ mode: m.value })}
                          />
                          <span>{m.label}</span>
                        </label>
                      ))}
                    </div>
                    <div className="idx-add-row">
                      <label className="idx-label">间隔（分钟）</label>
                      <input
                        className="idx-input"
                        type="number"
                        min={1}
                        disabled={schedule.mode !== 'interval'}
                        value={schedule.intervalMinutes}
                        onChange={(e) =>
                          patchSchedule({ intervalMinutes: Math.max(1, Number(e.target.value) || 1) })
                        }
                      />
                    </div>
                    <div className="idx-add-row">
                      <label className="idx-label">每日时刻</label>
                      <input
                        className="idx-input"
                        type="time"
                        disabled={schedule.mode !== 'daily'}
                        value={schedule.dailyAt}
                        onChange={(e) => patchSchedule({ dailyAt: e.target.value })}
                      />
                    </div>
                    <div className="idx-add-row">
                      <button type="button" className="btn primary" onClick={() => void onSaveSchedule()}>
                        保存
                      </button>
                    </div>
                  </>
                ) : (
                  <div className="empty">读取中…</div>
                )}
              </section>
            )}

            {/* ---------------- 区块 5：索引构建 ---------------- */}
            {section === 'build' && (
              <section className="idx-section">
                <h3>索引构建</h3>

                <h4 className="idx-sub">索引的文档类型</h4>
                <div className="idx-type-pick">
                  <label className="idx-type-item all">
                    <input
                      type="checkbox"
                      checked={indexTypes.length === 0}
                      onChange={() => void onAllIndexTypes()}
                    />
                    <span>全部类型</span>
                  </label>
                  {types.map((t) => (
                    <label key={t.id} className="idx-type-item">
                      <input
                        type="checkbox"
                        checked={indexTypes.length === 0 || indexTypes.includes(t.id)}
                        disabled={indexTypes.length === 0}
                        onChange={() => void onToggleIndexType(t.id)}
                      />
                      <span>{t.name}</span>
                      <span className="idx-type-exts">{t.exts.slice(0, 4).join('/')}</span>
                    </label>
                  ))}
                </div>
                <p className="idx-hint">
                  默认只索引「常用办公文档」（Word / Excel / PPT / PDF / 纯文本 / Markdown / WPS）。
                  勾选更多类型后需重建索引才对已有文件生效；类型越多，首次索引越慢。
                </p>

                <h4 className="idx-sub">已索引根目录</h4>
                {roots.length === 0 ? (
                  <div className="idx-empty">尚未添加任何根目录</div>
                ) : (
                  <ul className="idx-roots">
                    {roots.map((r) => (
                      <li key={r}>
                        <span className="idx-mono idx-root-path" title={r}>
                          {r}
                        </span>
                        <button
                          type="button"
                          className="idx-root-del"
                          title="从索引中移除此目录"
                          disabled={busy}
                          onClick={() => onRemoveRoot(r)}
                        >
                          移除
                        </button>
                      </li>
                    ))}
                  </ul>
                )}

                <div className="idx-add-row">
                  <input
                    className="idx-input grow"
                    placeholder="要索引的目录（可直接输入，或点「选择…」）"
                    value={newRoot}
                    onChange={(e) => setNewRoot(e.target.value)}
                    disabled={busy}
                  />
                  <button
                    type="button"
                    className="btn"
                    disabled={busy}
                    onClick={() => void pickDir(setNewRoot)}
                  >
                    选择…
                  </button>
                  <button
                    type="button"
                    className="btn primary"
                    disabled={busy}
                    onClick={() => void onAddRoot()}
                  >
                    添加目录
                  </button>
                </div>

                <div className="idx-add-row">
                  <button type="button" className="btn danger" disabled={busy} onClick={onRebuild}>
                    重建索引
                  </button>
                  <button type="button" className="btn" disabled={!busy} onClick={() => void onCancel()}>
                    取消
                  </button>
                  <button type="button" className="btn" onClick={() => void onTogglePause()}>
                    {paused ? '恢复' : '暂停'}
                  </button>
                </div>

                {(busy || progress) && (
                  <div className="idx-progress">
                    <div className="idx-progress-bar">
                      <div className="idx-progress-fill" style={{ width: `${pct}%` }} />
                    </div>
                    <div className="idx-progress-text">
                      {progress
                        ? `${progress.done} / ${progress.total}（${pct}%）· ${progress.current}`
                        : '正在准备…'}
                    </div>
                  </div>
                )}
              </section>
            )}
          </div>
        </div>

        <footer className="idx-foot">
          <span className={`idx-msg${msg ? ' ' + msg.kind : ''}`} role="status">
            {msg?.text ?? ''}
          </span>
          <button type="button" className="btn" onClick={onClose}>
            关闭
          </button>
        </footer>

        {confirm && (
          <div className="idx-confirm" role="alertdialog">
            <span className="idx-confirm-text">{confirm.text}</span>
            <button
              type="button"
              className="btn"
              onClick={() => {
                const action = confirm.onOk
                setConfirm(null)
                action()
              }}
            >
              确定
            </button>
            <button type="button" className="btn primary" onClick={() => setConfirm(null)}>
              取消
            </button>
          </div>
        )}
      </div>
    </div>
  )
}
