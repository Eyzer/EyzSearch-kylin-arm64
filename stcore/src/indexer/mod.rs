//! 扫描-提取-索引管线（架构 v2 R2：引擎入 stcore，宿主只留接线）
//!
//! 原 `app/src/scanner.rs` 迁入（D3：引擎与宿主分离，GUI/CLI 共用同一引擎）：
//!
//! 子项 3 接线（R10 单一事实源）：
//!   - 目录三态规则经 `RuleSet::allows` 接入 `filter_entry`，与查询层共用同一判定函数；
//!   - 受支持扩展名由 `Config::all_exts()`（config.rs 文件类型表）派生，不再硬编码；
//!   - 取消 / 暂停经 `ScanControl`（进程级原子标志）接入，取消后**正常 commit 已写入的部分**，
//!     保证索引不损坏、可续。
//!
//! 线程模型（R1）：
//!   - 提取走**专用 rayon 池**（N-1 限流），不再用全局默认池，避免与 UI 抢满核；
//!   - 写入走 `index::Writer`（写侧独占 IndexWriter），与读侧 IndexManager 分离。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

use ignore::WalkBuilder;
use rayon::prelude::*;

use crate::config::{Config, RuleSet};
use crate::extract;
use crate::index::{IndexDoc, Manifest, Writer};

/// 支持的压缩包扩展名（递归展开内层文本）。
///
/// 与 config 内置"压缩包"类型重叠，此处显式并集保留压缩链路语义：
/// 即使类型表被调整，压缩包内层展开行为不变。
pub const ARCHIVE_EXTS: &[&str] = &["zip", "tar", "gz", "tgz"];

/// 路径扩展名（小写）是否在预计算的受支持集合中
fn ext_allowed(path: &Path, exts: &[String]) -> bool {
    let Some(ext) = path.extension().map(|e| e.to_string_lossy().to_lowercase()) else {
        return false;
    };
    exts.iter().any(|s| s == &ext)
}

/// 从配置派生受支持扩展名（R10 单一事实源：`config.rs` 文件类型表 + 索引类型范围）。
///
/// 判据是 `Config::index_exts()`：默认只含「常用办公文档」，用户在
/// 「索引管理 → 索引构建」勾选更多类型后自动放宽。
/// 归一化：去点前缀、转小写、去重、排序。
pub fn supported_exts(cfg: &Config) -> Vec<String> {
    let mut v: Vec<String> = cfg
        .index_exts()
        .into_iter()
        .map(|e| e.trim_start_matches('.').to_lowercase())
        .filter(|e| !e.is_empty())
        .collect();
    v.sort();
    v.dedup();
    v
}

/// 路径是否为受支持类型（小写扩展名 ∈ 索引类型派生集合）
pub fn is_supported(path: &Path, cfg: &Config) -> bool {
    let Some(ext) = path.extension().map(|e| e.to_string_lossy().to_lowercase()) else {
        return false;
    };
    cfg.index_exts().iter().any(|e| e == &ext)
}

/// 单文件大小上限（P0：50MB；与提取层共用常量，单一真源）
pub use crate::extract::MAX_FILE_SIZE;

/// 默认排除目录名（与 ripgrep 惯例一致，按 `file_name` 精确匹配）。
///
/// 注意：以 `.` 开头的隐藏目录（`.git` / `.cache` / `.local` / `.mozilla` …）
/// 已由 `WalkBuilder::hidden(true)` 整体跳过，不必在此重复列举。
/// 这里只列**非隐藏**的、确定不是文档的地方：
/// 依赖目录、构建产物、Python 缓存、回收站、快照、lost+found。
const DEFAULT_EXCLUDES: &[&str] = &[
    "node_modules",
    "target",
    "__pycache__",
    "venv",
    "site-packages",
    "Trash",
    "lost+found",
    "snap",
];

/// 索引模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexMode {
    /// 增量：按 size+mtime 跳过未变更文件，并清理磁盘上已消失的记录
    Incremental,
    /// 全量：不看清单，所有候选文件重新提取
    Full,
}

/// 候选文件（遍历阶段一次 stat 拿到全部指纹，后续不再重复 stat）
#[derive(Debug, Clone)]
struct Candidate {
    path: PathBuf,
    size: u64,
    mtime: i64,
}

/// 单批上限：文件数 500 或原始字节 128MB（先到者切批）。
///
/// 分两重限制是因为两种极端都要防：500 个 25MB 的 PDF 会吃掉十几 GB 内存；
/// 而纯小文件时按字节切会让 commit 过于频繁（每次 commit 有固定开销）。
const BATCH_MAX_FILES: usize = 500;
const BATCH_MAX_BYTES: u64 = 128 * 1024 * 1024;

/// 把候选列表切成若干批（每批独立「并行提取 → 串行写入 → commit」）
fn plan_batches(items: &[Candidate]) -> Vec<&[Candidate]> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut bytes = 0u64;
    for (i, c) in items.iter().enumerate() {
        bytes = bytes.saturating_add(c.size);
        if i - start + 1 >= BATCH_MAX_FILES || bytes >= BATCH_MAX_BYTES {
            out.push(&items[start..=i]);
            start = i + 1;
            bytes = 0;
        }
    }
    if start < items.len() {
        out.push(&items[start..]);
    }
    out
}

/// 暂停时轮询粒度
const PAUSE_POLL: Duration = Duration::from_millis(50);

/* ------------------------------ 提取线程池（N-1 限流） ------------------------------ */

/// 专用提取池：`available_parallelism() - 1`（至少 1），给 UI 线程留一核。
///
/// 不用 rayon 全局默认池：GUI 进程里搜索/预览等其它 rayon 用户会与本管线抢满核。
static EXTRACT_POOL: OnceLock<rayon::ThreadPool> = OnceLock::new();

fn extract_pool() -> &'static rayon::ThreadPool {
    EXTRACT_POOL.get_or_init(|| {
        let cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        rayon::ThreadPoolBuilder::new()
            .num_threads(cores.saturating_sub(1).max(1))
            .thread_name(|i| format!("st-extract-{i}"))
            .build()
            .expect("构建提取线程池失败")
    })
}

/// 扫描进度（供 `index-progress` 事件与 CLI 复用）
#[derive(Debug, Clone)]
pub struct ScanProgress {
    pub done: usize,
    pub total: usize,
    /// 当前正在处理的文件路径（重建 UI 显示用）
    pub current: String,
}

/// 取消 / 暂停控制（进程级共享；由 index_admin 的 rebuild 使用）
#[derive(Debug, Default)]
pub struct ScanControl {
    cancel: AtomicBool,
    paused: AtomicBool,
}

impl ScanControl {
    pub fn new() -> Self {
        Self::default()
    }

    /// 请求取消：不再启动新文件，已提取部分正常入库
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    /// 清除取消标志（开始新一轮重建前调用）
    pub fn reset(&self) {
        self.cancel.store(false, Ordering::SeqCst);
        self.paused.store(false, Ordering::SeqCst);
    }

    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::SeqCst);
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    /// 阻塞直至恢复或被取消；返回 false 表示期间收到取消
    fn wait_if_paused(&self) -> bool {
        while self.is_paused() {
            if self.is_cancelled() {
                return false;
            }
            std::thread::sleep(PAUSE_POLL);
        }
        !self.is_cancelled()
    }
}

pub struct ScanStats {
    pub scanned: usize,
    pub indexed: usize,
    pub skipped: usize,
    pub failed: usize,
    /// 因 size+mtime 未变而跳过的文件数（增量模式的核心收益）
    pub unchanged: usize,
    /// 磁盘上已消失、本次从索引中清除的记录数
    pub removed: usize,
    /// 是否因取消而提前结束（已写入部分已 commit）
    pub cancelled: bool,
}

/// 全量索引一个目录（并行提取 → 串行写索引）——旧签名薄封装，保留 CLI（main.rs）兼容。
///
/// 扩展名集合从 `index_dir` 的 config.json 派生（文件不存在则用内置默认）。
pub fn index_directory(
    root: &Path,
    index_dir: &Path,
    on_progress: impl Fn(usize, usize) + Sync,
) -> anyhow::Result<ScanStats> {
    let cfg = Config::load(index_dir).unwrap_or_default();
    index_directory_controlled(
        root,
        index_dir,
        &RuleSet::empty(),
        &cfg,
        None,
        |p: ScanProgress| on_progress(p.done, p.total),
    )
}

/// 带目录三态规则的全量索引（GUI「索引目录」入口；任务卡 3.2）
///
/// 扩展名集合同样从 `index_dir` 的 config.json 派生（单一事实源）。
pub fn index_directory_with_rules(
    root: &Path,
    index_dir: &Path,
    rules: &RuleSet,
    on_progress: impl Fn(usize, usize) + Sync,
) -> anyhow::Result<ScanStats> {
    let cfg = Config::load(index_dir).unwrap_or_default();
    index_directory_controlled(root, index_dir, rules, &cfg, None, |p: ScanProgress| {
        on_progress(p.done, p.total)
    })
}

/// 全功能入口：规则过滤 + 取消/暂停 + 带当前文件名的进度回调。
///
/// `cfg` 提供受支持扩展名（R10：与查询层共用 config 文件类型表）。
///
/// 取消语义：命中取消后 `filter_map` 跳过剩余文件，随后照常 `commit` 已提取的文档，
/// 返回 `cancelled = true`。写入端幂等（按 path 先删后建），故可续跑。
pub fn index_directory_controlled(
    root: &Path,
    index_dir: &Path,
    rules: &RuleSet,
    cfg: &Config,
    ctrl: Option<&ScanControl>,
    on_progress: impl Fn(ScanProgress) + Sync,
) -> anyhow::Result<ScanStats> {
    index_directory_mode(
        root,
        index_dir,
        rules,
        cfg,
        IndexMode::Incremental,
        ctrl,
        on_progress,
    )
}

/// 带索引模式的入口（增量 / 全量）。
///
/// 增量模式下：磁盘上 size+mtime 与清单一致的文件**不进提取队列**，
/// 磁盘上已消失的记录从索引删除。日常重建因此只需处理变更部分。
pub fn index_directory_mode(
    root: &Path,
    index_dir: &Path,
    rules: &RuleSet,
    cfg: &Config,
    mode: IndexMode,
    ctrl: Option<&ScanControl>,
    on_progress: impl Fn(ScanProgress) + Sync,
) -> anyhow::Result<ScanStats> {
    index_directory_mode_at(
        root,
        index_dir,
        None,
        rules,
        cfg,
        mode,
        ctrl,
        on_progress,
    )
}

/// 带「输出目录」的入口：全量重建写影子索引时用。
///
/// `write_dir = Some(d)` 时，本次写入落到 `d`，**不**自动替换主目录；
/// 由调用方在全部根目录写完后统一 `promote_shadow`。
/// 这样多根目录重建才共用同一个影子索引，而不是每个根各清一次。
pub fn index_directory_mode_at(
    root: &Path,
    index_dir: &Path,
    write_dir: Option<&Path>,
    rules: &RuleSet,
    cfg: &Config,
    mode: IndexMode,
    ctrl: Option<&ScanControl>,
    on_progress: impl Fn(ScanProgress) + Sync,
) -> anyhow::Result<ScanStats> {
    // 写侧独占 writer（tantivy 目录锁保证单实例；读侧 IndexManager 不受影响）
    //
    // 全量模式写影子目录：旧索引在重建全程保持可查，写完才整体替换。
    // 增量模式直接写主目录——它本来就是原地更新，没有「清空期」。
    let owned_shadow = if write_dir.is_none() && mode == IndexMode::Full {
        Some(crate::index::reset_shadow(index_dir)?)
    } else {
        None
    };
    let target: &Path = write_dir
        .or(owned_shadow.as_deref())
        .unwrap_or(index_dir);
    let mut writer = Writer::open(target)?;

    // 提取缓存：命中即跳过解析（PDF / Office / OCR 是重建的真实瓶颈）。
    // 放在主目录而非影子目录——`reset_index_data` 会保留 `*.db`，重建不丢。
    let cache = crate::cache::ExtractCache::open(index_dir);

    // 受支持扩展名预计算一次（避免遍历中逐文件派生）
    let exts = supported_exts(cfg);

    // 1) 收集候选文件：一次 stat 拿全 size+mtime，提取阶段不再重复 stat。
    //    filter_entry 同时应用默认排除 + 三态规则（规则集 clone 进闭包，
    //    GlobMatcher 内部为 Arc，克隆廉价）。
    let rules_owned = rules.clone();
    let existed: Vec<Candidate> = WalkBuilder::new(root)
        .hidden(true) // 跳过全部「.」开头的隐藏项（.git / .cache / .local / .mozilla …）
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .filter_entry(move |e| {
            !DEFAULT_EXCLUDES
                .iter()
                .any(|x| e.file_name().to_string_lossy() == *x)
                && rules_owned.allows(e.path())
        })
        .build()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            let size = meta.len();
            if size > MAX_FILE_SIZE || !ext_allowed(e.path(), &exts) {
                return None;
            }
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            Some(Candidate {
                path: e.into_path(),
                size,
                mtime,
            })
        })
        .collect();

    // 2) 与增量清单比对
    let mut unchanged = 0usize;
    let mut removed = 0usize;
    let todo: Vec<Candidate> = match mode {
        IndexMode::Full => existed,
        IndexMode::Incremental => {
            let known = Manifest::load(index_dir);
            // 2a) 清理磁盘上已消失的记录：只处理本 root 下的真实文件路径。
            //     压缩包内层条目形如 `a.zip#entry`，不对应真实路径，跳过。
            for path in known.docs.keys() {
                if path.contains('#') {
                    continue;
                }
                let p = Path::new(path);
                if !p.starts_with(root) || p.exists() {
                    continue;
                }
                writer.delete(path)?;
                removed += 1;
            }
            // 2b) size 与 mtime 都没变的文件，不再提取
            let mut todo = Vec::with_capacity(existed.len());
            for c in existed {
                let key = c.path.to_string_lossy();
                if known.unchanged(key.as_ref(), c.size, c.mtime) {
                    unchanged += 1;
                } else {
                    todo.push(c);
                }
            }
            todo
        }
    };

    let total = todo.len();
    let done = AtomicUsize::new(0);
    let mut indexed = 0usize;

    // 全量模式（或本次既无变更、也无删除）时，索引里不可能存在同 path 的旧文档，
    // 逐篇 delete_term 只会往段里写一堆删除位、commit 时还要扫一遍。跳过。
    writer.set_skip_delete_term(mode == IndexMode::Full || (unchanged == 0 && removed == 0));

    // 3) 分批：并行提取 → 串行写入 → commit。
    //    单批封顶（500 个 / 128MB），内存峰值可控，且进度能随 commit 稳定推进。
    for batch in plan_batches(&todo) {
        let docs: Vec<(IndexDoc, String)> = extract_pool().install(|| {
            batch
                .par_iter()
                .filter_map(|c| {
                    if let Some(ctl) = ctrl {
                        if ctl.is_cancelled() || !ctl.wait_if_paused() {
                            return None;
                        }
                    }
                    let docs = extract_file(c, cache.as_ref());
                    let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                    on_progress(ScanProgress {
                        done: n,
                        total,
                        current: c.path.to_string_lossy().to_string(),
                    });
                    docs
                })
                .flatten()
                .collect()
        });

        for (doc, py) in &docs {
            writer.upsert_with_pinyin(doc, py)?;
        }
        indexed += docs.len();
        // 每批一次 commit：取消/崩溃时已写入的部分是完整可用的
        writer.commit()?;
    }

    // 4) 本批清单为空但有待删除记录时，仍需落盘删除与清单变更
    if todo.is_empty() && removed > 0 {
        writer.commit()?;
    }

    let cancelled = ctrl.map(|c| c.is_cancelled()).unwrap_or(false);

    // 提取缓存效果（每次索引都打，便于排障与验收）
    if let Some(c) = cache.as_ref() {
        let (hits, misses) = c.stats();
        if hits + misses > 0 {
            eprintln!("提取缓存：命中 {hits} / 未命中 {misses}（命中即跳过解析）");
        }
    }

    // 5) 全量模式：影子索引写完，整体替换主目录（毫秒级）。
    //    被取消就保留影子目录不替换，旧索引继续可用，下次全量重建会先清干净。
    //    调用方显式传了 write_dir 时由它负责替换（多根目录场景）。
    if mode == IndexMode::Full && !cancelled && write_dir.is_none() {
        drop(writer);
        crate::index::promote_shadow(index_dir)?;
    }

    let scanned = total + unchanged;
    Ok(ScanStats {
        scanned,
        indexed,
        skipped: scanned.saturating_sub(indexed),
        failed: 0,
        unchanged,
        removed,
        cancelled,
    })
}

/// 提取单个候选文件 → (索引文档, 拼音串)
///
/// 拼音在这里一并算好：本函数跑在提取线程池内（并行），
/// 而 `Writer` 是唯一的串行段，CPU 活不能放到那里。
///
/// 带缓存：`路径 + 大小 + 修改时间` 命中就直接返回上次的提取结果，
/// 完全跳过解析。这是全量重建能快一个数量级的关键——重建时文件根本没变，
/// 重新解析 PDF / Word / OCR 是纯粹的白工。
fn extract_file(
    c: &Candidate,
    cache: Option<&crate::cache::ExtractCache>,
) -> Option<Vec<(IndexDoc, String)>> {
    let key = c.path.to_string_lossy().to_string();

    if let Some(ca) = cache {
        if let Some(hit) = ca.get(&key, c.size, c.mtime) {
            return Some(
                hit.into_iter()
                    .map(|d| {
                        (
                            IndexDoc {
                                path: d.path,
                                title: d.title,
                                body: d.body,
                                size: d.size,
                                mtime: d.mtime,
                            },
                            d.pinyin,
                        )
                    })
                    .collect(),
            );
        }
    }

    let out = extract_file_uncached(c);

    // 空结果也缓存（提取不出文字的损坏文件），避免每次重建都白试一遍
    if let (Some(ca), Some(docs)) = (cache, out.as_ref()) {
        let cached: Vec<crate::cache::CachedDoc> = docs
            .iter()
            .map(|(d, py)| crate::cache::CachedDoc {
                path: d.path.clone(),
                title: d.title.clone(),
                body: d.body.clone(),
                size: d.size,
                mtime: d.mtime,
                pinyin: py.clone(),
            })
            .collect();
        ca.put(&key, c.size, c.mtime, &cached);
    }

    out
}

fn extract_file_uncached(c: &Candidate) -> Option<Vec<(IndexDoc, String)>> {
    use crate::extract::ExtractError;

    match extract::extract_path(&c.path) {
        Ok(r) => {
            let mut docs = Vec::new();
            let title = c
                .path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            if !r.text.is_empty() {
                docs.push(make_doc(
                    c.path.to_string_lossy().to_string(),
                    title,
                    r.text,
                    c.size,
                    c.mtime,
                ));
            }
            for child in r.children {
                if child.text.is_empty() {
                    continue;
                }
                // 先取长度再 move，避免 borrow-after-move
                let body_len = child.text.len() as u64;
                docs.push(make_doc(
                    format!("{}#{}", c.path.to_string_lossy(), child.name),
                    child.name,
                    child.text,
                    body_len,
                    c.mtime,
                ));
            }
            Some(docs)
        }
        Err(ExtractError::Unsupported(_)) => None,
        Err(e) => {
            // 可观测性：提取/OCR 失败不静默丢弃（曾因 `Err(_) => None`
            // 致 OCR 竞态丢文档无任何线索）。Unsupported 属正常跳过，不打日志。
            eprintln!("提取失败，跳过 {}: {e}", c.path.display());
            None
        }
    }
}

/// 构造索引文档 + 拼音串。
///
/// 纯 ASCII 文本（英文 / 代码 / 日志）的拼音串就是原文小写化本身，
/// 而原文已由 body 字段索引——`text_to_pinyin_text` 会直接返回空串，整段转换免掉。
fn make_doc(
    path: String,
    title: String,
    body: String,
    size: u64,
    mtime: i64,
) -> (IndexDoc, String) {
    let py_title = crate::pinyin::text_to_pinyin_text(&title);
    let py_body = crate::pinyin::text_to_pinyin_text(&body);
    let pinyin = if py_title.is_empty() && py_body.is_empty() {
        String::new()
    } else {
        format!("{py_title} {py_body}")
    };
    (
        IndexDoc {
            path,
            title,
            body,
            size,
            mtime,
        },
        pinyin,
    )
}

/// 测试用临时目录（避免为单测给 stcore 引入 tempfile 主依赖）
#[cfg(test)]
pub(crate) mod testutil {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static SEQ: AtomicUsize = AtomicUsize::new(0);

    pub struct TempDir(PathBuf);

    impl TempDir {
        pub fn new(tag: &str) -> Self {
            let p = std::env::temp_dir().join(format!(
                "st-{}-{}-{}",
                tag,
                std::process::id(),
                SEQ.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Self(p)
        }

        pub fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// 写入文件（自动建父目录）
    pub fn write(dir: &Path, rel: &str, body: &str) {
        let p = dir.join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(p, body).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::{write, TempDir};
    use super::*;
    use crate::config::{Config, RuleState};

    #[test]
    fn rules_prune_excluded_subtree() {
        let src = TempDir::new("scan-src");
        let idx = TempDir::new("scan-idx");
        write(src.path(), "keep/a.txt", "hello 世界");
        write(src.path(), "dist/b.txt", "should be excluded 世界");

        let mut cfg = Config::default();
        cfg.set_rule(None, "**/dist/**", RuleState::Exclude);
        let rules = cfg.compile_rules(None).unwrap();

        let stats = index_directory_with_rules(src.path(), idx.path(), &rules, |_, _| {}).unwrap();
        assert_eq!(stats.indexed, 1, "dist/ 应被三态规则排除");
    }

    #[test]
    fn supported_exts_derived_from_config() {
        let cfg = Config::default();
        let exts = supported_exts(&cfg);
        // 默认索引类型 = 常用办公文档
        assert!(exts.iter().any(|e| e == "txt"), "常用办公文档应含 txt");
        assert!(exts.iter().any(|e| e == "md"));
        assert!(exts.iter().any(|e| e == "docx"));
        assert!(exts.iter().any(|e| e == "pdf"));
        // 未勾选的类型不进索引范围
        assert!(!exts.iter().any(|e| e == "rs"), "代码默认不索引");
        assert!(!exts.iter().any(|e| e == "zip"), "压缩包默认不索引");
        // 排序去重
        let mut sorted = exts.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(exts, sorted);

        // 用户新增类型即时进入受支持集合（单一事实源，无需改代码）
        let mut cfg2 = Config::default();
        cfg2.add_type("自定义", vec![".FOO".into(), "bar".into()]);
        let exts2 = supported_exts(&cfg2);
        assert!(exts2.iter().any(|e| e == "foo"), "归一化去点小写");
        assert!(exts2.iter().any(|e| e == "bar"));

        // 选择「全部类型」→ 收全部扩展名
        let mut cfg3 = Config::default();
        cfg3.set_index_types(Vec::new());
        let exts3 = supported_exts(&cfg3);
        assert!(exts3.iter().any(|e| e == "rs"));
        assert!(exts3.iter().any(|e| e == "zip"));
    }

    #[test]
    fn is_supported_uses_config_table() {
        let cfg = Config::default();
        assert!(is_supported(Path::new("/a/b.txt"), &cfg));
        assert!(is_supported(Path::new("/a/b.TXT"), &cfg), "大小写不敏感");
        assert!(is_supported(Path::new("/a/b.docx"), &cfg));
        assert!(
            !is_supported(Path::new("/a/包.zip"), &cfg),
            "压缩包默认不在索引类型内"
        );
        assert!(
            !is_supported(Path::new("/a/b.notsupportedext"), &cfg),
            "未配置扩展名不支持"
        );
        assert!(!is_supported(Path::new("/a/noext"), &cfg), "无扩展名不支持");

        let mut cfg2 = Config::default();
        cfg2.add_type("自定义", vec!["foo".into()]);
        assert!(
            is_supported(Path::new("/a/x.foo"), &cfg2),
            "新增类型即时生效"
        );
        assert!(!is_supported(Path::new("/a/x.foo"), &cfg));
    }

    #[test]
    fn cancel_commits_partial_and_reports_flag() {
        let src = TempDir::new("scan-src");
        let idx = TempDir::new("scan-idx");
        for i in 0..5 {
            write(src.path(), &format!("f{i}.txt"), "取消测试 世界");
        }
        let ctrl = ScanControl::new();
        ctrl.cancel(); // 一开始就取消 → 不启动任何文件，但不应报错
        let stats = index_directory_controlled(
            src.path(),
            idx.path(),
            &RuleSet::empty(),
            &Config::default(),
            Some(&ctrl),
            |_| {},
        )
        .unwrap();
        assert!(stats.cancelled);
        assert_eq!(stats.indexed, 0);
        // 索引目录仍可正常打开（未损坏）
        let mgr = crate::index::IndexManager::open(idx.path()).unwrap();
        assert_eq!(mgr.num_docs(), 0);
    }

    #[test]
    fn paused_flag_roundtrip() {
        let ctrl = ScanControl::new();
        assert!(!ctrl.is_paused());
        ctrl.set_paused(true);
        assert!(ctrl.is_paused());
        ctrl.set_paused(false);
        ctrl.cancel();
        ctrl.reset();
        assert!(!ctrl.is_cancelled());
    }
}
