/**
 * 设置对话框（子项 5 所有权）
 * 分区：
 *   外观：皮肤三态（浅色 / 深色 / 跟随系统）——即时预览（写 documentElement.dataset.theme）；
 *   启动：开机启动开关（setAutostart）；「关闭主窗口时最小化到托盘」开关（AppConfig.closeToTray）；
 *   快捷键：全量热键表（AppConfig.hotkeys，默认 10 条由后端 default_hotkeys() 提供），
 *           每行 label + accel 可编辑（输入框捕获按键组合，如 Ctrl+Shift+T）。
 *   更新：检测更新（7.9 / B6；通道占位未配置时给出可读提示，不暴露裸网络错误）。
 *
 * 保存：setAutostart + applyGlobalHotkeys + setAppConfig，随后由 App.tsx 在 onClose 后重新拉 config 生效。
 * 注意：App.tsx 已持有 theme 状态并监听 onClose 重拉 config，故本组件只在「保存」时落盘；
 *       皮肤切换即时预览仅写 dataset，不提前落盘，从而「取消」可完整回退。
 */
import { useCallback, useEffect, useState } from 'react'
import type { AppConfig, ThemeMode } from '../../types'
import * as api from '../../api/shell'
import * as updaterApi from '../../api/updater'
import { errMsg } from '../../util'
import './shell.css'

export interface SettingsDialogProps {
  open: boolean
  onClose: () => void
}

const THEMES: Array<{ value: ThemeMode; label: string }> = [
  { value: 'light', label: '浅色' },
  { value: 'dark', label: '深色' },
  { value: 'system', label: '跟随系统' },
]

  /** 即时预览：仅改 dataset（与 App.tsx / ThemeProvider 同一属性名），不落盘 */
  function previewTheme(t: ThemeMode) {
    const mq = window.matchMedia('(prefers-color-scheme: dark)')
    const resolved = t === 'system' ? (mq.matches ? 'dark' : 'light') : t
    document.documentElement.dataset.theme = resolved
  }

export default function SettingsDialog({ open, onClose }: SettingsDialogProps) {
  const [cfg, setCfg] = useState<AppConfig | null>(null)
  const [saving, setSaving] = useState(false)
  const [feedback, setFeedback] = useState<{ kind: 'ok' | 'err'; msg: string } | null>(null)
  const [updateBusy, setUpdateBusy] = useState(false)
  const [updateMsg, setUpdateMsg] = useState('')
  /** OCR helper 是否就位（false 时给出「开关无效」提示） */
  const [ocrReady, setOcrReady] = useState(true)

  /** 检测更新（7.9 / B6）：有更新 → confirm → 下载安装（自动重启） */
  async function onCheckUpdate() {
    setUpdateBusy(true)
    setUpdateMsg('')
    try {
      const info = await updaterApi.checkUpdate()
      if (!info.available) {
        setUpdateMsg(`当前已是最新版本（${info.currentVersion}）`)
        return
      }
      const notes = info.notes ? `\n\n${info.notes}` : ''
      const ok = window.confirm(
        `发现新版本 ${info.version ?? ''}，是否现在更新？\n应用将下载并自动重启。${notes}`,
      )
      if (!ok) {
        setUpdateMsg(`发现新版本 ${info.version ?? ''}（未更新）`)
        return
      }
      setUpdateMsg('正在下载并安装更新，应用即将重启…')
      await updaterApi.installUpdate()
    } catch (e) {
      const msg = errMsg(e)
      setUpdateMsg(
        msg.includes('未配置') ? '更新通道未配置（首个正式版发布后启用）' : `更新检查失败：${msg}`,
      )
    } finally {
      setUpdateBusy(false)
    }
  }

  useEffect(() => {
    if (!open) return
    setFeedback(null)
    void api
      .getAppConfig()
      .then(setCfg)
      .catch((e: unknown) => setFeedback({ kind: 'err', msg: `读取配置失败：${errMsg(e)}` }))
    // OCR helper 是否就位：开关打开但 helper 缺失时，扫描件依然识别不出内容
    void api
      .ocrStatus()
      .then((s) => setOcrReady(s.helperReady))
      .catch(() => setOcrReady(false))
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

  const patch = useCallback((p: Partial<AppConfig>) => {
    setCfg((c) => (c ? { ...c, ...p } : c))
  }, [])

  const onPickTheme = (t: ThemeMode) => {
    patch({ theme: t })
    previewTheme(t)
  }

  const onEditAccel = (idx: number, accel: string) => {
    setCfg((c) => {
      if (!c) return c
      const hotkeys = c.hotkeys.map((h, i) => (i === idx ? { ...h, accel } : h))
      return { ...c, hotkeys }
    })
  }

  /** 输入框捕获按键组合 → 规范化加速键串 */
  const captureAccel = (e: React.KeyboardEvent<HTMLInputElement>, idx: number) => {
    e.preventDefault()
    if (e.key === 'Backspace' || e.key === 'Delete') {
      onEditAccel(idx, '')
      return
    }
    if (['Control', 'Shift', 'Alt', 'Meta'].includes(e.key)) return // 仅按下修饰键不记录
    const parts: string[] = []
    if (e.ctrlKey || e.metaKey) parts.push('Ctrl')
    if (e.altKey) parts.push('Alt')
    if (e.shiftKey) parts.push('Shift')
    let key = e.key
    if (key.length === 1) key = key.toUpperCase()
    else if (key === ' ') key = 'Space'
    parts.push(key)
    onEditAccel(idx, parts.join('+'))
  }

  const onSave = async () => {
    if (!cfg) return
    setSaving(true)
    setFeedback(null)
    try {
      await api.setAutostart(cfg.autostart)
      await api.applyGlobalHotkeys(cfg.hotkeys.map((h) => h.accel))
      await api.setAppConfig(cfg)
      // 右击搜索：写系统集成（Linux 脚本/扩展、Windows 注册表）；返回结果文案
      let msg = '设置已保存'
      try {
        const r = await api.setContextMenuEnabled(cfg.rightClickSearch)
        if (cfg.rightClickSearch && r) msg = `设置已保存；${r}`
      } catch (e) {
        // 集成失败不回滚其余设置，但如实提示（如 macOS 不支持）
        return setFeedback({ kind: 'err', msg: `设置已保存，但右击搜索：${errMsg(e)}` })
      }
      setFeedback({ kind: 'ok', msg })
    } catch (e) {
      setFeedback({ kind: 'err', msg: `保存失败：${errMsg(e)}` })
    } finally {
      setSaving(false)
    }
  }

  if (!open) return null

  return (
    <div className="modal-overlay" onMouseDown={onClose}>
      <div
        className="settings-dialog"
        role="dialog"
        aria-label="设置"
        onMouseDown={(e) => e.stopPropagation()}
      >
        <header className="dialog-head">
          <span className="dialog-title">设置</span>
          <button type="button" className="dialog-close" title="关闭" onClick={onClose}>
            ✕
          </button>
        </header>

        <div className="settings-body scroll-y">
          {!cfg ? (
            <div className="empty">配置加载中…</div>
          ) : (
            <>
              <section className="settings-section">
                <h3>外观</h3>
                <div className="theme-row">
                  {THEMES.map((t) => (
                    <button
                      key={t.value}
                      type="button"
                      className={`theme-choice${cfg.theme === t.value ? ' active' : ''}`}
                      onClick={() => onPickTheme(t.value)}
                    >
                      {t.label}
                    </button>
                  ))}
                </div>
              </section>

              <section className="settings-section">
                <h3>启动</h3>
                <label className="switch-row">
                  <input
                    type="checkbox"
                    checked={cfg.autostart}
                    onChange={(e) => patch({ autostart: e.target.checked })}
                  />
                  <span>开机时自动启动 EyzSearch</span>
                </label>
                <label className="switch-row">
                  <input
                    type="checkbox"
                    checked={cfg.closeToTray}
                    onChange={(e) => patch({ closeToTray: e.target.checked })}
                  />
                  <span>关闭主窗口时最小化到托盘</span>
                </label>
                <label className="switch-row">
                  <input
                    type="checkbox"
                    checked={cfg.rightClickSearch}
                    onChange={(e) => patch({ rightClickSearch: e.target.checked })}
                  />
                  <span>右击目录搜索（文件管理器右键启动，保存后写入系统集成）</span>
                </label>
              </section>

              <section className="settings-section">
                <h3>文档识别</h3>
                <label className="switch-row">
                  <input
                    type="checkbox"
                    checked={cfg.ocrEnabled}
                    onChange={(e) => patch({ ocrEnabled: e.target.checked })}
                  />
                  <span>启用 OCR 识别（图片与扫描型 PDF）</span>
                </label>
                <p className="settings-hint">
                  默认关闭。开启后，没有文字层的扫描件与图片才会被识别成可检索文字；
                  代价是这类文件的索引耗时明显增加（逐页识别）。改完保存后，需重建索引才对已有文件生效。
                </p>
                {cfg.ocrEnabled && !ocrReady && (
                  <p className="settings-hint">
                    未在当前安装包内找到 OCR 组件，开关打开也不会有识别结果。
                  </p>
                )}
              </section>

              <section className="settings-section">
                <h3>快捷键</h3>
                <p className="settings-hint">
                  点击输入框后按下组合键即可改绑（Backspace 清除）；保存后立即生效。
                </p>
                <table className="hotkey-table">
                  <thead>
                    <tr>
                      <th>功能</th>
                      <th>加速键</th>
                    </tr>
                  </thead>
                  <tbody>
                    {cfg.hotkeys.map((h, idx) => (
                      <tr key={h.action}>
                        <td className="hotkey-label">{h.label}</td>
                        <td>
                          <input
                            className="hotkey-input"
                            value={h.accel}
                            readOnly
                            placeholder="按下按键组合"
                            onKeyDown={(e) => captureAccel(e, idx)}
                          />
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </section>

              <section className="settings-section">
                <h3>更新</h3>
                <div className="settings-update-row">
                  <span>检测新版本（快捷键 Ctrl+U）</span>
                  <button
                    type="button"
                    className="btn"
                    disabled={updateBusy}
                    onClick={() => void onCheckUpdate()}
                  >
                    {updateBusy ? '检查中…' : '检测更新'}
                  </button>
                </div>
                {updateMsg && <p className="settings-hint">{updateMsg}</p>}
              </section>
            </>
          )}
        </div>

        <footer className="dialog-foot">
          <span className={`settings-feedback${feedback ? ' ' + feedback.kind : ''}`} role="status">
            {feedback?.msg ?? ''}
          </span>
          <button type="button" className="btn" onClick={onClose} disabled={saving}>
            取消
          </button>
          <button type="button" className="btn primary" onClick={() => void onSave()} disabled={!cfg || saving}>
            保存
          </button>
        </footer>
      </div>
    </div>
  )
}
