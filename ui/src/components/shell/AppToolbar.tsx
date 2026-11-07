/**
 * 应用工具栏图标组（子项 5 所有权）
 * 按规格
 *
 * 约定：
 *   - 只用文本字形，不引入图标库；
 *   - 每个按钮 title 为中文名；
 *   - canBack / canForward 为 false 时禁用 nav.back / nav.forward；
 *   - 模式下拉不属于本组件（由 SearchPanel 渲染在同一 flex 行）；
 *   - 高度由 --h-toolbar 控制（见 shell.css）。
 */
import type { IndexRunState, ToolbarAction } from '../../types'
import './shell.css'

export interface AppToolbarProps {
  indexState: IndexRunState
  canBack: boolean
  canForward: boolean
  onAction: (a: ToolbarAction) => void
}

interface ToolBtn {
  glyph: string
  title: string
  action: ToolbarAction
  /** 依赖 canBack/canForward 的按钮，运行时再决定禁用 */
  nav?: 'back' | 'forward'
  /** 分组分隔线（渲染在本按钮之前） */
  sepBefore?: boolean
}

/** 图标组定义（顺序严格照 image3 / 任务卡）
 *
 * 字形只用 BMP 内的几何/箭头符号：麒麟的 webkit2gtk 里没装彩色 emoji 字体，
 * 原来的 🎨 / 🗑 会渲染成空白或豆腐块（表现为「按钮不显示」）。
 * ◐ = 明暗切换，⌫ = 删除，字体覆盖都稳。
 */
const BUTTONS: ToolBtn[] = [
  { glyph: '←', title: '后退', action: 'nav.back', nav: 'back' },
  { glyph: '→', title: '前进', action: 'nav.forward', nav: 'forward' },
  { glyph: '▼', title: '暂停 / 继续索引', action: 'index.togglePause' },
  { glyph: '↻', title: '刷新', action: 'nav.refresh' },
  { glyph: '✕', title: '清除结果', action: 'nav.clear' },
  { glyph: '⌫', title: '删除到回收站', action: 'edit.trash', sepBefore: true },
  { glyph: '⚙', title: '选项设置', action: 'settings.options', sepBefore: true },
  { glyph: '↑', title: '上级目录', action: 'nav.up' },
  { glyph: '◐', title: '切换皮肤（明 / 暗）', action: 'view.palette', sepBefore: true },
  { glyph: 'ℹ', title: '关于 EyzSearch', action: 'help.info' },
  { glyph: '?', title: '帮助 / 主页', action: 'help.homepage' },
]

export default function AppToolbar({ indexState, canBack, canForward, onAction }: AppToolbarProps) {
  // 索引态 → base.css 的 .dot 三态
  const dotCls = indexState === 'indexing' ? 'busy' : indexState === 'paused' ? 'warn' : 'on'
  const dotTitle =
    indexState === 'indexing' ? '正在建立索引' : indexState === 'paused' ? '索引已暂停' : '索引就绪'

  return (
    <div className="app-toolbar" role="toolbar" aria-label="应用工具栏">
      {BUTTONS.map((b) => {
        const disabled = b.nav === 'back' ? !canBack : b.nav === 'forward' ? !canForward : false
        return (
          <span key={b.action + b.glyph} className="tb-cell">
            {b.sepBefore && <span className="tb-sep" aria-hidden="true" />}
            <button
              type="button"
              className="tb-btn"
              title={b.title}
              aria-label={b.title}
              disabled={disabled}
              onClick={() => onAction(b.action)}
            >
              {b.glyph}
            </button>
          </span>
        )
      })}

      <span className="tb-spacer" />
      <span className={`dot ${dotCls}`} title={dotTitle} aria-label={dotTitle} />
    </div>
  )
}
