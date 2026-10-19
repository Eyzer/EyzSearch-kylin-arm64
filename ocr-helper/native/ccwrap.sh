#!/bin/sh
# 链接器包装：仅当为 aarch64 目标链接时，在命令末尾追加
# libstdc++ 与 glibc 兼容垫片（rustc 会把 build script 声明的库排在 rlib 之前，
# 被 --as-needed 丢弃，导致 undefined reference）。
# 宿主侧（x86_64）链接 build script 时不能追加，否则找不到 aarch64 的库。
case "$*" in
    *aarch64*)
        exec gcc "$@" -lglibc_shim -lstdc++ -lm
        ;;
    *)
        exec gcc "$@"
        ;;
esac
