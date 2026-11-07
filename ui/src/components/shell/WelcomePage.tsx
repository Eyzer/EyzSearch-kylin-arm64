/**
 * 欢迎页（子项 5 所有权）
 * 按规格，三节：搜索步骤 / 使用技巧 / 关于。
 * 底部「访问项目主页」按钮 → onOpenHomepage()。
 * 文案走 i18n t()（key 见 ui/src/i18n/zh-CN.ts welcome.*）。
 */
import { useI18n } from '../../i18n'
import { ABOUT } from '../../aboutText'
import './shell.css'

export interface WelcomePageProps {
  onOpenHomepage: () => void
}

export default function WelcomePage({ onOpenHomepage }: WelcomePageProps) {
  const { t } = useI18n()
  const steps = [
    t('welcome.step1'),
    t('welcome.step2'),
    t('welcome.step3'),
    t('welcome.step4'),
  ]
  const tips = [t('welcome.tip1'), t('welcome.tip2'), t('welcome.tip3'), t('welcome.tip4')]

  return (
    <div className="welcome scroll-y">
      <div className="welcome-head">
        <span className="welcome-title">{t('welcome.title')}</span>
      </div>

      <section className="welcome-section">
        <h3>{t('welcome.steps.title')}</h3>
        <ol>
          {steps.map((s) => (
            <li key={s}>{s}</li>
          ))}
        </ol>
      </section>

      <section className="welcome-section">
        <h3>{t('welcome.tips.title')}</h3>
        <ol>
          {tips.map((tip) => (
            <li key={tip}>{tip}</li>
          ))}
        </ol>
        <p className="welcome-dim">{t('welcome.more')}</p>
      </section>

      <section className="welcome-section">
        <h3>{ABOUT.title}</h3>
        <p>{ABOUT.intro}</p>

        <h4 className="about-sub">{ABOUT.featureTitle}</h4>
        <ul className="about-list">
          {ABOUT.features.map((f) => (
            <li key={f}>{f}</li>
          ))}
        </ul>

        <dl className="about-kv">
          <dt>{ABOUT.envTitle}</dt>
          <dd>{ABOUT.env}</dd>
          <dt>{ABOUT.techTitle}</dt>
          <dd>{ABOUT.tech}</dd>
          <dt>{ABOUT.dataTitle}</dt>
          <dd>{ABOUT.data}</dd>
          <dt>{ABOUT.creditTitle}</dt>
          <dd>{ABOUT.credit}</dd>
          <dt>{ABOUT.authorLabel}</dt>
          <dd>{ABOUT.author}</dd>
        </dl>

        <p className="welcome-dim">{ABOUT.copyright}</p>
      </section>

      <div className="welcome-actions">
        <button type="button" className="btn primary" onClick={onOpenHomepage}>
          {t('welcome.openHomepage')}
        </button>
      </div>
    </div>
  )
}
