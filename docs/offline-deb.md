# 离线（内网）arm64 deb 构建与部署

本分支在 everytxt 基础上增加两件事：

1. **sjz 透明解密**：被「十进制.exe」加过标签的文档（扩展名不变，内容为 .SJZ 容器）
   在索引与预览前自动解密，无需手工转换。
2. **OCR 全离线**：PP-OCRv6 tiny 模型、ONNX Runtime、PDFium、ocr-helper 全部随 deb 分发，
   运行期不访问网络。

## 一、构建机要求（必须是原生 arm64）

- Ubuntu 24.04 arm64 或 Debian 12 arm64（需要 `webkit2gtk-4.1`，Ubuntu 22.04 也可）
- 系统包：
  `build-essential pkg-config dpkg-dev patchelf libgtk-3-dev libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev`
- Rust stable（`rustup`），Node ≥ 22，pnpm

> 为什么必须原生 arm64：Tauri 2 依赖 webkit2gtk-4.1，交叉编译需要整套 arm64 sysroot，
> 成本远高于直接在一台 arm64 机器上构建。

## 二、构建

```bash
bash scripts/build-arm64-deb.sh
# 产物：build/search-tool_0.1.0_arm64.deb
```

可选开关：

```bash
SKIP_BIN=1 bash scripts/build-arm64-deb.sh   # 复用已下载的 PDFium / OCR 模型
SKIP_UI=1  bash scripts/build-arm64-deb.sh   # 复用已构建的 ui/dist
```

首次构建会下载：PDFium arm64、OCR 三件套（ModelScope）、ONNX Runtime（pyke CDN）、
crates.io 依赖。**只有构建期需要网络，运行期完全离线。**

## 三、内网安装

```bash
sudo dpkg -i build/search-tool_0.1.0_arm64.deb
```

安装后布局：

```text
/usr/bin/search-tool
/usr/lib/search-tool/lib/ocr-helper
/usr/lib/search-tool/lib/libonnxruntime.so
/usr/lib/search-tool/lib/libpdfium.so
/usr/lib/search-tool/lib/search-tool/models/{det,rec}.onnx + dict.txt
```

应用启动时自动把上述路径注入环境变量（`SEARCH_TOOL_OCR_HELPER` /
`SEARCH_TOOL_OCR_MODEL_DIR` / `SEARCH_TOOL_PDFIUM_LIB` / `ORT_DYLIB_PATH`），
已有同名环境变量优先。

## 四、OCR 开关

- 桌面图标启动：`.desktop` 已带 `SEARCH_TOOL_OCR=1`，开箱即用。
- 命令行启动：`SEARCH_TOOL_OCR=1 search-tool`
- 关闭 OCR：去掉该变量即可（扫描件/图片将被跳过，不报错）。

## 五、sjz 解密行为

- 检测：读取文件尾部 512 字节解密，校验两处 `.SJZ` 魔数与明文长度，
  与文件大小无关，普通文件误判概率可忽略。
- 解密：整文件按 16 字节块解密，按容器记录的 `plain_len` 截断，
  产出物与原文档逐字节一致。
- 生效范围：索引（全文可搜）、文档预览、Office 高保真视图。
- 前提：目标文件由同一版本「十进制.exe」打标签（密钥表随程序固化）。
