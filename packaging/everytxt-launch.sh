#!/bin/sh
# EyzSearch 启动包装器
# 目的：把「点击图标没反应」变成「有日志、有弹窗、能定位」
set -u

APP=/usr/bin/eyzsearch
LIBDIR=/usr/lib/eyzsearch/lib
LOG_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/eyzsearch"
LOG="$LOG_DIR/launch.log"
mkdir -p "$LOG_DIR" 2>/dev/null || { LOG_DIR=/tmp; LOG=/tmp/eyzsearch-launch.log; }

popup() {
    # 尽量用系统已有的图形提示，都没有就只留日志
    msg="$1"
    if command -v zenity >/dev/null 2>&1; then
        zenity --error --title="EyzSearch 启动失败" --text="$msg" >/dev/null 2>&1 &
    elif command -v kdialog >/dev/null 2>&1; then
        kdialog --title "EyzSearch 启动失败" --error "$msg" >/dev/null 2>&1 &
    elif command -v yad >/dev/null 2>&1; then
        yad --error --text="$msg" >/dev/null 2>&1 &
    elif command -v xmessage >/dev/null 2>&1; then
        xmessage -center "$msg" >/dev/null 2>&1 &
    fi
}

{
    echo "===== $(date '+%F %T') EyzSearch 启动 ====="
    echo "kernel : $(uname -sr)"
    echo "系统   : $(. /etc/os-release 2>/dev/null; echo "${PRETTY_NAME:-unknown}")"
} >>"$LOG" 2>&1

# 1) 动态库自检：缺库时给出明确的库名和对应包名
MISSING=$(ldd "$APP" 2>/dev/null | awk '/not found/{print $1}' | sort -u)
if [ -n "$MISSING" ]; then
    {
        echo "[错误] 缺少运行库："
        echo "$MISSING"
    } >>"$LOG" 2>&1
    TEXT="缺少运行库：
$(echo "$MISSING" | sed 's/^/  • /')

常见对应包：
  libwebkit2gtk-4.0.so.37  ->  libwebkit2gtk-4.0-37
  libjavascriptcoregtk-4.0.so.18 -> libjavascriptcoregtk-4.0-18
  libsoup-2.4.so.1         ->  libsoup2.4-1
  libgdk_pixbuf-2.0.so.0   ->  libgdk-pixbuf-2.0-0

详细日志：$LOG"
    popup "$TEXT"
    exit 1
fi

# 2) 正常启动：日志重定向，若 3 秒内退出则弹窗报错
# 只注入资源「路径」，不注入 OCR 开关：
# OCR 是否启用由应用内「设置 → 文档识别」决定（默认关闭），存于 app.json。
# 若在此处 export SEARCH_TOOL_OCR=1，会覆盖用户关掉的开关（曾经的行为，已移除）。
export SEARCH_TOOL_OCR_HELPER="$LIBDIR/ocr-helper"
export SEARCH_TOOL_OCR_MODEL_DIR="$LIBDIR/eyzsearch/models"
export SEARCH_TOOL_PDFIUM_LIB="$LIBDIR/libpdfium.so"
export ORT_DYLIB_PATH="$LIBDIR/libonnxruntime.so"
"$APP" "$@" >>"$LOG" 2>&1 &
PID=$!
sleep 3
if ! kill -0 "$PID" 2>/dev/null; then
    TAIL=$(tail -n 15 "$LOG" 2>/dev/null)
    popup "EyzSearch 启动后立即退出。

日志尾部：
$TAIL

完整日志：$LOG"
    exit 1
fi
wait "$PID"
