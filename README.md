# EyzSearch 全文搜索

本地文件内容全文搜索工具，面向**银河麒麟 V10 桌面版 / arm64（飞腾等）**。

时间该花在正事上，不该耗在找文件上。所有索引与检索都在本机完成，不联网、不上传任何内容。

基于 [shadyrispy/everytxt](https://github.com/shadyrispy/everytxt) 二次开发，
桌面端 [Tauri](https://tauri.app/) + [React](https://react.dev/)，引擎 Rust。

## 功能特性

- **办公文档**：Word / Excel / PowerPoint、WPS 文字表格演示、PDF、电子书、纯文本
- **保密标签**：带保密标签文件内容可检索（`.SJZ` 加密容器透明解密，无需手动解包）
- **离线 OCR**：默认关闭，可在「设置 → 文档识别」开启，扫描件与图片转可检索文字
- **增量索引**：重建时只处理新增与修改过的文件，未变文件走提取缓存
- **高级语法**：`&` 与、`|` 或、`!` 非、`()` 分组、`""` 精确

- **全文检索**：分词（[jieba-rs](https://crates.io/crates/jieba-rs)）+ 全文索引
  （[tantivy](https://crates.io/crates/tantivy) / [rusqlite](https://crates.io/crates/rusqlite)），
  命中数徽标、结果分页、搜索历史与书签
- **多搜索模式**：普通 / 正则 / 全词 / 大小写敏感；时间区间过滤；指定目录与文件类型过滤
- **拼音与同义词**：拼音检索通道 + 同义词扩展（[pinyin](https://crates.io/crates/pinyin)）
- **格式覆盖**：txt/md/html/eml、docx/xlsx/pptx、pdf、epub/mobi/fb2 等电子书、
  zip/tar/gz/bz2/7z/rar/chm 压缩包、二进制 strings
- **PDF**：文本层直提 + 扫描件页面渲染与命中框标注（[pdfium-render](https://crates.io/crates/pdfium-render)）
- **OCR**：扫描件与图片经 ONNX Runtime 推理识别（[oar-ocr](https://crates.io/crates/oar-ocr) +
  [ort](https://crates.io/crates/ort)，PaddleOCR PP-OCRv6 tiny 模型）
- **文档预览**：富文本/Office 高保真视图（[@silurus/ooxml](https://www.npmjs.com/package/@silurus/ooxml)）、
  PDF 页渲染、文档内查找、词组统计、minimap
- **索引管理**：目录规则（glob）、定时索引、暂停/继续/取消、实时文件监听
  （[notify](https://crates.io/crates/notify)）
- **桌面集成**：全局热键、托盘、右击目录搜索（Linux/macOS）、回收站删除

## 安装

从 [Releases](https://github.com/Eyzer/EyzSearch-kylin-arm64/releases) 下载对应 deb，双击安装或用命令行：

```bash
sudo dpkg -i EyzSearch_0.2.4+kylin1_arm64.deb
sudo apt-get -f install    # 缺依赖时补齐
```

**运行环境**：银河麒麟 V10 桌面版（webkit2gtk-4.0 / glibc 2.28），arm64。
安装后从开始菜单启动「EyzSearch」，或在终端执行 `eyzsearch`。

命令行索引（适合定时任务）：

```bash
eyzsearch cli index --dir /home/user/文档          # 增量
eyzsearch cli index --dir /home/user/文档 --full   # 强制全量重建
```

## 技术栈

| 层 | 技术 |
|---|---|
| 引擎（`stcore/`） | Rust · [tantivy](https://crates.io/crates/tantivy) · [rusqlite](https://crates.io/crates/rusqlite)（bundled SQLite）· [jieba-rs](https://crates.io/crates/jieba-rs) · [rayon](https://crates.io/crates/rayon) · [notify](https://crates.io/crates/notify) · [regex](https://crates.io/crates/regex) · [aho-corasick](https://crates.io/crates/aho-corasick) · [globset](https://crates.io/crates/globset) |
| 提取层 | [zip](https://crates.io/crates/zip) · [tar](https://crates.io/crates/tar) · [flate2](https://crates.io/crates/flate2) · [bzip2](https://crates.io/crates/bzip2) · [sevenz-rust2](https://crates.io/crates/sevenz-rust2) · [unrar](https://crates.io/crates/unrar) · [chmlib](https://crates.io/crates/chmlib) · [pdf-extract](https://crates.io/crates/pdf-extract) · [quick-xml](https://crates.io/crates/quick-xml) · [ebook-rs](https://crates.io/crates/ebook-rs) · [encoding_rs](https://crates.io/crates/encoding_rs) · [chardetng](https://crates.io/crates/chardetng) · [mail-parser](https://crates.io/crates/mail-parser) · [scraper](https://crates.io/crates/scraper) |
| PDF / OCR | [pdfium-render](https://crates.io/crates/pdfium-render)（[PDFium](https://github.com/bblanchon/pdfium-binaries)）· [oar-ocr](https://crates.io/crates/oar-ocr) + [ort](https://crates.io/crates/ort)（ONNX Runtime）· [PP-OCRv6 tiny 模型](https://www.modelscope.cn/models/greatv/oar-ocr) |
| 外壳（`app/`） | [Tauri 2](https://tauri.app/) · [tauri-plugin-dialog](https://crates.io/crates/tauri-plugin-dialog) · [tauri-plugin-opener](https://crates.io/crates/tauri-plugin-opener) · [clap](https://crates.io/crates/clap) · [trash](https://crates.io/crates/trash) · [arboard](https://crates.io/crates/arboard) |
| 前端（`ui/`） | [React 19](https://react.dev/) · [TypeScript](https://www.typescriptlang.org/) · [Vite](https://vite.dev/) · [@tauri-apps/api](https://crates.io/crates/tauri) · [@silurus/ooxml](https://www.npmjs.com/package/@silurus/ooxml) |

第三方补丁源码（Windows 适配）以 vendored crate 形式存放于 `third_party/`，
各补丁根因见 `docs/cross-platform-assessment.md`。

## 构建

**前置**：Rust stable、Node ≥ 22 + pnpm、C/C++ 工具链。
第三方二进制（PDFium / OCR 模型）不入库，先获取：

```bash
scripts/fetch-binaries.sh                # 按当前平台
scripts/fetch-binaries.sh --platform all # Linux + Windows 全取
```

```bash
# 引擎测试（无需 Node）
cargo test --workspace

# 桌面应用
cd ui && pnpm install && pnpm build && cd ..
cargo tauri dev
```

国内网络 GitHub 直连失败时：`GH_PROXY=https://gh-proxy.com scripts/fetch-binaries.sh`。

## 平台支持

| 平台 | 构建 | 运行 | 打包 |
|---|---|---|---|
| Linux x86_64 | ✅ | ✅ | deb / AppImage |
| Linux arm64 | ✅ | ✅ | deb / AppImage |
| Windows x64 | ✅ | ✅ | NSIS（`cargo tauri build --bundles nsis`） |
| macOS | 引擎 ✅ | 外壳未验证 | — |

平台差异与 Windows 构建细节见 `docs/windows-runbook.md`，
跨平台评估见 `docs/cross-platform-assessment.md`。

## 目录结构

```
stcore/       索引/检索引擎（Rust library，237 个测试）
app/          Tauri 2 桌面外壳（含 CLI）
ui/           React 19 + Vite 前端
ocr-helper/   OCR 子进程（独立 workspace，oar-ocr 需 rustc ≥ 1.95）
third_party/  vendored patch crate + 二进制占位目录（内容由脚本获取）
scripts/      fetch-binaries.sh（第三方二进制获取）
docs/         架构 / 跨平台评估 / Windows runbook / UI 规格 / 性能基准
```

## 质量门禁

每批次执行：`cargo fmt --all` · `cargo clippy --workspace --all-targets -- -D warnings` ·
`cargo test --workspace`（237 通过）· `pnpm build`。
CI（`.github/workflows/ci.yml`）在 Linux x86 / Linux arm / Windows 三平台原生镜像上
自动执行获取二进制 → fmt → clippy → test。

## 文档

| 文档 | 内容 |
|---|---|
| `docs/architecture-v2.md` | 架构定稿（含平台适配与单实例决策记录） |
| `docs/cross-platform-assessment.md` | 三平台可构建性评估与验证矩阵 |
| `docs/windows-runbook.md` | Windows 构建 / 冒烟 / 打包手册 |
| `docs/ui-redesign.md` | UI 设计规格 |
| `docs/bench-d7-time-filter.md` | 时间筛选性能基准 |
| `docs/pdf-helper-assessment.md` | OCR helper 工具链隔离评估 |

各阶段交付报告（`P0+-REPORT.md` ~ `R35-ACCEPTANCE-REPORT.md`）记录了逐批次实施与验收证据。

## 致谢

本项目站在众多开源库之上，完整清单见上方技术栈表与各 `Cargo.toml` /
`package.json`。特别感谢 [Tauri](https://tauri.app/)、[tantivy](https://crates.io/crates/tantivy)、
[jieba-rs](https://crates.io/crates/jieba-rs)、[oar-ocr](https://crates.io/crates/oar-ocr) 与
[PDFium](https://github.com/bblanchon/pdfium-binaries) 的维护者们。
