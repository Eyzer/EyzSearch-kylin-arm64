/**
 * 静态文件类型目录（B4 查找/浏览两途径 + D2 类型描述数据源）。
 *
 * - {@link CATALOG}：ext → 中文类型名 + 简短描述，供「查找添加」候选列表与
 *   搜索页类型下拉 tooltip 使用。ext 一律小写无点。
 * - {@link SUPPORTED_EXTS}：stcore 提取器**真实支持**的扩展名集合，权威来源为
 *   stcore/src/extract/mod.rs 的八张常量表（TEXT/HTML/OOXML/PDF/EPUB/EBOOK/
 *   IMAGE/BINARY/LEGACY）外加 dispatch() 内的字面量分支
 *   （eml/zip/tar/tgz/gz/bz2/7z/rar/chm）——后者同样能提取内容，
 *   不收进来会让「浏览添加」把 .chm 误报为不支持（chm 恰是手册示例）。
 *   已知缺口（不收）：djvu（无文本层）。wps/et/dps 及其模板已纳入 LEGACY 链
 *   （OLE 解析，失败时 strings 兜底）。
 */

/** 目录条目：ext 小写无点 */
export interface CatalogEntry {
  ext: string
  /** 中文类型名，如 "CHM 帮助文档" */
  name: string
  /** 简短中文描述 */
  desc: string
}

export const CATALOG: CatalogEntry[] = [
  /* ------------------------------ 纯文本 / 数据 ------------------------------ */
  { ext: 'txt', name: '纯文本文档', desc: '无格式纯文本，编码自动检测' },
  { ext: 'text', name: '纯文本文档', desc: '无格式纯文本，编码自动检测' },
  { ext: 'log', name: '日志文件', desc: '程序或系统运行日志' },
  { ext: 'csv', name: 'CSV 表格', desc: '逗号分隔值表格数据' },
  { ext: 'tsv', name: 'TSV 表格', desc: '制表符分隔值表格数据' },
  { ext: 'json', name: 'JSON 数据', desc: 'JSON 结构化数据' },
  { ext: 'xml', name: 'XML 文档', desc: '可扩展标记语言文档' },
  { ext: 'yaml', name: 'YAML 配置', desc: 'YAML 格式配置文件' },
  { ext: 'yml', name: 'YAML 配置', desc: 'YAML 格式配置文件' },
  { ext: 'toml', name: 'TOML 配置', desc: 'TOML 格式配置文件' },
  { ext: 'ini', name: 'INI 配置', desc: 'Windows 风格初始化配置' },
  { ext: 'conf', name: '配置文件', desc: '通用配置文件' },
  { ext: 'cfg', name: '配置文件', desc: '通用配置文件' },
  { ext: 'properties', name: '属性配置', desc: 'Java 键值属性配置' },
  { ext: 'env', name: '环境变量配置', desc: '环境变量定义文件' },
  { ext: 'md', name: 'Markdown 文档', desc: 'Markdown 轻量标记文档' },
  { ext: 'markdown', name: 'Markdown 文档', desc: 'Markdown 轻量标记文档' },
  { ext: 'rst', name: 'reStructuredText 文档', desc: 'Python 文档常用标记格式' },
  { ext: 'org', name: 'Org 文档', desc: 'Emacs Org-mode 大纲文档' },

  /* -------------------------------- 代码 -------------------------------- */
  { ext: 'rs', name: 'Rust 源码', desc: 'Rust 语言源代码' },
  { ext: 'py', name: 'Python 脚本', desc: 'Python 语言源代码' },
  { ext: 'js', name: 'JavaScript 脚本', desc: 'JavaScript 源代码' },
  { ext: 'jsx', name: 'JSX 源码', desc: 'React JavaScript 语法源码' },
  { ext: 'ts', name: 'TypeScript 源码', desc: 'TypeScript 源代码' },
  { ext: 'tsx', name: 'TSX 源码', desc: 'React TypeScript 语法源码' },
  { ext: 'mjs', name: 'ES Module 脚本', desc: 'ES Module 格式 JavaScript' },
  { ext: 'cjs', name: 'CommonJS 脚本', desc: 'CommonJS 格式 JavaScript' },
  { ext: 'java', name: 'Java 源码', desc: 'Java 语言源代码' },
  { ext: 'kt', name: 'Kotlin 源码', desc: 'Kotlin 语言源代码' },
  { ext: 'kts', name: 'Kotlin 脚本', desc: 'Kotlin 脚本文件' },
  { ext: 'go', name: 'Go 源码', desc: 'Go 语言源代码' },
  { ext: 'c', name: 'C 源码', desc: 'C 语言源代码' },
  { ext: 'h', name: 'C/C++ 头文件', desc: 'C/C++ 头文件' },
  { ext: 'cpp', name: 'C++ 源码', desc: 'C++ 语言源代码' },
  { ext: 'cc', name: 'C++ 源码', desc: 'C++ 语言源代码' },
  { ext: 'cxx', name: 'C++ 源码', desc: 'C++ 语言源代码' },
  { ext: 'hpp', name: 'C++ 头文件', desc: 'C++ 头文件' },
  { ext: 'hh', name: 'C++ 头文件', desc: 'C++ 头文件' },
  { ext: 'css', name: 'CSS 样式表', desc: '层叠样式表' },
  { ext: 'scss', name: 'SCSS 样式表', desc: 'Sass 语法样式表' },
  { ext: 'sass', name: 'Sass 样式表', desc: 'Sass 语法样式表' },
  { ext: 'less', name: 'Less 样式表', desc: 'Less 语法样式表' },
  { ext: 'sql', name: 'SQL 脚本', desc: '数据库查询脚本' },
  { ext: 'sh', name: 'Shell 脚本', desc: 'POSIX Shell 脚本' },
  { ext: 'bash', name: 'Bash 脚本', desc: 'Bash 脚本' },
  { ext: 'zsh', name: 'Zsh 脚本', desc: 'Zsh 脚本' },
  { ext: 'fish', name: 'Fish 脚本', desc: 'Friendly Shell 脚本' },
  { ext: 'bat', name: '批处理文件', desc: 'Windows 批处理脚本' },
  { ext: 'cmd', name: '命令脚本', desc: 'Windows NT 命令脚本' },
  { ext: 'ps1', name: 'PowerShell 脚本', desc: 'PowerShell 脚本' },
  { ext: 'rb', name: 'Ruby 脚本', desc: 'Ruby 语言源代码' },
  { ext: 'php', name: 'PHP 脚本', desc: 'PHP 语言源代码' },
  { ext: 'pl', name: 'Perl 脚本', desc: 'Perl 语言源代码' },
  { ext: 'lua', name: 'Lua 脚本', desc: 'Lua 语言源代码' },
  { ext: 'r', name: 'R 脚本', desc: 'R 统计语言脚本' },
  { ext: 'dart', name: 'Dart 源码', desc: 'Dart 语言源代码' },
  { ext: 'swift', name: 'Swift 源码', desc: 'Swift 语言源代码' },
  { ext: 'm', name: 'Objective-C 源码', desc: 'Objective-C 源代码' },
  { ext: 'mm', name: 'Objective-C++ 源码', desc: 'Objective-C++ 源代码' },
  { ext: 'scala', name: 'Scala 源码', desc: 'Scala 语言源代码' },
  { ext: 'clj', name: 'Clojure 源码', desc: 'Clojure 语言源代码' },
  { ext: 'ex', name: 'Elixir 源码', desc: 'Elixir 语言源代码' },
  { ext: 'exs', name: 'Elixir 脚本', desc: 'Elixir 脚本文件' },
  { ext: 'erl', name: 'Erlang 源码', desc: 'Erlang 语言源代码' },
  { ext: 'hrl', name: 'Erlang 头文件', desc: 'Erlang 头文件' },
  { ext: 'hs', name: 'Haskell 源码', desc: 'Haskell 语言源代码' },
  { ext: 'ml', name: 'OCaml 源码', desc: 'OCaml 语言源代码' },
  { ext: 'fs', name: 'F# 源码', desc: 'F# 语言源代码' },
  { ext: 'cs', name: 'C# 源码', desc: 'C# 语言源代码' },
  { ext: 'vb', name: 'VB 源码', desc: 'Visual Basic 源代码' },
  { ext: 'groovy', name: 'Groovy 脚本', desc: 'Groovy 语言源代码' },
  { ext: 'gradle', name: 'Gradle 脚本', desc: 'Gradle 构建脚本' },
  { ext: 'vue', name: 'Vue 组件', desc: 'Vue 单文件组件' },
  { ext: 'svelte', name: 'Svelte 组件', desc: 'Svelte 单文件组件' },
  { ext: 'astro', name: 'Astro 组件', desc: 'Astro 页面组件' },
  { ext: 'proto', name: 'Protobuf 定义', desc: 'Protocol Buffers 接口定义' },
  { ext: 'graphql', name: 'GraphQL 模式', desc: 'GraphQL Schema 定义' },
  { ext: 'gql', name: 'GraphQL 查询', desc: 'GraphQL 查询文件' },
  { ext: 'tf', name: 'Terraform 配置', desc: 'Terraform 基础设施配置' },
  { ext: 'hcl', name: 'HCL 配置', desc: 'HashiCorp 配置语言' },
  { ext: 'cmake', name: 'CMake 脚本', desc: 'CMake 构建脚本' },
  { ext: 'mk', name: 'Makefile', desc: 'make 构建脚本' },
  { ext: 'sbt', name: 'sbt 脚本', desc: 'Scala sbt 构建脚本' },
  { ext: 'bib', name: 'BibTeX 文献库', desc: 'LaTeX 参考文献数据' },
  { ext: 'tex', name: 'LaTeX 文档', desc: 'LaTeX 排版源文档' },
  { ext: 'sty', name: 'LaTeX 宏包', desc: 'LaTeX 样式宏包' },
  { ext: 'cls', name: 'LaTeX 类文件', desc: 'LaTeX 文档类定义' },
  { ext: 'adoc', name: 'AsciiDoc 文档', desc: 'AsciiDoc 标记文档' },
  { ext: 'textile', name: 'Textile 文档', desc: 'Textile 标记文档' },
  { ext: 'wiki', name: 'Wiki 文本', desc: 'Wiki 语法文本' },
  { ext: 'po', name: 'gettext 译稿', desc: 'GNU gettext 翻译文件' },
  { ext: 'pot', name: 'gettext 模板', desc: 'gettext 译稿模板' },
  { ext: 'srt', name: 'SRT 字幕', desc: 'SubRip 字幕文件' },
  { ext: 'vtt', name: 'WebVTT 字幕', desc: 'Web 视频字幕文件' },
  { ext: 'ass', name: 'ASS 字幕', desc: '高级字幕格式文件' },
  { ext: 'ics', name: 'iCalendar 日程', desc: '日历日程交换格式' },
  { ext: 'vcf', name: 'vCard 名片', desc: '电子名片格式' },
  { ext: 'svg', name: 'SVG 矢量图', desc: '可缩放矢量图形（文本源码）' },

  /* -------------------------------- 网页 -------------------------------- */
  { ext: 'html', name: 'HTML 网页', desc: '超文本标记语言网页' },
  { ext: 'htm', name: 'HTML 网页', desc: '超文本标记语言网页' },
  { ext: 'xhtml', name: 'XHTML 网页', desc: '严格 XHTML 网页' },

  /* ------------------------------- OOXML ------------------------------- */
  { ext: 'docx', name: 'Word 文档', desc: 'Office Open XML 文字文档' },
  { ext: 'docm', name: 'Word 宏文档', desc: '启用宏的 Word 文档' },
  { ext: 'xlsx', name: 'Excel 工作簿', desc: 'Office Open XML 电子表格' },
  { ext: 'xlsm', name: 'Excel 宏工作簿', desc: '启用宏的 Excel 工作簿' },
  { ext: 'xlsb', name: 'Excel 二进制簿', desc: 'Excel 二进制格式工作簿' },
  { ext: 'pptx', name: 'PowerPoint 演示文稿', desc: 'Office Open XML 演示文稿' },
  { ext: 'pptm', name: 'PowerPoint 宏文稿', desc: '启用宏的演示文稿' },
  { ext: 'pps', name: 'PowerPoint 放映', desc: 'PowerPoint 幻灯片放映' },
  { ext: 'ppsx', name: 'PowerPoint 放映', desc: 'PowerPoint 幻灯片放映' },
  { ext: 'pot', name: 'PowerPoint 模板', desc: 'PowerPoint 模板文件' },
  { ext: 'potx', name: 'PowerPoint 模板', desc: 'PowerPoint 模板文件' },

  /* ------------------------------ PDF / 电子书 ------------------------------ */
  { ext: 'pdf', name: 'PDF 文档', desc: '便携式文档格式（文字层提取）' },
  { ext: 'epub', name: 'EPUB 电子书', desc: '开放电子书格式' },
  { ext: 'mobi', name: 'MOBI 电子书', desc: 'Mobipocket 电子书格式' },
  { ext: 'azw', name: 'AZW 电子书', desc: 'Kindle 电子书格式' },
  { ext: 'azw3', name: 'AZW3 电子书', desc: 'Kindle Format 8 电子书' },
  { ext: 'fb2', name: 'FictionBook 电子书', desc: 'FictionBook XML 电子书' },
  { ext: 'lit', name: 'LIT 电子书', desc: 'Microsoft Reader 电子书' },
  { ext: 'kfx', name: 'KFX 电子书', desc: 'Kindle 新版电子书格式' },
  { ext: 'cbz', name: 'CBZ 漫画包', desc: '漫画书 ZIP 压缩包' },
  { ext: 'rtf', name: 'RTF 富文本', desc: '富文本格式文档' },
  { ext: 'odt', name: 'OpenDocument 文档', desc: 'ODF 开放文本文档' },

  /* ------------------------- 旧格式 / ODF（legacy） ------------------------- */
  { ext: 'doc', name: 'Word 97-2003 文档', desc: '旧版 Word 二进制文档' },
  { ext: 'dot', name: 'Word 模板', desc: '旧版 Word 模板文件' },
  { ext: 'xls', name: 'Excel 97-2003 工作簿', desc: '旧版 Excel 二进制工作簿' },
  { ext: 'xlt', name: 'Excel 模板', desc: '旧版 Excel 模板文件' },
  { ext: 'ppt', name: 'PowerPoint 97-2003', desc: '旧版 PowerPoint 演示文稿' },
  { ext: 'ods', name: 'OpenDocument 表格', desc: 'ODF 开放电子表格' },
  { ext: 'odp', name: 'OpenDocument 演示', desc: 'ODF 开放演示文稿' },

  /* --------------- WPS 原生格式（OLE 复合文档，已支持提取） --------------- */
  { ext: 'wps', name: 'WPS 文字文档', desc: 'WPS Office 文字文档' },
  { ext: 'wpt', name: 'WPS 文字模板', desc: 'WPS Office 文字模板' },
  { ext: 'et', name: 'WPS 表格', desc: 'WPS Office 电子表格' },
  { ext: 'ett', name: 'WPS 表格模板', desc: 'WPS Office 电子表格模板' },
  { ext: 'dps', name: 'WPS 演示文稿', desc: 'WPS Office 演示文稿' },
  { ext: 'dpt', name: 'WPS 演示模板', desc: 'WPS Office 演示文稿模板' },

  /* -------------------------------- 图片 -------------------------------- */
  { ext: 'png', name: 'PNG 图片', desc: '便携网络图形（需 OCR）' },
  { ext: 'jpg', name: 'JPEG 图片', desc: 'JPEG 照片格式（需 OCR）' },
  { ext: 'jpeg', name: 'JPEG 图片', desc: 'JPEG 照片格式（需 OCR）' },
  { ext: 'webp', name: 'WebP 图片', desc: 'Google WebP 图形（需 OCR）' },
  { ext: 'bmp', name: 'BMP 图片', desc: 'Windows 位图（需 OCR）' },
  { ext: 'tiff', name: 'TIFF 图片', desc: '标签图像格式（需 OCR）' },
  { ext: 'tif', name: 'TIFF 图片', desc: '标签图像格式（需 OCR）' },
  { ext: 'gif', name: 'GIF 图片', desc: '图形交换格式（需 OCR）' },

  /* ------------------------------- 压缩包 ------------------------------- */
  { ext: 'zip', name: 'ZIP 压缩包', desc: 'ZIP 归档（展开内层文本条目）' },
  { ext: 'tar', name: 'tar 归档', desc: 'tar 归档（展开内层文本条目）' },
  { ext: 'tgz', name: 'tar.gz 归档', desc: 'gzip 压缩的 tar 归档' },
  { ext: 'gz', name: 'gzip 压缩文件', desc: '单文件 gzip（或 tar.gz）' },
  { ext: 'bz2', name: 'bzip2 压缩文件', desc: '单文件 bzip2（或 tar.bz2）' },
  { ext: '7z', name: '7z 压缩包', desc: '7-Zip 归档（展开内层文本条目）' },
  { ext: 'rar', name: 'RAR 压缩包', desc: 'RAR 归档（展开内层文本条目）' },
  { ext: 'chm', name: 'CHM 帮助文档', desc: '微软编译 HTML 帮助文件' },

  /* -------------------------------- 邮件 -------------------------------- */
  { ext: 'eml', name: '邮件文件', desc: 'RFC 822 邮件（含邮件头与正文）' },

  /* ------------------------------ 二进制 ------------------------------ */
  { ext: 'exe', name: 'Windows 可执行文件', desc: 'PE 可执行程序（字符串提取）' },
  { ext: 'dll', name: 'Windows 动态库', desc: 'PE 动态链接库（字符串提取）' },
  { ext: 'so', name: 'Linux 共享库', desc: 'ELF 共享库（字符串提取）' },
  { ext: 'dylib', name: 'macOS 动态库', desc: 'Mach-O 动态库（字符串提取）' },
  { ext: 'bin', name: '二进制文件', desc: '通用二进制数据（字符串提取）' },
]

/* ------------------------------------------------------------------------- */
/* SUPPORTED_EXTS：与 stcore/src/extract/mod.rs 的常量表逐成员同步            */
/* ------------------------------------------------------------------------- */

/** TEXT_EXTS（mod.rs L69-170，纯文本/代码类） */
const TEXT_EXTS = [
  'txt', 'text', 'log', 'csv', 'tsv', 'json', 'xml', 'yaml', 'yml', 'toml', 'ini', 'conf', 'cfg',
  'properties', 'env', 'md', 'markdown', 'rst', 'org',
  'rs', 'py', 'js', 'jsx', 'ts', 'tsx', 'mjs', 'cjs', 'java', 'kt', 'kts', 'go', 'c', 'h', 'cpp',
  'cc', 'cxx', 'hpp', 'hh', 'css', 'scss', 'sass', 'less', 'sql', 'sh', 'bash', 'zsh', 'fish',
  'bat', 'cmd', 'ps1', 'rb', 'php', 'pl', 'lua', 'r', 'dart', 'swift', 'm', 'mm', 'scala', 'clj',
  'ex', 'exs', 'erl', 'hrl', 'hs', 'ml', 'fs', 'cs', 'vb', 'groovy', 'gradle', 'vue', 'svelte',
  'astro', 'proto', 'graphql', 'gql', 'tf', 'hcl', 'cmake', 'mk', 'sbt', 'bib', 'tex', 'sty',
  'cls', 'adoc', 'textile', 'wiki', 'po', 'pot', 'srt', 'vtt', 'ass', 'ics', 'vcf', 'svg',
]

/** HTML_EXTS（mod.rs L173） */
const HTML_EXTS = ['html', 'htm', 'xhtml']

/** OOXML_EXTS（mod.rs L176-178） */
const OOXML_EXTS = [
  'docx', 'docm', 'xlsx', 'xlsm', 'xlsb', 'pptx', 'pptm', 'pps', 'ppsx', 'pot', 'potx',
]

/** PDF_EXTS（mod.rs L181） */
const PDF_EXTS = ['pdf']

/** EPUB_EXTS（mod.rs L184） */
const EPUB_EXTS = ['epub']

/** EBOOK_EXTS（mod.rs L190-192） */
const EBOOK_EXTS = ['mobi', 'azw', 'azw3', 'fb2', 'lit', 'kfx', 'cbz', 'rtf', 'odt']

/** IMAGE_EXTS（mod.rs L195） */
const IMAGE_EXTS = ['png', 'jpg', 'jpeg', 'webp', 'bmp', 'tiff', 'tif', 'gif']

/** BINARY_EXTS（mod.rs L200） */
const BINARY_EXTS = ['exe', 'dll', 'so', 'dylib', 'bin']

/** LEGACY_EXTS（mod.rs L206-210） */
const LEGACY_EXTS = [
  'doc', 'dot', 'xls', 'xlsb', 'xlt', 'ppt', 'pps', 'pot', 'odt', 'ods', 'odp', 'rtf',
  // WPS 原生（OLE 复合文档；anydoc 失败时由 strings 兜底）
  'wps', 'wpt', 'et', 'ett', 'dps', 'dpt',
]

/**
 * dispatch() 字面量分支（mod.rs L288-409）：eml 与各压缩包格式不在上述常量表内，
 * 但提取器同样支持（eml 走 eml.rs；zip/tar/tgz/gz/bz2/7z/rar/chm 走对应展开器）。
 */
const DISPATCH_EXTS = ['eml', 'zip', 'tar', 'tgz', 'gz', 'bz2', '7z', 'rar', 'chm']

/** 提取器真实支持的扩展名集合（小写无点） */
export const SUPPORTED_EXTS: Set<string> = new Set([
  ...TEXT_EXTS,
  ...HTML_EXTS,
  ...OOXML_EXTS,
  ...PDF_EXTS,
  ...EPUB_EXTS,
  ...EBOOK_EXTS,
  ...IMAGE_EXTS,
  ...BINARY_EXTS,
  ...LEGACY_EXTS,
  ...DISPATCH_EXTS,
])

/* ------------------------------------------------------------------------- */
/* 查找辅助                                                                  */
/* ------------------------------------------------------------------------- */

/** ext → 目录条目（同 ext 多条目取首条） */
const BY_EXT = new Map(CATALOG.map((e) => [e.ext, e]))

/** 按 ext 取目录条目 */
export function entryForExt(ext: string): CatalogEntry | undefined {
  return BY_EXT.get(ext.toLowerCase())
}

/**
 * 按类型名取描述（D2 tooltip 用）：命中目录名 → desc；未命中 → 类型名本身。
 */
export function descForTypeName(name: string): string {
  return CATALOG.find((e) => e.name === name)?.desc ?? name
}

/** 「查找添加」候选过滤：按名称/扩展名包含匹配（大小写不敏感） */
export function filterCatalog(query: string): CatalogEntry[] {
  const q = query.trim().toLowerCase()
  if (!q) return CATALOG
  return CATALOG.filter((e) => e.name.toLowerCase().includes(q) || e.ext.includes(q))
}
