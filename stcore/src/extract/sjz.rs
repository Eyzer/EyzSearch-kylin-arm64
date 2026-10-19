//! sjz（十进制.exe）标签容器解密。
//!
//! ## 容器结构（样本与逆向结论双向确证）
//!
//! ```text
//! 磁盘文件 = AES-256（T 表变体）ECB，逐 16 字节块，长度恒为 16 的倍数
//! 解密结果 = 原始文档 + 尾部头（约 375 字节）
//! 尾部头   : 两处 ".SJZ" 魔数（相隔 0x173 字节），
//!            plain_len @ 头部 + 0x15e（u32 LE）
//! ```
//!
//! ## 检测策略
//!
//! 只解密**尾部 512 字节**（32 块），在解出的尾窗里定位 ".SJZ" 并校验第二处魔数
//! 与 plain_len 合法性。命中率与误报率：
//! - 真实 sjz 文件：必然命中（尾部头落在最后 512 字节内）
//! - 普通文件：需 32 块解密恰好产出两处相隔 0x173 的 ".SJZ" 且 plain_len 合法，
//!   概率可忽略
//!
//! ## 与 0x402200 内核的关系
//!
//! 解密核由 `scripts/gen-sjz-kernel.py` 从反汇编语义机械翻译而来
//! （见 `sjz_kernel.rs`），本文件只负责表装载、块循环与截断。

use super::ExtractError;

// ---- 内核访存助手（生成代码引用） ----

pub(super) fn rd1(m: &[u8], a: usize) -> u32 {
    m[a] as u32
}

pub(super) fn rd4(m: &[u8], a: usize) -> u32 {
    u32::from_le_bytes([m[a], m[a + 1], m[a + 2], m[a + 3]])
}

pub(super) fn wr4(m: &mut [u8], a: usize, v: u32) {
    m[a..a + 4].copy_from_slice(&v.to_le_bytes());
}

#[path = "sjz_kernel.rs"]
mod sjz_kernel;

/// T0/T1/T2/T3/ISS 各 1KB + 轮密钥 240B，地址布局与内核访存地址一一对应。
static TABLES: &[u8] = include_bytes!("sjz_tables.bin");

const MEM_SIZE: usize = 0x9000;
const IN_OFF: usize = 0x2000;
const OUT_OFF: usize = 0x2100;

/// 尾部窗口：检测只看最后 512 字节。
const TAIL_WINDOW: usize = 512;
/// 两处 ".SJZ" 魔数的间距（0x4020c0 处校验逻辑）。
const MAGIC_GAP: usize = 0x173;
/// plain_len 相对头部起点的偏移。
const PLAIN_LEN_OFF: usize = 0x15e;

struct Kernel {
    mem: Vec<u8>,
}

impl Kernel {
    fn new() -> Self {
        let mut mem = vec![0u8; MEM_SIZE];
        mem[..TABLES.len()].copy_from_slice(TABLES);
        Self { mem }
    }

    fn block(&mut self, blk: &[u8]) -> [u8; 16] {
        debug_assert_eq!(blk.len(), 16);
        self.mem[IN_OFF..IN_OFF + 16].copy_from_slice(&blk[..16]);
        self.mem[OUT_OFF..OUT_OFF + 16].fill(0);
        sjz_kernel::kernel(&mut self.mem, OUT_OFF as u32, IN_OFF as u32);
        let mut out = [0u8; 16];
        out.copy_from_slice(&self.mem[OUT_OFF..OUT_OFF + 16]);
        out
    }

    fn window(&mut self, bytes: &[u8], start: usize, len: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(len);
        let mut off = start;
        while off + 16 <= start + len {
            out.extend_from_slice(&self.block(&bytes[off..off + 16]));
            off += 16;
        }
        out
    }
}

/// 探测是否为 sjz 容器；命中返回解密后的明文长度。
///
/// 成本：32 次块解密（512 字节），与文件大小无关。
pub fn probe(bytes: &[u8]) -> Option<usize> {
    // sjz 密文长度恒为 16 的倍数；尾部头至少需要 512 字节窗口
    if bytes.len() < TAIL_WINDOW || bytes.len() % 16 != 0 {
        return None;
    }
    // 已是明文 zip：不可能是 sjz 密文（避免把正常 docx 再解一遍）
    if bytes.starts_with(b"PK\x03\x04") {
        return None;
    }
    let start = bytes.len() - TAIL_WINDOW;
    let mut k = Kernel::new();
    let tail = k.window(bytes, start, TAIL_WINDOW);

    let mut pos = None;
    for i in 0..tail.len().saturating_sub(4) {
        if &tail[i..i + 4] == b".SJZ" {
            pos = Some(i);
            break;
        }
    }
    let i = pos?;
    if i + MAGIC_GAP + 4 > tail.len() {
        return None;
    }
    if &tail[i + MAGIC_GAP..i + MAGIC_GAP + 4] != b".SJZ" {
        return None;
    }
    let p = i + PLAIN_LEN_OFF;
    if p + 4 > tail.len() {
        return None;
    }
    let plain = u32::from_le_bytes([tail[p], tail[p + 1], tail[p + 2], tail[p + 3]]) as usize;
    if plain == 0 || plain > bytes.len() {
        return None;
    }
    Some(plain)
}

/// 全量解密并截断到原始文档长度。
///
/// `plain_len` 来自 [`probe`]；缺失时退回 zip EOCD 推算。
pub fn decrypt(bytes: &[u8], plain_len: usize) -> Result<Vec<u8>, ExtractError> {
    if bytes.len() % 16 != 0 {
        return Err(ExtractError::Unsupported(
            "sjz 密文长度未按 16 字节对齐".to_string(),
        ));
    }
    let mut k = Kernel::new();
    let mut out = Vec::with_capacity(bytes.len());
    let mut off = 0;
    while off + 16 <= bytes.len() {
        out.extend_from_slice(&k.block(&bytes[off..off + 16]));
        off += 16;
    }
    let n = plain_len
        .min(out.len())
        .max(1);
    out.truncate(n);
    // 允许两类载体：zip（docx/xlsx/pptx）与 OLE 复合文档（doc/xls/ppt）
    let is_zip = out.starts_with(b"PK\x03\x04");
    let is_ole = out.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]);
    if !is_zip && !is_ole {
        return Err(ExtractError::Unsupported(
            "sjz 解密结果不是 zip / OLE 容器，已跳过".to_string(),
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 用已知明密文对锁定内核语义（取自样本首块）：
    /// 密文 ba86e21c... -> 明文 504b0304...（zip 头）
    #[test]
    fn first_block_matches_sample() {
        let ct = hex("ba86e21c0b25da60e63cc5b82c25ec99");
        let mut k = Kernel::new();
        let pt = k.block(&ct);
        assert_eq!(&pt[..4], b"PK\x03\x04");
        assert_eq!(hex_str(&pt), "504b030414000600080000002100e721");
    }

    #[test]
    fn probe_rejects_plain_zip_and_short_input() {
        assert!(probe(b"PK\x03\x04rest").is_none());
        assert!(probe(&vec![0u8; 1024]).is_none());
        // 长度非 16 倍数直接拒绝
        assert!(probe(&vec![0u8; 1025]).is_none());
    }

    #[test]
    fn probe_does_not_panic_on_random() {
        let mut x: u64 = 0x9e3779b97f4a7c15;
        for n in [1024usize, 4096, 110592] {
            let mut buf = vec![0u8; n];
            for b in buf.iter_mut() {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                *b = x as u8;
            }
            let _ = probe(&buf);
        }
    }

    fn hex(s: &str) -> [u8; 16] {
        let mut out = [0u8; 16];
        for (i, b) in out.iter_mut().enumerate() {
            *b = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap();
        }
        out
    }

    fn hex_str(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }
}
