#!/bin/sh
# deb 安装后置脚本：确保随包 OCR helper 可执行
set -e
for f in /usr/lib/eyzsearch/lib/ocr-helper; do
    if [ -f "$f" ]; then
        chmod 0755 "$f" || true
    fi
done
# 旧版（0.2.2 及以前）资源目录，升级后清掉，避免两份模型共存
if [ -d /usr/lib/search-tool ] && [ ! -L /usr/lib/search-tool ]; then
    rm -rf /usr/lib/search-tool || true
fi
# 刷新桌面数据库与图标缓存（失败不影响安装）
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database -q || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor || true
fi
exit 0
