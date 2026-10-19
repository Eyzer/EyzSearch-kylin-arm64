// 构建脚本：补齐预编译 ONNX Runtime 静态库缺失的宿主侧符号。
//
// 1) 预编译 ORT 静态库内部是 C++，链接时需要显式 -lstdc++（rustc 的 gcc 驱动
//    不会自动带）。
// 2) 该库在 glibc >= 2.38 环境编译，引用了 __isoc23_strtol 等 C23 符号；
//    Debian 12 (glibc 2.36) 没有，用 native/glibc_shim.c 转发到经典函数。
//
// 不引入 cc crate，直接调系统 gcc/ar，避免额外依赖。

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=native/glibc_shim.c");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR 缺失"));
    let obj = out_dir.join("glibc_shim.o");
    let lib = out_dir.join("libglibc_shim.a");

    let cc = env::var("CC").unwrap_or_else(|_| "gcc".to_string());
    let status = Command::new(&cc)
        .args(["-O2", "-fPIC", "-c", "native/glibc_shim.c", "-o"])
        .arg(&obj)
        .status()
        .expect("调用 gcc 失败");
    assert!(status.success(), "编译 glibc_shim.c 失败");

    let _ = std::fs::remove_file(&lib);
    let status = Command::new("ar")
        .arg("crs")
        .arg(&lib)
        .arg(&obj)
        .status()
        .expect("调用 ar 失败");
    assert!(status.success(), "归档 libglibc_shim.a 失败");

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=glibc_shim");
    // C++ 运行库：ORT 静态库内的 std:: 符号
    println!("cargo:rustc-link-lib=dylib=stdc++");
}
