/* glibc 兼容垫片
 *
 * 预编译的 ONNX Runtime 静态库在 glibc >= 2.38 的 C23 头文件下编译，
 * 内部引用了 __isoc23_strtol / __isoc23_strtoll / __isoc23_strtoull。
 * Debian 12 的 glibc 是 2.36，没有这三个符号，链接期报 undefined reference。
 *
 * 三者的语义与经典的 strtol/strtoll/strtoull 完全一致，唯一差别是
 * 额外接受 0b 二进制前缀（C23 新增）。ONNX Runtime 解析配置数值不会用到
 * 0b 前缀，因此这里直接转发给同名经典函数，行为等价。
 */
#include <stdlib.h>

long __isoc23_strtol(const char *nptr, char **endptr, int base) {
    return strtol(nptr, endptr, base);
}

long long __isoc23_strtoll(const char *nptr, char **endptr, int base) {
    return strtoll(nptr, endptr, base);
}

unsigned long long __isoc23_strtoull(const char *nptr, char **endptr, int base) {
    return strtoull(nptr, endptr, base);
}

unsigned long __isoc23_strtoul(const char *nptr, char **endptr, int base) {
    return strtoul(nptr, endptr, base);
}
