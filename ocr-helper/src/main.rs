//! OCR helper（P3-A / P4-A / P5）：**独立进程**，把重型推理依赖（ONNX Runtime +
//! PP-OCR 模型）与主程序隔离。
//!
//! ## 为什么独立进程
//! - `oar-ocr` 要求 rustc ≥ 1.95（主工程 stable 1.93）→ 工具链隔离（`rust-toolchain.toml`）
//! - ONNX Runtime 动态库与模型不进主二进制，主程序体积/内存不受影响
//! - 推理崩溃/内存峰值（plan 风险 R2）不波及索引进程
//!
//! ## 模型来源（按优先级；P5 起随安装包分发）
//! 1. 环境变量 `SEARCH_TOOL_OCR_MODEL_DIR`
//! 2. 可执行文件同级 `models/`（打包态，见 `tauri.conf.json` resources）
//! 3. 源码树 `third_party/ocr-models/`
//! 4. 以上皆无 → 退回注册名（`auto-download` 从 ModelScope 拉取到 `$OAR_HOME`）
//!
//! ## 协议（响应恒为 stdout 一行 JSON；请求二选一）
//! 请求：`{"image_path": "/abs/path/page.png"}`
//! - **首选 argv**：`ocr-helper '{"image_path":"..."}'`（argv[1] 即请求 JSON）
//! - 回退 stdin：一行 JSON（`echo '{...}' | ocr-helper`，手动调试用）
//!
//! 为什么首选 argv：Windows 上 Rust `Command::spawn` 的 stdin 管道
//! （OUTBOUND 服务器 + GENERIC_READ 客户端）在部分机器上确定性返回
//! ERROR_PIPE_BUSY(231)；stdout 管道（INBOUND + GENERIC_WRITE）不受影响，
//! 故请求走 argv、响应走 stdout。
//! 成功：`{"ok":true,"image_width":900,"image_height":300,
//!        "lines":[{"text":"...","confidence":0.99,"box":[x,y,w,h]}]}`（box 为像素坐标）
//! 失败（退出码 1）：`{"ok":false,"error":"..."}`
//!
//! ## 构建（需 1.95 工具链，本目录已 pin）
//! ```bash
//! cd ocr-helper && cargo build --release
//! ```

use anyhow::{anyhow, Result};
use oar_ocr::prelude::*;
use serde::{Deserialize, Serialize};
use std::io::BufRead;
use std::path::{Path, PathBuf};

/// 模型档位：默认 tiny（plan §5.2 桌面默认档，RSS ~0.5GB）
const DET_MODEL: &str = "pp-ocrv6_tiny_det.onnx";
const REC_MODEL: &str = "pp-ocrv6_tiny_rec.onnx";
const DICT_FILE: &str = "ppocrv6_tiny_dict.txt";

#[derive(Debug, Deserialize)]
struct Request {
    image_path: String,
}

#[derive(Debug, Serialize)]
struct Line {
    text: String,
    confidence: f32,
    /// 像素坐标 [x, y, w, h]（左上原点；stcore 侧归一化）
    #[serde(rename = "box")]
    bbox: [f32; 4],
}

#[derive(Debug, Serialize)]
struct Response {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    lines: Option<Vec<Line>>,
    image_width: u32,
    image_height: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

fn main() {
    // 模式一：单次调用 —— `ocr-helper '{"image_path":"..."}'`
    // 保留它是为了向后兼容与手动排障（脚本里 echo 一行也能跑）。
    if let Some(arg) = std::env::args().nth(1) {
        let arg = arg.trim().to_string();
        if !arg.is_empty() {
            match serde_json::from_str::<Request>(&arg) {
                Ok(req) => match run(&req) {
                    Ok((lines, w, h)) => print_json(&Response {
                        ok: true,
                        lines: Some(lines),
                        image_width: w,
                        image_height: h,
                        error: None,
                    }),
                    Err(e) => {
                        print_json(&err_resp(e.to_string()));
                        std::process::exit(1);
                    }
                },
                Err(e) => {
                    print_json(&err_resp(format!("argv 请求解析失败: {e}")));
                    std::process::exit(1);
                }
            }
            return;
        }
    }
    // 模式二：常驻服务 —— stdin 一行请求，stdout 一行响应
    serve();
}

/// 常驻服务循环：**引擎只初始化一次**，之后复用。
///
/// 旧版每张图 / 每页 PDF 都 spawn 一次进程，等于每次都重新加载 ONNX Runtime
/// 与三个 PP-OCR 模型（数百毫秒到数秒）。一份 20 页的扫描件就白等十几秒。
///
/// 出错不退出进程：单张图失败只回 `ok:false`，下一行请求照常处理。
fn serve() {
    use std::io::Write as _;

    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let mut line = String::new();

    // 引擎初始化失败也继续服务：后续每个请求都回同一条错误，
    // 避免主程序反复重启一个注定失败的子进程。
    let (det, rec, dict) = resolve_models();
    let mut engine = match OAROCRBuilder::new(det, rec, dict).build() {
        Ok(e) => Some(e),
        Err(e) => {
            eprintln!("OCR 引擎初始化失败: {e}");
            None
        }
    };

    loop {
        line.clear();
        match input.read_line(&mut line) {
            Ok(0) => break, // 主进程关闭了管道
            Ok(_) => {}
            Err(_) => break,
        }
        let raw = line.trim();
        if raw.is_empty() {
            continue;
        }
        if raw == "exit" || raw == "quit" {
            break;
        }

        // 全部内联：ocr.predict 的具体类型由编译器推断，不需要写出引擎类型名
        let resp = match serde_json::from_str::<Request>(raw) {
            Err(e) => err_resp(format!("请求解析失败: {e}")),
            Ok(req) => {
                let path = PathBuf::from(&req.image_path);
                if !path.is_file() {
                    err_resp(format!("图像不存在: {}", path.display()))
                } else {
                    match load_image(&path) {
                        Err(e) => err_resp(format!("读取图像失败: {e}")),
                        Ok(image) => match engine.as_mut() {
                            None => err_resp("OCR 引擎不可用（模型或 ONNX Runtime 加载失败）".into()),
                            Some(ocr) => {
                                let (iw, ih) = (image.width(), image.height());
                                match ocr.predict(vec![image]) {
                                    Err(e) => err_resp(format!("识别失败: {e}")),
                                    Ok(results) => {
                                        let mut lines = Vec::new();
                                        for page in &results {
                                            for region in &page.text_regions {
                                                let Some((text, confidence)) =
                                                    region.text_with_confidence()
                                                else {
                                                    continue;
                                                };
                                                if text.trim().is_empty() {
                                                    continue;
                                                }
                                                let bb = &region.bounding_box;
                                                let (x, y) = (bb.x_min(), bb.y_min());
                                                lines.push(Line {
                                                    text: text.to_string(),
                                                    confidence,
                                                    bbox: [
                                                        x,
                                                        y,
                                                        bb.x_max() - x,
                                                        bb.y_max() - y,
                                                    ],
                                                });
                                            }
                                        }
                                        Response {
                                            ok: true,
                                            lines: Some(lines),
                                            image_width: iw,
                                            image_height: ih,
                                            error: None,
                                        }
                                    }
                                }
                            }
                        },
                    }
                }
            }
        };

        // 立即 flush：主进程在另一头逐行等，缓冲住会死锁
        match serde_json::to_string(&resp) {
            Ok(s) => {
                let out = std::io::stdout();
                let mut lock = out.lock();
                let _ = writeln!(lock, "{s}");
                let _ = lock.flush();
            }
            Err(e) => {
                let out = std::io::stdout();
                let mut lock = out.lock();
                let _ = writeln!(lock, r#"{{"ok":false,"error":"序列化失败: {e}"}}"#);
                let _ = lock.flush();
            }
        }
    }
}

fn err_resp(msg: String) -> Response {
    Response {
        ok: false,
        lines: None,
        image_width: 0,
        image_height: 0,
        error: Some(msg),
    }
}

fn print_json(resp: &Response) {
    use std::io::Write as _;
    let s = match serde_json::to_string(resp) {
        Ok(s) => s,
        Err(e) => format!(r#"{{"ok":false,"error":"序列化失败: {e}"}}"#),
    };
    let out = std::io::stdout();
    let mut lock = out.lock();
    let _ = writeln!(lock, "{s}");
    let _ = lock.flush();
}

/// 三个候选目录中第一个"三件套齐全"的；都没有则返回注册名（auto-download 回退）
fn resolve_models() -> (String, String, String) {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Ok(d) = std::env::var("SEARCH_TOOL_OCR_MODEL_DIR") {
        dirs.push(PathBuf::from(d));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(p) = exe.parent() {
            dirs.push(p.join("models"));
            dirs.push(p.join("..").join("lib").join("search-tool").join("models"));
            dirs.push(p.join("..").join("share").join("search-tool").join("models"));
        }
    }
    dirs.push(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("third_party")
            .join("ocr-models"),
    );

    for d in dirs {
        let (a, b, c) = (d.join(DET_MODEL), d.join(REC_MODEL), d.join(DICT_FILE));
        if a.is_file() && b.is_file() && c.is_file() {
            return (
                a.to_string_lossy().into_owned(),
                b.to_string_lossy().into_owned(),
                c.to_string_lossy().into_owned(),
            );
        }
    }
    (
        DET_MODEL.to_string(),
        REC_MODEL.to_string(),
        DICT_FILE.to_string(),
    )
}

fn run(req: &Request) -> Result<(Vec<Line>, u32, u32)> {
    let path = PathBuf::from(&req.image_path);
    if !path.is_file() {
        return Err(anyhow!("图像不存在: {}", path.display()));
    }

    let (det, rec, dict) = resolve_models();
    let ocr = OAROCRBuilder::new(det, rec, dict).build()?;

    let image = load_image(Path::new(&req.image_path))?;
    let (iw, ih) = (image.width(), image.height());
    let results = ocr.predict(vec![image])?;

    let mut lines = Vec::new();
    for page in &results {
        for region in &page.text_regions {
            let Some((text, confidence)) = region.text_with_confidence() else {
                continue;
            };
            if text.trim().is_empty() {
                continue;
            }
            let bb = &region.bounding_box;
            let (x, y) = (bb.x_min(), bb.y_min());
            lines.push(Line {
                text: text.to_string(),
                confidence,
                bbox: [x, y, bb.x_max() - x, bb.y_max() - y],
            });
        }
    }
    Ok((lines, iw, ih))
}
