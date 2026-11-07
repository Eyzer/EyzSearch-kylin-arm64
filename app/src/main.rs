//! eyzsearch 入口：Tauri GUI + headless CLI 双模式
//!
//! CLI 模式让软件在无 GUI 环境（服务器/CI/冒烟测试）完整可运行：
//!   eyzsearch cli index  --dir <目录> [--index-dir <路径>]
//!   eyzsearch cli search <查询> --mode fuzzy|exact|dsl [--reverse] [--limit N]
//!   eyzsearch cli stats
//!   eyzsearch cli testdata  # 生成演示语料并索引（冒烟用）
//!
//! 错误处理口径（P2 修复）：CLI 失败一律 `eprintln` + `exit(1)`，
//! 不再裸 `unwrap`/`expect`（避免 panic 栈 + 退出码不统一）。

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "eyzsearch", about = "EyzSearch —— 本地全文搜索引擎")]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
    /// 启动 GUI 并把该目录作为初始过滤条件（右击搜索入口，P5）
    #[arg(long, value_name = "DIR")]
    open_dir: Option<String>,
    /// CLI /s 关键字（atgui /s 关键字 [/d 盘符] [/e 后缀]；
    /// argv 以 `/` 开头时由 preprocess_slash_args 映射而来）
    #[arg(long, value_name = "KW")]
    search: Option<String>,
    /// CLI /d 盘符（如 C 或 C:\）——搜索范围限定该盘
    #[arg(long, value_name = "D")]
    drive: Option<String>,
    /// CLI /e 扩展名（如 txt）——只保留该类型结果
    #[arg(long, value_name = "E")]
    ext: Option<String>,
}

#[derive(Subcommand)]
enum Cmd {
    /// headless 模式（GUI 启动时省略子命令）
    Cli {
        #[command(subcommand)]
        sub: CliSub,
    },
}

#[derive(Subcommand)]
enum CliSub {
    /// 全量索引一个目录
    Index {
        #[arg(long)]
        dir: String,
        #[arg(long)]
        index_dir: Option<String>,
        /// 忽略清单强制全量重建：写影子索引，完成后整体替换（旧索引全程可查）
        #[arg(long)]
        full: bool,
    },
    /// 搜索
    Search {
        query: String,
        #[arg(long, default_value = "fuzzy")]
        mode: String,
        #[arg(long)]
        reverse: bool,
        #[arg(long, default_value = "20")]
        limit: usize,
        #[arg(long)]
        index_dir: Option<String>,
    },
    /// 索引统计
    Stats {
        #[arg(long)]
        index_dir: Option<String>,
    },
    /// 生成演示语料并索引（冒烟验收）
    Testdata {
        #[arg(long)]
        index_dir: Option<String>,
    },
}

fn main() {
    //`eyzsearch /s 关键字 [/d 盘符] [/e 后缀]` 风格
    // 仅当 argv[1] 以 '/' 开头才启用映射，不影响 `cli ...` / `--open-dir` 等正常入口
    let argv: Vec<String> = preprocess_slash_args(&std::env::args().collect::<Vec<_>>());
    let cli = Cli::parse_from(argv);

    if let Some(kw) = cli.search.clone() {
        run_slash_search(kw, cli.drive.clone(), cli.ext.clone());
        return;
    }

    match cli.cmd {
        Some(Cmd::Cli { sub }) => run_cli(sub),
        None => search_tool_lib::run(cli.open_dir),
    }
}

/// argv 预处理：`/s`→`--search`、`/d`→`--drive`、`/e`→`--ext`（大小写不敏感）。
///
/// 仅当 `argv[1]` 以 '/' 开头才启用该模式（避免影响正常子命令）；
/// 模式内只映射**整 token 精确等于** /s、/d、/e 的项，其余（含查询词本身、
/// `/usr/share` 之类路径）原样保留。
fn preprocess_slash_args(argv: &[String]) -> Vec<String> {
    if argv.len() < 2 || !argv[1].starts_with('/') {
        return argv.to_vec();
    }
    argv.iter()
        .map(|a| match a.to_lowercase().as_str() {
            "/s" => "--search".to_string(),
            "/d" => "--drive".to_string(),
            "/e" => "--ext".to_string(),
            _ => a.clone(),
        })
        .collect()
}

/// 盘符 → 目录前缀（`SearchFilter.dir` 用）："C" → "C:/"；已带 `:`/`/` 的原样保留。
fn drive_to_dir_prefix(d: &str) -> String {
    let s = d.trim().trim_end_matches(['/', '\\']);
    if s.chars().count() == 1 {
        format!("{s}:/")
    } else {
        s.to_string()
    }
}

/// CLI 标准输出（忽略 BrokenPipe）：`| head` 等下游提前关闭管道时
/// `println!` 会 panic（`failed printing to stdout`），改用忽略错误的写出
fn out(line: std::fmt::Arguments) {
    use std::io::Write;
    let _ = writeln!(std::io::stdout(), "{line}");
}

/// 安全 println（见 [`out`]）
macro_rules! cli_out {
    ($($arg:tt)*) => {
        out(format_args!($($arg)*))
    };
}

/// CLI 错误统一出口：打印上下文后以退出码 1 结束（不再 panic 裸栈）
fn die<T, E: std::fmt::Display>(r: Result<T, E>, ctx: &str) -> T {
    match r {
        Ok(v) => v,
        Err(e) => {
            eprintln!("错误：{ctx}: {e}");
            std::process::exit(1);
        }
    }
}

/// CLI `/s` 搜索：主实例在 → socket 转发呈现后退出 0；
/// 主实例不在 → 回退本地 headless（复用 cli search 逻辑 + drive/ext 过滤）。
fn run_slash_search(query: String, drive: Option<String>, ext: Option<String>) {
    use stcore::index::IndexManager;
    use stcore::search::{search, SearchFilter, SearchMode, SortKey};
    use stcore::service::SearchService as _;

    let ext = ext
        .map(|e| e.trim_start_matches('.').to_lowercase())
        .filter(|e| !e.is_empty());
    let dir = drive
        .as_deref()
        .map(drive_to_dir_prefix)
        .filter(|d| !d.is_empty());

    // 1) 尝试单实例转发（主实例在 GUI 呈现结果）
    //    transport 按平台：Unix socket 文件 / Windows TCP 端口文件（shell.rs）
    let index_dir = search_tool_lib::state::default_index_dir();
    #[cfg(unix)]
    let client = Some(stcore::service::SocketClient::new(
        search_tool_lib::commands::shell::socket_path(&index_dir),
    ));
    #[cfg(windows)]
    let client = search_tool_lib::commands::shell::read_instance_port(&index_dir)
        .map(stcore::service::SocketClient::new_tcp);
    let req = stcore::service::ServiceSearchRequest {
        query: query.clone(),
        dir: dir.clone(),
        ext: ext.clone(),
        ..Default::default()
    };
    match client {
        Some(client) => match client.search(req) {
            Ok(_) => {
                cli_out!("已在主窗口呈现结果");
                return;
            }
            Err(e) => eprintln!("主实例未在运行（{e}），回退本地搜索"),
        },
        None => eprintln!("主实例未在运行（无端口文件），回退本地搜索"),
    }

    // 2) 本地 headless 回退
    let mgr = die(IndexManager::open(&index_dir), "打开索引失败");
    let filter = SearchFilter {
        dir,
        exts: ext.map(|e| vec![e]),
        sort: SortKey::Relevance,
    };
    let started = std::time::Instant::now();
    let hits = die(
        search(&mgr, &query, SearchMode::Fuzzy, false, 20, &filter),
        "搜索失败",
    );
    let elapsed = started.elapsed();
    cli_out!(
        "返回 {} 个结果，耗时 {:.3} 秒，搜索完成",
        hits.len(),
        elapsed.as_secs_f64()
    );
    for (i, h) in hits.iter().enumerate() {
        cli_out!("{:>3}. [{} 次命中] {}", i + 1, h.hits, h.path);
        if !h.snippet.is_empty() {
            cli_out!("      {}", h.snippet.replace('\n', " "));
        }
    }
}

fn run_cli(sub: CliSub) {
    use stcore::index::IndexManager;
    use stcore::search::{search, SearchMode};

    let resolve_index_dir = |override_dir: &Option<String>| {
        override_dir
            .clone()
            .map(std::path::PathBuf::from)
            .unwrap_or_else(search_tool_lib::state::default_index_dir)
    };

    match sub {
        CliSub::Index {
            dir,
            index_dir,
            full,
        } => {
            let index_dir = resolve_index_dir(&index_dir);
            cli_out!("索引目录: {} → {}", dir, index_dir.display());
            let progress = |done: usize, total: usize| {
                if done == total || done % 100 == 0 {
                    cli_out!("  进度: {}/{}", done, total);
                }
            };
            let stats = if full {
                let cfg =
                    stcore::config::Config::load(&index_dir).unwrap_or_default();
                cli_out!("模式: 全量重建（影子索引，完成后整体替换）");
                die(
                    search_tool_lib::indexer::index_directory_mode(
                        std::path::Path::new(&dir),
                        &index_dir,
                        &stcore::config::RuleSet::empty(),
                        &cfg,
                        search_tool_lib::indexer::IndexMode::Full,
                        None,
                        |p: search_tool_lib::indexer::ScanProgress| {
                            progress(p.done, p.total)
                        },
                    ),
                    "索引失败",
                )
            } else {
                die(
                    search_tool_lib::indexer::index_directory(
                        std::path::Path::new(&dir),
                        &index_dir,
                        progress,
                    ),
                    "索引失败",
                )
            };
            cli_out!(
                "完成: 扫描 {} 个文件, 索引 {} 篇文档",
                stats.scanned,
                stats.indexed
            );
        }
        CliSub::Search {
            query,
            mode,
            reverse,
            limit,
            index_dir,
        } => {
            let index_dir = resolve_index_dir(&index_dir);
            let mode = match mode.as_str() {
                "fuzzy" => SearchMode::Fuzzy,
                "exact" => SearchMode::Exact,
                "dsl" => SearchMode::Dsl,
                other => {
                    eprintln!("未知模式: {}（可选 fuzzy/exact/dsl）", other);
                    std::process::exit(2);
                }
            };
            let mgr = die(IndexManager::open(&index_dir), "打开索引失败");
            let started = std::time::Instant::now();
            let filter = stcore::search::SearchFilter::default();
            let hits = die(
                search(&mgr, &query, mode, reverse, limit, &filter),
                "搜索失败",
            );
            let elapsed = started.elapsed();
            cli_out!(
                "返回 {} 个结果，耗时 {:.3} 秒，搜索完成",
                hits.len(),
                elapsed.as_secs_f64()
            );
            for (i, h) in hits.iter().enumerate() {
                cli_out!("{:>3}. [{} 次命中] {}", i + 1, h.hits, h.path);
                if !h.snippet.is_empty() {
                    cli_out!("      {}", h.snippet.replace('\n', " "));
                }
            }
        }
        CliSub::Stats { index_dir } => {
            let index_dir = resolve_index_dir(&index_dir);
            let mgr = die(IndexManager::open(&index_dir), "打开索引失败");
            let size = dir_size(&index_dir);
            cli_out!("索引 {} 篇文档，磁盘占用 {}", mgr.num_docs(), human(size));
        }
        CliSub::Testdata { index_dir } => {
            let index_dir = resolve_index_dir(&index_dir);
            let demo = std::env::temp_dir().join("eyzsearch-demo");
            let _ = std::fs::remove_dir_all(&demo);
            die(std::fs::create_dir_all(&demo), "创建演示目录失败");
            let samples = [
                ("中华人民共和国简史.txt", "中华人民共和国 成立于 一九四九年。人民 生活 水平 不断 提高，中华 文化 博大 精 深。"),
                ("财经新闻.txt", "人民币 汇率 保持 基本 稳定，外贸 进出口 总额 持续 增长。朝鲜 共和国 的 外交 代表团 访问 中国。"),
                ("生活随笔.md", "# 柴米油盐\n\n人民 的 生活 日常 离 不 开 柴 米 油 盐。\n\n中华烟 与 人民好 的 朝鲜共和国 见闻。"),
                ("科技动态.html", "<html><body><h1>量子计算</h1><p>人工 智能 大模型 引发 行业 变革，算力 需求 快速 增长。</p><script>var x=1;</script></body></html>"),
                ("邮件存档.eml", "From: a@b.c\nSubject: 会议纪要\nContent-Type: text/plain; charset=utf-8\n\n下周 讨论 数字 货币 试点 范围 扩大 事宜。"),
            ];
            for (name, body) in samples {
                die(std::fs::write(demo.join(name), body), "写入演示文件失败");
            }
            // 造一个 zip（内含 2 个文本）
            let zip_path = demo.join("资料打包.zip");
            {
                let f = die(std::fs::File::create(&zip_path), "创建 zip 失败");
                let mut z = zip::ZipWriter::new(f);
                let opts = zip::write::SimpleFileOptions::default();
                die(z.start_file("说明.txt", opts), "写 zip 条目失败");
                die(
                    std::io::Write::write_all(
                        &mut z,
                        "这是 压缩包 内 的 说明 文件：搜索 引擎 改变 了 人 们 获取 信息 的 方式。"
                            .as_bytes(),
                    ),
                    "写 zip 内容失败",
                );
                die(z.start_file("笔记.md", opts), "写 zip 条目失败");
                die(
                    std::io::Write::write_all(
                        &mut z,
                        "# 笔记\n\n区块 链 技术 应用 于 供应链 金融。".as_bytes(),
                    ),
                    "写 zip 内容失败",
                );
                die(z.finish(), "收尾 zip 失败");
            }
            cli_out!("演示语料: {}", demo.display());
            let stats = die(
                search_tool_lib::indexer::index_directory(&demo, &index_dir, |_, _| {}),
                "索引失败",
            );
            cli_out!(
                "完成: 扫描 {} 个文件, 索引 {} 篇文档（含压缩包内层）",
                stats.scanned,
                stats.indexed
            );
            cli_out!("\n试试: eyzsearch cli search 中华人民共和国 --mode fuzzy");
        }
    }
}

fn dir_size(p: &std::path::Path) -> u64 {
    std::fs::read_dir(p)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| {
                    let path = e.path();
                    if path.is_dir() {
                        dir_size(&path)
                    } else {
                        e.metadata().map(|m| m.len()).unwrap_or(0)
                    }
                })
                .sum()
        })
        .unwrap_or(0)
}

fn human(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    format!("{:.1}{}", size, UNITS[unit])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn slash_mode_maps_flags() {
        let got = preprocess_slash_args(&v(&[
            "eyzsearch",
            "/s",
            "量子计算",
            "/d",
            "C",
            "/e",
            "txt",
        ]));
        assert_eq!(
            got,
            v(&[
                "eyzsearch",
                "--search",
                "量子计算",
                "--drive",
                "C",
                "--ext",
                "txt"
            ])
        );
    }

    #[test]
    fn slash_mode_case_insensitive() {
        let got = preprocess_slash_args(&v(&["eyzsearch", "/S", "kw", "/D", "C:/", "/E", "md"]));
        assert_eq!(
            got,
            v(&[
                "eyzsearch",
                "--search",
                "kw",
                "--drive",
                "C:/",
                "--ext",
                "md"
            ])
        );
    }

    #[test]
    fn slash_mode_keeps_non_flag_tokens() {
        // 查询词本身以 / 开头（路径）不被误映射
        let got = preprocess_slash_args(&v(&["eyzsearch", "/s", "/usr/share", "/e", "log"]));
        assert_eq!(
            got,
            v(&["eyzsearch", "--search", "/usr/share", "--ext", "log"])
        );
    }

    #[test]
    fn normal_subcommand_untouched() {
        // argv[1] 非 '/' 开头：整组参数原样（含组内 /s）
        let args = v(&["eyzsearch", "cli", "search", "/s"]);
        assert_eq!(preprocess_slash_args(&args), args);
    }

    #[test]
    fn single_arg_untouched() {
        let args = v(&["eyzsearch"]);
        assert_eq!(preprocess_slash_args(&args), args);
    }

    #[test]
    fn drive_prefix_normalization() {
        assert_eq!(drive_to_dir_prefix("C"), "C:/");
        assert_eq!(drive_to_dir_prefix("c:\\"), "c:");
        assert_eq!(drive_to_dir_prefix("C:/"), "C:");
        assert_eq!(drive_to_dir_prefix(" /d "), "/d");
        assert_eq!(drive_to_dir_prefix(""), "");
    }
}
