/**
 * 关于对话框（帮助 → 关于）。
 *
 * 之前 `help.about` 直接 `window.open(GitHub)`，Tauri webview 里会被拦，
 * 表现为「点了没反应」。现在改成内置面板，外链统一走 `open_external` 命令。
 */
import { useEffect, useState } from 'react'
import * as shellApi from '../../api/shell'
import { errMsg } from '../../util'
import { ABOUT as TEXT, HOMEPAGE } from '../../aboutText'

export interface AboutDialogProps {
  open: boolean
  onClose: () => void
}

export default function AboutDialog({ open, onClose }: AboutDialogProps) {
  const [version, setVersion] = useState('')
  const [err, setErr] = useState('')

  useEffect(() => {
    if (!open) return
    setErr('')
    shellApi
      .appInfo()
      .then((i) => setVersion(i.version))
      .catch(() => setVersion(''))
  }, [open])

  useEffect(() => {
    if (!open) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [open, onClose])

  if (!open) return null

  const openHome = async () => {
    try {
      await shellApi.openExternal(HOMEPAGE)
    } catch (e) {
      setErr(errMsg(e))
    }
  }

  return (
    <div className="modal-overlay" onMouseDown={onClose}>
      <div
        className="about-dialog"
        role="dialog"
        aria-label={TEXT.title}
        onMouseDown={(e) => e.stopPropagation()}
      >
        <header className="dialog-head">
          <span className="dialog-title">{TEXT.title}</span>
          <button type="button" className="dialog-close" title={TEXT.close} onClick={onClose}>
            ✕
          </button>
        </header>

        <div className="about-body scroll-y">
          <div className="about-brand">
            <div className="about-logo" aria-hidden="true">
              <svg viewBox="0 0 48 48" width="48" height="48">
                <defs>
                  <linearGradient id="aboutLogoGrad" x1="0" y1="0" x2="1" y2="1">
                    <stop offset="0%" stopColor="#8B5CF6" />
                    <stop offset="100%" stopColor="#6D28D9" />
                  </linearGradient>
                </defs>
                <rect x="0" y="0" width="48" height="48" rx="12" fill="url(#aboutLogoGrad)" />
                <circle cx="21" cy="21" r="8.5" fill="none" stroke="#fff" strokeWidth="3.2" />
                <line
                  x1="27.5"
                  y1="27.5"
                  x2="36"
                  y2="36"
                  stroke="#fff"
                  strokeWidth="3.6"
                  strokeLinecap="round"
                />
              </svg>
            </div>
            <div className="about-brand-text">
              <div className="about-name">{TEXT.name}</div>
              <div className="about-tagline">{TEXT.tagline}</div>
              {version && (
                <div className="about-version">
                  {TEXT.versionPrefix} {version}
                </div>
              )}
            </div>
          </div>

          <p className="about-intro">{TEXT.intro}</p>

          <h4 className="about-sub">{TEXT.featureTitle}</h4>
          <ul className="about-list">
            {TEXT.features.map((f) => (
              <li key={f}>{f}</li>
            ))}
          </ul>

          <dl className="about-kv">
            <dt>{TEXT.envTitle}</dt>
            <dd>{TEXT.env}</dd>
            <dt>{TEXT.techTitle}</dt>
            <dd>{TEXT.tech}</dd>
            <dt>{TEXT.dataTitle}</dt>
            <dd>{TEXT.data}</dd>
            <dt>{TEXT.creditTitle}</dt>
            <dd>{TEXT.credit}</dd>
            <dt>{TEXT.authorLabel}</dt>
            <dd>
              {TEXT.author}　
              <button type="button" className="about-link" onClick={() => void openHome()}>
                {HOMEPAGE}
              </button>
            </dd>
          </dl>

          {err && <div className="about-err">{err}</div>}

          <div className="about-copyright">{TEXT.copyright}</div>
        </div>

        <footer className="dialog-foot">
          <span className="about-foot-spacer" />
          <button type="button" className="btn primary" onClick={onClose}>
            {TEXT.close}
          </button>
        </footer>
      </div>
    </div>
  )
}
