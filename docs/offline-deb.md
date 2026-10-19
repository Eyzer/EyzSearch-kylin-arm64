# 离线（内网）arm64 deb 构建与部署

本分支在 everytxt 基础上增加两件事：

1. **sjz 透明解密**：被「十进制.exe」加过标签的文档（扩展名不变，内容为 .SJZ 容器）
   在索引与预览前自动解密，无需手工转换。
2. **OCR 全离线**：PP-OCRv6 tiny 模型、ONNX Runtime、PDFium、ocr-helper 全部随 deb 分发，
   运行期不访问网络。

## 产物

`everytxt_0.1.0_arm64.deb`（29 MB）

```bash
sudo dpkg -i everytxt_0.1.0_arm64.deb
# 离线机器缺依赖时：
# sudo dpkg -i --force-depends everytxt_0.1.0_arm64.deb
```

启动：应用菜单「everytxt 本地全文搜索」，或终端 `SEARCH_TOOL_OCR=1 search-tool`

## 包内布局

```
/usr/bin/search-tool
/usr/lib/search-tool/lib/libpdfium.so
/usr/lib/search-tool/lib/libonnxruntime.so
/usr/lib/search-tool/lib/ocr-helper
/usr/lib/search-tool/lib/search-tool/models/*.onnx
/usr/share/applications/search-tool.desktop
```

主程序启动时把上述路径写入 `SEARCH_TOOL_OCR_HELPER / SEARCH_TOOL_OCR_MODEL_DIR /
SEARCH_TOOL_PDFIUM_LIB / ORT_DYLIB_PATH`，运行期完全不联网。

## 目标机前提

| 项 | 要求 | 说明 |
|---|---|---|
| 架构 | aarch64 | — |
| glibc | ≥ 2.35 | 本次在 Debian 12（glibc 2.36）上编译 |
| libwebkit2gtk-4.1 | 必须存在 | Tauri 2 硬依赖 4.1 ABI |

自检：

```bash
ldd --version | head -1
dpkg -l | grep -E 'webkit2gtk|javascriptcoregtk'
```

麒麟 V10（glibc 2.31 + webkit2gtk-4.0）跑不了这个包，需要改到 glibc 2.31 的 arm64
环境并从源码编 webkit2gtk-4.1，或替换桌面外壳。

## 构建（arm64 Linux）

```bash
sudo apt-get install -y build-essential pkg-config dpkg-dev patchelf \
  libgtk-3-dev libwebkit2gtk-4.1-dev librsvg2-dev libayatana-appindicator3-dev
bash scripts/build-arm64-deb.sh
```

无 arm64 机器时，在 qemu-user 模拟的 arm64 rootfs 里跑（本次做法）：

```bash
debootstrap --arch=arm64 --variant=minbase bookworm /opt/arm64root https://mirrors.ustc.edu.cn/debian
update-binfmts --enable qemu-aarch64
ALLOW_EMULATED=1 bash scripts/build-arm64-deb.sh
```

耗时参考（8 核 x86 + qemu）：主程序 136 分钟，OCR 助手约 2.5 小时。

## 踩过的坑（都已修）

1. **ort 的 `download-binaries` 不能用**：它拉的是 pyke 编译的**静态库**
   `libonnxruntime.a`，用 GCC 13+ / glibc 2.38 构建，引用
   `__cxa_call_terminate`、`std::string::_M_replace_cold`、`__isoc23_strtol`，
   Debian 12 链接期全部 undefined。
   → 改用 `ort` 的 `load-dynamic`，运行期 dlopen 官方 `libonnxruntime.so.1.22.0`
   （只要求 glibc 2.27 / GLIBCXX 3.4.22）。
2. **rustc 把 build script 声明的库排在 rlib 之前**，配合 `--as-needed` 会被丢弃；
   改 `load-dynamic` 后不再链接任何 ORT 库，问题消失。
3. GitHub release 直连不稳，`ghfast.top` / `gh-proxy.com` 可回退。
4. qemu 下 ORT 进程退出偶发 `SIGSEGV`，发生在输出 JSON 之后；主程序按 stdout
   流式解析、不看退出码，不影响功能。
