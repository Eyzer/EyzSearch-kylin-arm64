//! tantivy 索引管理（生产版，架构 v2 R2：读/写分离）
//!
//! tantivy 0.26.2 API 注记（spike S8 实测）：
//!   - Tokenizer trait 为 GAT：type TokenStream<'a>，自实现 advance/token/token_mut
//!     后用 BoxTokenStream::new 装箱
//!   - TopDocs 须 .order_by_score() 才是 Collector
//!   - IndexRecordOption 三档：Basic / WithFreqs / WithFreqsAndPositions（多词查询需最后者）
//!
//! 引擎拆分（D3）：
//!   - [`Writer`]：写侧独占 `IndexWriter`（indexer 线程 / scanner / CLI 使用），
//!     同一目录同一时刻只有一个 writer（tantivy 目录锁保证）
//!   - [`IndexManager`]：读侧只读（搜索/预览/统计），不持有 writer，
//!     reader 以 `ReloadPolicy::OnCommitWithDelay` 构建（tantivy 0.26 无 `OnCommit`
//!     变体；该策略监听 meta.json，外部 writer commit 后毫秒级自动 reload）

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use tantivy::schema::{
    Field, IndexRecordOption, OwnedValue, Schema, Value as _, FAST, INDEXED, STORED, STRING, TEXT,
};
use tantivy::tokenizer::{BoxTokenStream, Token, TokenStream, Tokenizer};
use tantivy::{DocAddress, Index, IndexReader, IndexWriter, ReloadPolicy, TantivyDocument};

use crate::seg;

/// tantivy Tokenizer：包装 seg::segment_index（索引侧 n-gram）
#[derive(Clone)]
pub struct SegIndexTokenizer;

struct VecTokenStream {
    tokens: Vec<Token>,
    index: usize,
    current: Token,
}

impl TokenStream for VecTokenStream {
    fn advance(&mut self) -> bool {
        if self.index < self.tokens.len() {
            self.current = Token {
                offset_from: self.tokens[self.index].offset_from,
                offset_to: self.tokens[self.index].offset_to,
                position: self.tokens[self.index].position,
                text: self.tokens[self.index].text.clone(),
                position_length: 1,
            };
            self.index += 1;
            true
        } else {
            false
        }
    }
    fn token(&self) -> &Token {
        &self.current
    }
    fn token_mut(&mut self) -> &mut Token {
        &mut self.current
    }
}

impl Tokenizer for SegIndexTokenizer {
    type TokenStream<'a> = BoxTokenStream<'a>;

    fn token_stream<'a>(&'a mut self, text: &'a str) -> Self::TokenStream<'a> {
        let tokens: Vec<Token> = seg::segment_index(text)
            .into_iter()
            .enumerate()
            .map(|(pos, t)| Token {
                text: t,
                offset_from: 0,
                offset_to: 0,
                position: pos,
                position_length: 1,
            })
            .collect();
        if std::env::var_os("ST_SEG_DEBUG").is_some() {
            eprintln!(
                "[seg-debug] 输入 {} 字节 → {} token（首三个: {:?}）",
                text.len(),
                tokens.len(),
                tokens.iter().take(3).map(|t| t.text.as_str()).collect::<Vec<_>>()
            );
        }
        BoxTokenStream::new(VecTokenStream {
            tokens,
            index: 0,
            current: Token::default(),
        })
    }
}

/// 索引文档（提取层产出 → 索引）
#[derive(Debug, Clone)]
pub struct IndexDoc {
    pub path: String,
    pub title: String,
    pub body: String,
    pub size: u64,
    pub mtime: i64,
}

/// schema 版本号：索引结构（字段 / tokenizer / 语义）变更时递增。
/// open 时校验 meta.json 记录的版本，不匹配返回带指引的错误（不静默失败）。
///
/// 变更记录：
/// - v1：path/title/body/size/mtime/ext
/// - v2：新增 `pinyin` 字段（拼音检索通道，P3-C）
/// - v3：`size`/`mtime` 由 STORED 升级为 `INDEXED|FAST|STORED`
///   （RangeQuery 时间/大小筛选与排序必需，架构 v2 D7；上线需一次全量重建）
/// - v4：`pinyin` 字段索引选项由 `WithFreqsAndPositions` 降为 `WithFreqs`
///   （拼音只做 token AND，不存词位置；体积与写入时间显著下降。上线需一次全量重建）
pub const SCHEMA_VERSION: u32 = 4;

/// meta.json 中记录 schema 版本的键（tantivy 自身不识别该键，读时忽略）
const SCHEMA_VERSION_KEY: &str = "stcore_schema_version";

/// schema 各字段的 Field 句柄（读写两侧共用）
#[derive(Debug, Clone, Copy)]
struct SchemaFields {
    f_path: Field,
    f_title: Field,
    f_body: Field,
    f_pinyin: Field,
    f_size: Field,
    f_mtime: Field,
    f_ext: Field,
}

fn build_schema() -> Schema {
    let mut sb = Schema::builder();
    // 正文/标题：需要词位置（短语查询与命中定位）
    let with_positions = || {
        TEXT.set_indexing_options(
            tantivy::schema::TextFieldIndexing::default()
                .set_tokenizer("seg")
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        )
    };
    // 拼音：只做 token AND（`renmin` → `人民`），不需要词位置。
    // 拼音串长度约为正文 2～3 倍，不存 positions 能省下可观的索引体积与写入时间。
    let freqs_only = || {
        TEXT.set_indexing_options(
            tantivy::schema::TextFieldIndexing::default()
                .set_tokenizer("seg")
                .set_index_option(IndexRecordOption::WithFreqs),
        )
    };
    let text_stored = with_positions().set_stored();
    // 拼音字段只索引不存储：拼音串体积约为正文的数倍，存下来无检索价值
    let text_plain = freqs_only();
    sb.add_text_field("path", STRING | STORED);
    sb.add_text_field("title", text_stored.clone());
    sb.add_text_field("body", text_stored);
    sb.add_text_field("pinyin", text_plain);
    // v3（D7）：INDEXED（RangeQuery 过滤）+ FAST（按值排序）+ STORED（结果列展示）
    sb.add_i64_field("size", INDEXED | FAST | STORED);
    sb.add_i64_field("mtime", INDEXED | FAST | STORED);
    sb.add_text_field("ext", STRING | STORED);
    sb.build()
}

fn fields_of(index: &Index) -> anyhow::Result<SchemaFields> {
    Ok(SchemaFields {
        f_path: index.schema().get_field("path")?,
        f_title: index.schema().get_field("title")?,
        f_body: index.schema().get_field("body")?,
        f_pinyin: index.schema().get_field("pinyin")?,
        f_size: index.schema().get_field("size")?,
        f_mtime: index.schema().get_field("mtime")?,
        f_ext: index.schema().get_field("ext")?,
    })
}

/* ------------------------------- 增量清单 ------------------------------- */

/// 增量清单文件名（与 tantivy 的 meta.json 并列，放在索引目录根下）
pub const MANIFEST_FILE: &str = "stmanifest.json";

/// 单篇文档的变更指纹（size + mtime）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocMeta {
    pub size: i64,
    pub mtime: i64,
}

/// 增量清单：`path → (size, mtime)`
///
/// ## 为什么单独存一份而不是从索引里读
/// tantivy 的 stored 字段按行压缩，想拿到某篇文档的 path 必须解压整行（含 body）。
/// 「判断哪些文件没变」若逐文档读 stored，等于把所有正文再读一遍——和重新提取一样慢。
/// 所以另存一份轻量清单，让增量重建只碰这个小文件。
///
/// ## 失效策略
/// 清单里记着 schema 版本；与当前 `SCHEMA_VERSION` 不符时整体作废（退回全量重建），
/// 避免索引结构变更后拿着旧指纹跳过文件、导致新字段为空。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    #[serde(default)]
    pub schema: u32,
    #[serde(default)]
    pub docs: BTreeMap<String, DocMeta>,
}

impl Default for Manifest {
    fn default() -> Self {
        Self {
            schema: SCHEMA_VERSION,
            docs: BTreeMap::new(),
        }
    }
}

impl Manifest {
    /// 读取清单；文件不存在 / 解析失败 / schema 版本不符 → 返回空清单（等价全量）
    pub fn load(dir: &Path) -> Self {
        let Ok(s) = std::fs::read_to_string(dir.join(MANIFEST_FILE)) else {
            return Self::default();
        };
        match serde_json::from_str::<Manifest>(&s) {
            Ok(m) if m.schema == SCHEMA_VERSION => m,
            _ => Self::default(),
        }
    }

    /// 原子落盘（临时文件 + rename），断电不会留下半截清单
    pub fn save(&self, dir: &Path) -> anyhow::Result<()> {
        let p = dir.join(MANIFEST_FILE);
        let tmp = dir.join(format!("{MANIFEST_FILE}.tmp"));
        std::fs::write(&tmp, serde_json::to_string(self)?)?;
        std::fs::rename(&tmp, &p)?;
        Ok(())
    }

    pub fn get(&self, path: &str) -> Option<DocMeta> {
        self.docs.get(path).copied()
    }

    /// 该文件相对清单是否**未变更**（size 与 mtime 都一致）
    pub fn unchanged(&self, path: &str, size: u64, mtime: i64) -> bool {
        matches!(
            self.docs.get(path),
            Some(m) if m.size == size as i64 && m.mtime == mtime
        )
    }

    pub fn len(&self) -> usize {
        self.docs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }
}

/// 打开（或创建）索引目录：schema 版本校验 + tokenizer 注册（读写两侧共用）
fn open_index(dir: &Path) -> anyhow::Result<Index> {
    std::fs::create_dir_all(dir)?;
    let schema = build_schema();
    let index = if dir.join("meta.json").exists() {
        // schema 演进迁移：版本不匹配 → 带指引报错，不静默失败
        check_schema_version(dir)?;
        Index::open_in_dir(dir)?
    } else {
        let idx = Index::create_in_dir(dir, schema.clone())?;
        persist_schema_version(dir)?;
        idx
    };
    index.tokenizers().register("seg", SegIndexTokenizer);
    Ok(index)
}

/* ------------------------------- 写侧（独占 writer） ------------------------------- */

/// 索引写入端：独占一个 `IndexWriter`（indexer 线程 / scanner / CLI 使用）。
///
/// tantivy 以目录锁保证同一索引目录同时只有一个 writer；
/// 写入端与读侧 [`IndexManager`] 完全分离，互不持锁。
pub struct Writer {
    index: Index,
    fields: SchemaFields,
    /// 索引目录（commit 后回写 schema 版本号用）
    dir: PathBuf,
    writer: Option<IndexWriter>,
    /// 本次会话的变更集：`Some(meta)` = 已写入，`None` = 已删除。
    /// commit 时统一合并进增量清单，避免每篇文档都读写一次清单文件。
    pending: HashMap<String, Option<DocMeta>>,
    /// 批量导入期是否跳过逐文档 delete_term（索引为空时纯属浪费）
    skip_delete_term: bool,
}

impl Writer {
    /// 打开（或创建）写侧上下文（此刻不建 writer，首次写入时懒创建）
    pub fn open(dir: &Path) -> anyhow::Result<Self> {
        let index = open_index(dir)?;
        let fields = fields_of(&index)?;
        Ok(Self {
            index,
            fields,
            dir: dir.to_path_buf(),
            writer: None,
            pending: HashMap::new(),
            skip_delete_term: false,
        })
    }

    /// 读取增量清单（indexer 做差异比对用；与写侧无锁竞争，只读文件）
    pub fn manifest(&self) -> Manifest {
        Manifest::load(&self.dir)
    }

    /// 批量导入模式：跳过逐文档 `delete_term`。
    ///
    /// 适用场景：全新索引（`clear()` 之后）或首次全量重建——此时不存在同 path 的旧文档，
    /// `delete_term` 只会往段里写一堆删除位，commit 时还要再扫一遍。
    pub fn set_skip_delete_term(&mut self, skip: bool) {
        self.skip_delete_term = skip;
    }

    /// 获取 writer（内存预算按机器内存自适应，见 `writer_budget`）
    ///
    /// 多工作线程：tantivy 每个写入线程各建自己的 segment，天然并行。
    /// 旧版固定单线程，飞腾 8 核在写入侧只用了 1 核——这是纯文本库
    /// 全量重建慢的主因之一。线程数见 `writer_threads`，总内存预算不变
    /// （平分到各线程），不会因为多线程吃更多内存。
    fn writer(&mut self) -> anyhow::Result<&mut IndexWriter> {
        if self.writer.is_none() {
            let budget = writer_budget();
            let threads = writer_threads();
            self.writer = Some(self.index.writer_with_num_threads(threads, budget)?);
        }
        Ok(self.writer.as_mut().unwrap())
    }

    /// 写入/更新一篇文档（按 path 先删后建，保证幂等）。
    ///
    /// 拼音串在这里现算——**只适合小批量或测试**。
    /// 批量索引请走 [`Writer::upsert_with_pinyin`]，在提取线程里并行算好，
    /// 否则所有文档都要排在这条唯一的串行通道上等拼音转换。
    pub fn upsert(&mut self, doc: &IndexDoc) -> anyhow::Result<()> {
        let mut py = crate::pinyin::text_to_pinyin_text(&doc.title);
        let py_body = crate::pinyin::text_to_pinyin_text(&doc.body);
        if !py_body.is_empty() {
            py.push(' ');
            py.push_str(&py_body);
        }
        self.upsert_with_pinyin(doc, &py)
    }

    /// 写入/更新一篇文档，拼音串由调用方提供。
    ///
    /// 这是索引管线的热路径：提取线程并行算好拼音，写入端只做 IO，
    /// 串行段里不再有 CPU 密集计算。
    pub fn upsert_with_pinyin(&mut self, doc: &IndexDoc, pinyin: &str) -> anyhow::Result<()> {
        let f_path = self.fields.f_path;
        let ext = Path::new(&doc.path)
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        // 手工构造文档（tantivy::doc! 宏在 lib 目标下路径解析有坑，直接 add_field_value）
        let mut d = TantivyDocument::default();
        d.add_field_value(f_path, &doc.path);
        d.add_field_value(self.fields.f_title, &doc.title);
        d.add_field_value(self.fields.f_body, &doc.body);
        d.add_field_value(self.fields.f_pinyin, pinyin);
        d.add_field_value(self.fields.f_size, &OwnedValue::I64(doc.size as i64));
        d.add_field_value(self.fields.f_mtime, &OwnedValue::I64(doc.mtime));
        d.add_field_value(self.fields.f_ext, &ext);
        let skip_del = self.skip_delete_term;
        let w = self.writer()?;
        if !skip_del {
            w.delete_term(tantivy::Term::from_field_text(f_path, &doc.path));
        }
        w.add_document(d)?;
        self.pending.insert(
            doc.path.clone(),
            Some(DocMeta {
                size: doc.size as i64,
                mtime: doc.mtime,
            }),
        );
        Ok(())
    }

    /// 删除一篇文档
    pub fn delete(&mut self, path: &str) -> anyhow::Result<()> {
        let term = tantivy::Term::from_field_text(self.fields.f_path, path);
        self.writer()?.delete_term(term);
        self.pending.insert(path.to_string(), None);
        Ok(())
    }

    /// 提交并落盘增量清单（一次 merge + 一次写清单，开销摊到整批）
    pub fn commit(&mut self) -> anyhow::Result<()> {
        if let Some(w) = self.writer.as_mut() {
            w.commit()?;
        }
        // tantivy 每次 commit 都会覆写 meta.json（丢失自定义键），需回写版本号
        persist_schema_version(&self.dir)?;
        // 清单与本批写入同步落盘：中途崩溃最多丢掉本批指纹（下次重算这几篇，不会漏）
        if !self.pending.is_empty() {
            let mut m = Manifest::load(&self.dir);
            m.schema = SCHEMA_VERSION;
            for (k, v) in self.pending.drain() {
                match v {
                    Some(meta) => {
                        m.docs.insert(k, meta);
                    }
                    None => {
                        m.docs.remove(&k);
                    }
                }
            }
            m.save(&self.dir)?;
        }
        Ok(())
    }

    /// 清空索引（重建用）：一并作废增量清单
    pub fn clear(&mut self) -> anyhow::Result<()> {
        self.writer()?.delete_all_documents()?;
        self.pending.clear();
        self.commit()?;
        let _ = std::fs::remove_file(self.dir.join(MANIFEST_FILE));
        Ok(())
    }
}

/// 影子索引子目录名（全量重建时先写这里，写完再整体替换）
pub const SHADOW_DIR: &str = ".rebuild";

/// 把影子索引里的 tantivy 数据替换到主目录（Recoll `-Z` 就地重建思路的加强版）。
///
/// 为什么需要它：旧做法是全量重建前先把索引删光，用户在几十秒到几分钟内
/// 一条都搜不到。现在新索引在 `.rebuild` 里独立建好，旧索引全程可用，
/// 只在最后这一步做毫秒级的文件替换。
///
/// 替换窗口极短（同分区 rename），期间搜索可能报一次「索引不存在」，
/// 调用方在重建结束后会重开索引，用户无感。
pub fn promote_shadow(dir: &Path) -> anyhow::Result<()> {
    let shadow = dir.join(SHADOW_DIR);
    if !shadow.is_dir() {
        return Ok(());
    }
    // 1) 清掉主目录旧索引数据（配置文件、历史库、提取缓存都保留）
    reset_index_data(dir)?;
    // 2) 影子文件搬进主目录。同分区 rename 是原子的；跨分区（极端情况）退化为复制。
    for entry in std::fs::read_dir(&shadow)? {
        let entry = entry?;
        let from = entry.path();
        let to = dir.join(entry.file_name());
        if std::fs::rename(&from, &to).is_err() {
            std::fs::copy(&from, &to)?;
            let _ = std::fs::remove_file(&from);
        }
    }
    let _ = std::fs::remove_dir_all(&shadow);
    Ok(())
}

/// 清理影子目录里上次中断留下的残渣
pub fn reset_shadow(dir: &Path) -> anyhow::Result<PathBuf> {
    let shadow = dir.join(SHADOW_DIR);
    if shadow.exists() {
        std::fs::remove_dir_all(&shadow)?;
    }
    std::fs::create_dir_all(&shadow)?;
    Ok(shadow)
}

/// 重建前清空索引目录（**保留用户配置与历史库**）。
///
/// 用途：schema 版本升级后旧索引无法打开，而这次无论如何都要全量重建——
/// 与其让用户自己找目录删文件，不如自动清掉 tantivy 数据文件后重来。
///
/// 保留：`config.json` / `app.json` / `synonyms.json` / 任意 `*.db`（搜索历史与书签）。
pub fn reset_index_data(dir: &Path) -> anyhow::Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        // `.rebuild` 必须保留：promote_shadow 正是靠「先 reset 主目录、再把影子搬进来」
        // 完成切换，把它一起删掉就等于把刚建好的新索引扔了。
        let keep = matches!(name.as_str(), "config.json" | "app.json" | "synonyms.json")
            || name == SHADOW_DIR
            || name.ends_with(".db");
        if keep {
            continue;
        }
        let p = entry.path();
        if p.is_dir() {
            std::fs::remove_dir_all(&p)?;
        } else {
            std::fs::remove_file(&p)?;
        }
    }
    Ok(())
}

/// 写侧内存预算：按机器内存自适应。
///
/// 预算过小 → 段多 → 后台合并频繁 → 索引与检索都慢（旧版固定 50MB）。
/// 取物理内存的 1/16，夹在 [128MB, 1GB]：4GB 机器给 256MB，8GB 给 512MB。
fn writer_budget() -> usize {
    const MIN: usize = 128 * 1024 * 1024;
    const MAX: usize = 1024 * 1024 * 1024;
    let total = total_memory_bytes().unwrap_or(2 * 1024 * 1024 * 1024);
    (total / 16).clamp(MIN, MAX)
}

/// 写侧工作线程数。
///
/// 取 CPU 核数，封顶 4：再多收益递减，且合并线程也要占核。
/// 单核/双核机器退化为 1–2，行为与旧版一致。
fn writer_threads() -> usize {
    let cores = std::thread::available_parallelism()
        .map(|v| v.get())
        .unwrap_or(1);
    cores.clamp(1, 4)
}

/// 读物理内存总量（Linux 读 /proc/meminfo；其它平台返回 None 走兜底）
fn total_memory_bytes() -> Option<usize> {
    #[cfg(target_os = "linux")]
    {
        let s = std::fs::read_to_string("/proc/meminfo").ok()?;
        for line in s.lines() {
            if let Some(rest) = line.strip_prefix("MemTotal:") {
                let kb: usize = rest.trim().trim_end_matches("kB").trim().parse().ok()?;
                return Some(kb * 1024);
            }
        }
        None
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

/* ------------------------------- 读侧（只读 + 自动 reload） ------------------------------- */

/// 索引读取端（搜索/预览/统计）：不持有 writer，绝不与写侧争目录锁。
///
/// reader 以 `ReloadPolicy::OnCommitWithDelay` 构建（tantivy 0.26 无 `OnCommit`）：
/// 监听 meta.json，**外部** writer commit 后毫秒级自动 reload，搜索无需手动刷新。
pub struct IndexManager {
    pub index: Index,
    pub f_path: Field,
    pub f_title: Field,
    pub f_body: Field,
    /// 拼音字段（仅索引不存储）：中文词语拼音连写串，供拼音查询命中
    pub f_pinyin: Field,
    pub f_size: Field,
    pub f_mtime: Field,
    pub f_ext: Field,
    /// 同义词表（open 时从 `<index_dir>/synonyms.json` 或内置表加载）
    pub synonyms: crate::synonym::SynonymTable,
    /// 常驻 reader：OnCommitWithDelay 自动 reload（读侧正确性核心）
    reader: IndexReader,
}

impl IndexManager {
    /// 打开或创建索引目录（读侧）
    pub fn open(dir: &Path) -> anyhow::Result<Self> {
        let index = open_index(dir)?;
        let fields = fields_of(&index)?;
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()?;
        Ok(Self {
            index,
            f_path: fields.f_path,
            f_title: fields.f_title,
            f_body: fields.f_body,
            f_pinyin: fields.f_pinyin,
            f_size: fields.f_size,
            f_mtime: fields.f_mtime,
            f_ext: fields.f_ext,
            synonyms: crate::synonym::SynonymTable::load(dir),
            reader,
        })
    }

    /// 立即 reload reader（测试/确定性场景；正常路径 OnCommitWithDelay 自动完成）
    pub fn reload(&self) -> anyhow::Result<()> {
        Ok(self.reader.reload()?)
    }

    pub fn num_docs(&self) -> u64 {
        self.reader.searcher().num_docs()
    }

    /// 读取文档正文（预览/子串复核用）
    pub fn doc_body(&self, addr: DocAddress) -> Option<(String, String)> {
        let searcher = self.reader.searcher();
        let doc: TantivyDocument = searcher.doc(addr).ok()?;
        let body = doc
            .get_first(self.f_body)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let path = doc
            .get_first(self.f_path)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        Some((path, body))
    }

    /// 按 path（含 `archive.zip#inner.txt` 虚拟路径）精确取正文。
    ///
    /// 供上层预览压缩包内层条目： tantivy 细节不外溢到 app 层。
    pub fn doc_body_by_path(&self, path: &str) -> Option<String> {
        let searcher = self.reader.searcher();
        let term = tantivy::Term::from_field_text(self.f_path, path);
        let q = tantivy::query::TermQuery::new(term, tantivy::schema::IndexRecordOption::Basic);
        let top = searcher
            .search(
                &q,
                &tantivy::collector::TopDocs::with_limit(1).order_by_score(),
            )
            .ok()?;
        let (_, addr) = top.first()?;
        self.doc_body(*addr).map(|(_, body)| body)
    }
}

/* --------------------------- 一次性计数（迁移校验专用） --------------------------- */

/// 一次性文档计数（迁移校验用）：打开索引取数后**即弃**，不建常驻 reader/watcher。
///
/// 与 [`IndexManager::open`] 的差别仅在 reload 策略：
/// - `IndexManager` 用 `OnCommitWithDelay`，tantivy 会 spawn
///   `thread-tantivy-meta-file-watcher` 轮询线程；该线程在目录对象 drop 后仍最多
///   存活一个 poll 周期（500ms），期间 transient 持有 meta.json 打开句柄——
///   Windows 上 `remove_dir_all` 与该轮询竞态即 sharing violation（删除失败）。
/// - 本函数用 `ReloadPolicy::Manual`：零线程、零 WatchHandle，reader 与 `Index`
///   drop 时全部句柄**同步**关闭，满足 Windows 删除前置条件。
///
/// 生产读侧路径（搜索/预览）不受影响，仍走 `IndexManager` 的自动 reload。
pub fn count_docs(dir: &Path) -> anyhow::Result<u64> {
    let index = open_index(dir)?;
    let reader: IndexReader = index
        .reader_builder()
        .reload_policy(ReloadPolicy::Manual)
        .try_into()?;
    // InnerIndexReader::new 已 eager load searcher（tantivy 0.26.2 reader/mod.rs L172-178 实证），
    // 直接取数即可，无需 reload
    let num_docs = reader.searcher().num_docs();
    // 显式 drop：句柄随 drop 同步关闭（Windows 上删除目录的前置条件）
    drop(reader);
    drop(index);
    Ok(num_docs)
}

/* --------------------------------- 版本持久化 --------------------------------- */

/// 校验 meta.json 记录的 schema 版本；缺失 / 损坏 / 不符 → 指引重建
fn check_schema_version(dir: &Path) -> anyhow::Result<()> {
    let raw = std::fs::read_to_string(dir.join("meta.json"))?;
    let meta: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| anyhow::anyhow!("索引 meta.json 损坏: {e}"))?;
    let found = meta.get(SCHEMA_VERSION_KEY).and_then(|v| v.as_u64());
    match found {
        Some(v) if v == SCHEMA_VERSION as u64 => Ok(()),
        _ => anyhow::bail!(
            "索引由旧版本创建，请重建（schema 版本不匹配：期望 {SCHEMA_VERSION}，实际 {found:?}；删除索引目录后重新扫描即可）"
        ),
    }
}

/// 把 schema 版本号写回 meta.json（原子：临时文件 + rename）
fn persist_schema_version(dir: &Path) -> anyhow::Result<()> {
    let p = dir.join("meta.json");
    let mut meta: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&p)?)
        .map_err(|e| anyhow::anyhow!("索引 meta.json 损坏: {e}"))?;
    if let Some(obj) = meta.as_object_mut() {
        obj.insert(
            SCHEMA_VERSION_KEY.to_string(),
            serde_json::json!(SCHEMA_VERSION),
        );
    }
    let tmp = dir.join("meta.json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(&meta)?)?;
    std::fs::rename(&tmp, &p)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(path: &str, body: &str) -> IndexDoc {
        IndexDoc {
            path: path.to_string(),
            title: path.rsplit('/').next().unwrap_or(path).to_string(),
            body: body.to_string(),
            size: body.len() as u64,
            mtime: 0,
        }
    }

    /// 写侧 seed：一批文档入库并 commit
    fn seed(dir: &Path, docs: &[IndexDoc]) {
        let mut w = Writer::open(dir).unwrap();
        for d in docs {
            w.upsert(d).unwrap();
        }
        w.commit().unwrap();
    }

    #[test]
    fn schema_v3_size_mtime_are_indexed_and_fast() {
        let schema = build_schema();
        for name in ["size", "mtime"] {
            let entry = schema.get_field_entry(schema.get_field(name).unwrap());
            assert!(entry.is_indexed(), "{name} 应可索引（RangeQuery）");
            assert!(entry.is_fast(), "{name} 应为 FAST（排序）");
            assert!(entry.is_stored(), "{name} 应 STORED（结果列）");
        }
    }

    #[test]
    fn schema_version_recorded_and_survives_commit() {
        let dir = tempfile::tempdir().unwrap();
        seed(dir.path(), &[doc("/t/a.txt", "中华人民共和国")]);
        // commit 后版本键仍在（tantivy 覆写 meta.json 不丢失自定义键）
        let raw = std::fs::read_to_string(dir.path().join("meta.json")).unwrap();
        assert!(
            raw.contains(SCHEMA_VERSION_KEY),
            "meta.json 应记录 schema 版本: {raw}"
        );
        // 重新 open 正常（版本匹配）
        let mgr = IndexManager::open(dir.path()).unwrap();
        assert_eq!(mgr.num_docs(), 1);
    }

    #[test]
    fn legacy_index_without_version_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        // 旧版索引：meta.json 存在但无版本键
        std::fs::write(
            dir.path().join("meta.json"),
            r#"{"segments":[],"schema":[],"opstamp":0}"#,
        )
        .unwrap();
        let err = match IndexManager::open(dir.path()) {
            Ok(_) => panic!("旧版索引应被拒绝"),
            Err(e) => e.to_string(),
        };
        assert!(err.contains("旧版本"), "应提示重建，实际: {err}");
    }

    #[test]
    fn mismatched_schema_version_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        seed(dir.path(), &[doc("/t/a.txt", "x")]);
        // 人为写入更高版本号 → 不匹配
        let mut meta: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.path().join("meta.json")).unwrap())
                .unwrap();
        meta.as_object_mut()
            .unwrap()
            .insert(SCHEMA_VERSION_KEY.to_string(), serde_json::json!(99));
        std::fs::write(
            dir.path().join("meta.json"),
            serde_json::to_string_pretty(&meta).unwrap(),
        )
        .unwrap();
        assert!(IndexManager::open(dir.path()).is_err());
    }

    #[test]
    fn clear_keeps_schema_version() {
        let dir = tempfile::tempdir().unwrap();
        seed(dir.path(), &[doc("/t/a.txt", "x")]);
        let mut w = Writer::open(dir.path()).unwrap();
        w.clear().unwrap();
        drop(w);
        let mgr = IndexManager::open(dir.path()).unwrap();
        assert_eq!(mgr.num_docs(), 0);
        let raw = std::fs::read_to_string(dir.path().join("meta.json")).unwrap();
        assert!(raw.contains(SCHEMA_VERSION_KEY));
        // clear 后仍可正常重开
        drop(mgr);
        let mgr = IndexManager::open(dir.path()).unwrap();
        assert_eq!(mgr.num_docs(), 0);
    }

    /// R2 正确性核心：读侧 IndexManager 先打开，**另一个** Writer 实例 commit 后，
    /// 读侧 searcher 在 ≤2s 内自动可见（OnCommitWithDelay 监听 meta.json）。
    #[test]
    fn external_commit_becomes_visible_to_open_reader() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = IndexManager::open(dir.path()).unwrap();
        assert_eq!(mgr.num_docs(), 0);

        // 另一个（写侧）writer 实例：独立 Index 对象 + 独立 IndexWriter
        let mut w = Writer::open(dir.path()).unwrap();
        w.upsert(&doc("/t/x.txt", "量子计算 世界")).unwrap();
        w.commit().unwrap();
        drop(w);

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while mgr.num_docs() == 0 {
            assert!(
                std::time::Instant::now() < deadline,
                "外部 commit 后 2s 内读侧应自动可见（OnCommitWithDelay）"
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert_eq!(mgr.num_docs(), 1);
        // 手动 reload 兜底也应立即可见且一致
        mgr.reload().unwrap();
        assert_eq!(mgr.num_docs(), 1);
    }

    /// `count_docs` 与 `IndexManager::num_docs` 两条计数通道互证一致
    /// （迁移校验走前者：Manual reader 无 watcher 线程，Windows 回滚删除无竞态）
    #[test]
    fn count_docs_matches_manager() {
        let dir = tempfile::tempdir().unwrap();
        seed(
            dir.path(),
            &[
                doc("/t/a.txt", "x"),
                doc("/t/b.txt", "y"),
                doc("/t/c.txt", "z"),
            ],
        );
        assert_eq!(count_docs(dir.path()).unwrap(), 3);
        let mgr = IndexManager::open(dir.path()).unwrap();
        assert_eq!(mgr.num_docs(), 3);
        // 空目录（新建索引）计数为 0
        let empty = tempfile::tempdir().unwrap();
        assert_eq!(count_docs(empty.path()).unwrap(), 0);
    }

    #[test]
    fn writer_lock_excludes_second_writer() {
        // 单实例语义：持 writer 的目录上，第二个 writer 无法创建
        let dir = tempfile::tempdir().unwrap();
        let mut w1 = Writer::open(dir.path()).unwrap();
        w1.upsert(&doc("/t/a.txt", "x")).unwrap();
        let mut w2 = Writer::open(dir.path()).unwrap();
        assert!(
            w2.upsert(&doc("/t/b.txt", "y")).is_err(),
            "第二个 writer 应被 tantivy 目录锁拒绝"
        );
    }
}
