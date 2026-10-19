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
if [ "$ARCH" != "aarch64" ] && [ "${ALLOW_EMULATED:-0}" != "1" ]; then
    echo "✗ 需要原生 arm64 构建机（当前 $ARCH）。arm64 deb 必须在 aarch64 上编译。" >&2
    echo "  在 qemu-user 模拟的 arm64 rootfs 内运行时，设 ALLOW_EMULATED=1 跳过此检查。" >&2
    exit 2
fi
if [ "${ALLOW_EMULATED:-0}" = "1" ]; then
    echo "⚠ 模拟构建模式（${ARCH} 内核 + arm64 userland）"
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
if [ "${SKIP_UI:-0}" != "1" ]; then
    command -v node >/dev/null || { echo "✗ 未安装 Node" >&2; exit 2; }
fi

echo "== [1/6] 获取第三方二进制（PDFium arm64 + OCR 模型 + ONNX Runtime）=="
if [ "${SKIP_BIN:-0}" = "1" ]; then
    echo "   跳过（SKIP_BIN=1）"
else
    bash scripts/fetch-binaries.sh --platform linux
fi

# ONNX Runtime 必须是**动态库**：ocr-helper 走 ort 的 load-dynamic，
# 不用 pyke 的静态库（那是 GCC 13+ 编的，Debian 12 链接会 undefined reference）
ORT_TGZ_VER=1.22.0
ORT_DIR="$ROOT/third_party/onnxruntime"
if [ ! -f "$ORT_DIR/lib/libonnxruntime.so" ]; then
    echo "   下载 ONNX Runtime ${ORT_TGZ_VER} (linux-aarch64)"
    mkdir -p "$ORT_DIR"
    tmp="$(mktemp -d)"
    url="https://github.com/microsoft/onnxruntime/releases/download/v${ORT_TGZ_VER}/onnxruntime-linux-aarch64-${ORT_TGZ_VER}.tgz"
    # GitHub 直连不稳，按顺序回退镜像
    ok=0
    for u in "$url" "https://ghfast.top/$url" "https://gh-proxy.com/$url"; do
        if curl -fsSL --retry 2 --max-time 600 -o "$tmp/ort.tgz" "$u"; then ok=1; break; fi
    done
    [ "$ok" = "1" ] || { echo "✗ ONNX Runtime 下载失败" >&2; exit 2; }
    tar xzf "$tmp/ort.tgz" -C "$tmp" --strip-components=1
    install -m 0644 "$tmp/lib/libonnxruntime.so" "$ORT_DIR/lib/libonnxruntime.so"
    rm -rf "$tmp"
fi
ls -lh "$ORT_DIR/lib/libonnxruntime.so"

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
strip --strip-unneeded target/release/search-tool 2>/dev/null || true

echo "== [4/6] 构建 ocr-helper（独立 workspace）=="
# copy-dylibs 需要构建期就知道 ORT 动态库位置
( cd ocr-helper && ORT_DYLIB_PATH="$ORT_DIR/lib/libonnxruntime.so" cargo build --release )
strip --strip-unneeded ocr-helper/target/release/ocr-helper 2>/dev/null || true

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

# ONNX Runtime 动态库（load-dynamic 运行时 dlopen，路径由主程序注入 ORT_DYLIB_PATH）
ORT_SO="$ORT_DIR/lib/libonnxruntime.so"
install -m 0644 "$ORT_SO" "$PKG/lib/libonnxruntime.so"

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
Depends: libwebkit2gtk-4.1-0, libjavascriptcoregtk-4.1-0, libgtk-3-0 | libgtk-3-0t64, libsoup-3.0-0, libglib2.0-0, libcairo2, libpango-1.0-0, libgdk-pixbuf-2.0-0, libdbus-1-3, libstdc++6, libgcc-s1
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
