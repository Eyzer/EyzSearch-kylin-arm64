#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""把 sjz 解密内核（从 十进制.exe 0x402200 反汇编生成的直通式 Python）
机械翻译成 Rust，并把 T 表 / ISS / 轮密钥导出为二进制 blob。

用法（在本仓库根目录）:
    python3 scripts/gen-sjz-kernel.py <kernel.py> <tables.json>

产物:
    stcore/src/extract/sjz_kernel.rs   生成代码，勿手改
    stcore/src/extract/sjz_tables.bin  5376 字节，布局：
        T0(1024) T1(1024) T2(1024) T3(1024) ISS(1024) RK(240)

翻译规则（与 Python 源语义等价）:
    - 全部值按 u32 运算，+ - * 用 wrapping_*
    - << 用 wrapping_shl（Python 侧随后 &0xffffffff，等价于截断）
    - RD1/RD4/WR4 映射到本文件的 rd1/rd4/wr4
    - ROL32/ROR32 映射到 u32::rotate_left/right
"""
import ast
import json
import os
import sys

U32 = "u32"


class Emit(ast.NodeVisitor):
    def expr(self, node) -> str:
        if isinstance(node, ast.Constant):
            v = int(node.value)
            assert 0 <= v <= 0xFFFFFFFF, v
            return f"{v}u32"
        if isinstance(node, ast.Name):
            return node.id
        if isinstance(node, ast.UnaryOp) and isinstance(node.op, ast.USub):
            return f"(0u32).wrapping_sub({self.expr(node.operand)})"
        if isinstance(node, ast.BinOp):
            a, b = self.expr(node.left), self.expr(node.right)
            if isinstance(node.op, ast.Add):
                return f"({a}).wrapping_add({b})"
            if isinstance(node.op, ast.Sub):
                return f"({a}).wrapping_sub({b})"
            if isinstance(node.op, ast.Mult):
                return f"({a}).wrapping_mul({b})"
            if isinstance(node.op, ast.BitAnd):
                return f"(({a}) & ({b}))"
            if isinstance(node.op, ast.BitOr):
                return f"(({a}) | ({b}))"
            if isinstance(node.op, ast.BitXor):
                return f"(({a}) ^ ({b}))"
            if isinstance(node.op, ast.LShift):
                return f"({a}).wrapping_shl({b})"
            if isinstance(node.op, ast.RShift):
                return f"(({a}) >> ({b}))"
        if isinstance(node, ast.Call):
            fn = node.func.id
            args = node.args
            if fn in ("RD1", "RD4"):
                return f"{fn.lower()}(m, {self.expr(args[1])} as usize)"
            if fn in ("ROL32", "ROR32"):
                n = int(args[1].value)
                rot = "rotate_left" if fn == "ROL32" else "rotate_right"
                return f"({self.expr(args[0])}).{rot}({n})"
            raise AssertionError(f"未知调用 {fn}")
        raise AssertionError(f"未知表达式 {ast.dump(node)}")

    def stmt(self, node) -> str:
        if isinstance(node, ast.Assign):
            out = []
            for tgt in node.targets:
                if isinstance(tgt, ast.Name):
                    out.append(f"{tgt.id} = {self.expr(node.value)};")
                    continue
                if isinstance(tgt, ast.Call) and tgt.func.id == "WR4":
                    a = tgt.args
                    out.append(f"wr4(m, {self.expr(a[1])} as usize, "
                               f"{self.expr(a[2])});")
                    continue
                raise AssertionError(f"未知赋值目标 {ast.dump(tgt)}")
            return "\n".join(out)
        if isinstance(node, ast.Expr) and isinstance(node.value, ast.Call):
            call = node.value
            if call.func.id == "WR4":
                a = call.args
                return (f"wr4(m, {self.expr(a[1])} as usize, "
                        f"{self.expr(a[2])});")
            raise AssertionError(f"未知表达式语句 {ast.dump(call)}")
        if isinstance(node, ast.Pass):
            return ""
        if isinstance(node, ast.Return):
            return "return;"
        raise AssertionError(f"未知语句 {ast.dump(node)}")


def main() -> None:
    if len(sys.argv) != 3:
        print(__doc__)
        sys.exit(2)
    kernel_src = open(sys.argv[1], encoding="utf-8").read()
    tables = json.load(open(sys.argv[2], encoding="utf-8"))

    tree = ast.parse(kernel_src)
    fn = next(n for n in tree.body if isinstance(n, ast.FunctionDef))
    e = Emit()
    body = []
    names = set()
    for node in fn.body:
        body.append(e.stmt(node))
        for sub in ast.walk(node):
            if isinstance(sub, ast.Name):
                names.add(sub.id)
    names.discard("m")
    names.discard("M")
    for f in ("RD1", "RD4", "WR4", "ROL32", "ROR32"):
        names.discard(f)
    # 形参（ebp8/ebp12）不得被重复声明，否则遮蔽入参
    for arg in fn.args.args:
        names.discard(arg.arg)

    out = []
    out.append("//! 由 scripts/gen-sjz-kernel.py 生成 —— 请勿手工编辑。")
    out.append("//! 语义来源：十进制.exe 0x402200 解密内核（T 表 AES-256 变体 + 索引扰动）。")
    out.append("#![allow(unused_assignments, unused_variables, unused_parens, redundant_semicolons, clippy::all)]")
    out.append("")
    out.append("use super::{rd1, rd4, wr4};")
    out.append("")
    out.append("pub(super) fn kernel(m: &mut [u8], ebp8: u32, ebp12: u32) {")
    for n in sorted(names):
        out.append(f"    let mut {n}: u32 = 0;")
    out.append("    let _ = ebp8;")
    out.append("    let _ = ebp12;")
    for line in body:
        out.append("    " + line)
    out.append("}")
    out.append("")

    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    rs_path = os.path.join(root, "stcore", "src", "extract", "sjz_kernel.rs")
    bin_path = os.path.join(root, "stcore", "src", "extract", "sjz_tables.bin")
    open(rs_path, "w", encoding="utf-8").write("\n".join(out))

    blob = bytearray()
    for key in ("T0", "T1", "T2", "T3", "ISS"):
        vals = [int(v, 16) if isinstance(v, str) else int(v) for v in tables[key]]
        assert len(vals) == 256, key
        for v in vals:
            blob += v.to_bytes(4, "little")
    rk = [int(v, 16) if isinstance(v, str) else int(v) for v in tables["RK"]]
    assert len(rk) == 60
    for v in rk:
        blob += v.to_bytes(4, "little")
    assert len(blob) == 5360, len(blob)
    open(bin_path, "wb").write(bytes(blob))

    print(f"ok: {rs_path} ({len(body)} 条语句), {bin_path} ({len(blob)} 字节)")


if __name__ == "__main__":
    main()
