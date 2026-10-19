#!/usr/bin/env bash
# 构建可内网部署的 arm64 deb（含 sjz 解密 + 离线 OCR/PDF 资源）。
#
# 运行环境要求（原生 arm64）：
#   - Ubuntu 24.04 arm64 / Debian 12 arm64（需 webkit2gtk-4.1）
#   - 系统包：build-essential pkg-config libgtk-3-dev libwebkit2gtk-4.1-dev
#             libayatana-appindicator3-dev librsvg2-dev patchelf dpkg-dev
#   - Rust stable（rustup；ocr-helper 会自动装 1.95）
#   - Node ≥ 22 + pnpm
#
# 用法：
#   scripts/build-arm64-deb.sh              # 全流程
#   SKIP_UI=1 scripts/build-arm64-deb.sh    # 复用已有 ui/dist
#   SKIP_BIN=1 scripts/build-arm64-deb.sh   # 复用已下载的 pdfium/模型
#
# 产物：build/search-tool_<ver>_arm64.deb
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

ARCH="$(uname -m)"
if [ "$ARCH" != "aarch64" ]; then
    echo "✗ 需要原生 arm64 构建机（当前 $ARCH）。arm64 deb 必须在 aarch64 上编译。" >&2
    exit 2
fi

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' app/Cargo.toml | head -1)"
[ -n "$VERSION" ] || { echo "✗ 读不到 app/Cargo.toml version" >&2; exit 2; }

echo "== [0/6] 检查系统依赖 =="
need_pkg="build-essential pkg-config dpkg-dev patchelf"
missing=""
for p in $need_pkg; do
    dpkg -s "$p" >/dev/null 2>&1 || missing="$missing $p"
done
for p in libgtk-3-dev libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev; do
    dpkg -s "$p" >/dev/null 2>&1 || missing="$missing $p"
done
if [ -n "$missing" ]; then
    echo "缺少系统包:$missing" >&2
    echo "sudo apt-get update && sudo apt-get install -y$missing" >&2
    exit 2
fi
pkg-config --exists webkit2gtk-4.1 || { echo "✗ webkit2gtk-4.1 不可用" >&2; exit 2; }
command -v cargo >/dev/null || { echo "✗ 未安装 Rust" >&2; exit 2; }
command -v node >/dev/null || { echo "✗ 未安装 Node" >&2; exit 2; }

echo "== [1/6] 获取第三方二进制（PDFium arm64 + OCR 模型）=="
if [ "${SKIP_BIN:-0}" = "1" ]; then
    echo "   跳过（SKIP_BIN=1）"
else
    bash scripts/fetch-binaries.sh --platform linux
fi

echo "== [2/6] 构建前端 =="
if [ "${SKIP_UI:-0}" = "1" ]; then
    echo "   跳过（SKIP_UI=1）"
    [ -f ui/dist/index.html ] || { echo "✗ ui/dist 不存在" >&2; exit 2; }
else
    command -v pnpm >/dev/null || npm i -g pnpm
    ( cd ui && pnpm install --frozen-lockfile && pnpm build )
fi

echo "== [3/6] 构建主程序（release + custom-protocol）=="
cargo build --release -p search-tool --features custom-protocol

echo "== [4/6] 构建 ocr-helper（独立 workspace，rust 1.95）=="
( cd ocr-helper && cargo build --release )

echo "== [5/6] 组装 deb 目录树 =="
STAGE="$ROOT/build/deb-root"
PKG="$STAGE/usr/lib/search-tool"
rm -rf "$STAGE"
mkdir -p "$PKG/lib/search-tool/models" \
         "$STAGE/usr/bin" \
         "$STAGE/usr/share/applications" \
         "$STAGE/usr/share/icons/hicolor/128x128/apps" \
         "$STAGE/usr/share/icons/hicolor/32x32/apps" \
         "$STAGE/DEBIAN"

install -m 0755 target/release/search-tool "$STAGE/usr/bin/search-tool"
install -m 0755 ocr-helper/target/release/ocr-helper "$PKG/lib/ocr-helper"

# OCR 模型（三件套）
cp -f third_party/ocr-models/*.onnx third_party/ocr-models/*.txt "$PKG/lib/search-tool/models/"

# PDFium
install -m 0644 third_party/pdfium/lib/libpdfium.so "$PKG/lib/libpdfium.so"

# ONNX Runtime 动态库（oar-ocr 构建期下载；找不到则跳过，静态链接时不需要）
ORT_SO="$(find ocr-helper/target -name 'libonnxruntime.so*' -type f 2>/dev/null | head -1 || true)"
if [ -n "$ORT_SO" ]; then
    install -m 0644 "$ORT_SO" "$PKG/lib/libonnxruntime.so"
    # 让 helper 在自身目录找 ORT，避免构建机绝对 RPATH 失效
    patchelf --set-rpath '$ORIGIN' "$PKG/lib/ocr-helper" || true
else
    echo "   ⚠ 未找到 libonnxruntime.so（若静态链接可忽略）"
fi

# 图标 + desktop
install -m 0644 app/icons/128x128.png "$STAGE/usr/share/icons/hicolor/128x128/apps/search-tool.png"
install -m 0644 app/icons/32x32.png   "$STAGE/usr/share/icons/hicolor/32x32/apps/search-tool.png"
cat > "$STAGE/usr/share/applications/search-tool.desktop" <<'EOF'
[Desktop Entry]
Type=Application
Name=everytxt
Comment=本地全文搜索引擎（含 sjz 解密与离线 OCR）
Exec=env SEARCH_TOOL_OCR=1 /usr/bin/search-tool
Icon=search-tool
Terminal=false
Categories=Utility;Office;
EOF

SIZE_KB="$(du -sk "$STAGE" | cut -f1)"
cat > "$STAGE/DEBIAN/control" <<EOF
Package: search-tool
Version: $VERSION
Section: utils
Priority: optional
Architecture: arm64
Installed-Size: $SIZE_KB
Maintainer: search-tool
Depends: libwebkit2gtk-4.1-0, libgtk-3-0 | libgtk-3-0t64, libayatana-appindicator3-1, librsvg2-2
Description: everytxt 本地全文搜索引擎
 本地全文检索（tantivy + jieba），覆盖 Office/PDF/电子书/压缩包；
 内置 sjz（十进制.exe 标签容器）透明解密与离线 PP-OCRv6 模型。
EOF

install -m 0755 packaging/postinst.sh "$STAGE/DEBIAN/postinst"

# 去掉构建期的绝对路径依赖（若存在）
patchelf --remove-rpath "$PKG/lib/ocr-helper" 2>/dev/null || true
patchelf --set-rpath '$ORIGIN' "$PKG/lib/ocr-helper" 2>/dev/null || true

echo "== [6/6] 打包 =="
mkdir -p "$ROOT/build"
OUT="$ROOT/build/search-tool_${VERSION}_arm64.deb"
dpkg-deb --build --root-owner-group "$STAGE" "$OUT" >/dev/null
dpkg-deb -I "$OUT" | sed -n '1,20p'
echo
echo "✅ 产物：$OUT ($(du -h "$OUT" | cut -f1))"
