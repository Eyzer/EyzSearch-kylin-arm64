/**
 * 关于文案单一事实源。
 *
 * 「关于」对话框与欢迎页底部共用这一份，避免两处各写一套、改一处忘一处。
 * 用户审核过的定稿，改动前先确认。
 */

export const HOMEPAGE = 'https://github.com/Eyzer'

export const ABOUT = {
  title: '关于 EyzSearch 全文搜索',
  name: 'EyzSearch 全文搜索',
  tagline: '本地文件内容全文搜索工具',
  versionPrefix: '版本',
  intro:
    '时间该花在正事上，不该耗在找文件上。所有索引与检索都在本机完成，不联网、不上传任何内容。',
  featureTitle: '主要能力',
  features: [
    '支持 Word / Excel / PowerPoint、WPS 文字表格演示、PDF、电子书与纯文本',
    '支持带保密标签文件内容检索（.SJZ 加密容器透明解密，无需手动解包）',
    '离线 OCR（默认关闭，可在「设置 → 文档识别」开启）：扫描件与图片转为可检索文字',
    '增量索引：重建时只处理新增与修改过的文件',
    '高级语法：& 与、| 或、! 非、() 分组、"" 精确',
  ],
  envTitle: '运行环境',
  env: '银河麒麟 V10 桌面版（webkit2gtk-4.0 / glibc 2.28），飞腾等 arm64 平台',
  techTitle: '开源组件',
  tech: 'Tantivy 全文检索 · ONNX Runtime · PP-OCRv6 tiny 模型 · PDFium，按各自开源许可分发',
  dataTitle: '数据位置',
  data: '索引与配置存放在用户主目录，沿用旧版目录名以保证升级不丢索引，可在「索引管理 → 存储」迁移',
  creditTitle: '致谢',
  credit: '本项目基于 shadyrispy/everytxt 二次开发',
  authorLabel: '作者',
  author: 'Eyzer',
  homepageLabel: '项目主页',
  copyright: '© 2026 Eyzer',
  close: '关闭',
} as const
