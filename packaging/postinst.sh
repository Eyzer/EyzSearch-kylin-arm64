#!/bin/sh
# deb 安装后置脚本：确保随包 OCR helper 可执行
set -e
for f in /usr/lib/search-tool/lib/ocr-helper; do
    if [ -f "$f" ]; then
        chmod 0755 "$f" || true
    fi
done
exit 0
