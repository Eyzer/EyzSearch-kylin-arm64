//! 提取结果缓存（借鉴 rga / ripgrep-all 的 AdaCache 思路）
//!
//! 目的：**重建索引时跳过重新解析文档**。
//!
//! 全量重建的真实瓶颈是解析（PDF / Office / OCR），不是写倒排。
//! 把「路径 + 大小 + 修改时间」作为键，缓存提取出的纯文本与拼音，
//! 重建时直接读缓存走 upsert，解析环节整个省掉。
//!
//! 关键约束：
//!   - 缓存文件名以 `.db` 结尾，`reset_index_data()` 的保留规则天然覆盖它，
//!     schema 升级时的清库不会顺手把它删掉（这正是加速的前提）。
//!   - 缓存带结构版本号，提取逻辑变了就把 `CACHE_VER` 加一，旧缓存整体作废。
//!   - 缓存失效是安全的：读不到就重新解析，不影响正确性。

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// 缓存库文件名（位于 index_dir 根，与 tantivy 数据同级）
pub const CACHE_FILE: &str = "extract-cache.db";

/// 缓存结构版本：提取逻辑或字段结构变化时递增，旧缓存自动作废
pub const CACHE_VER: i64 = 1;

/// 单条记录上限（JSON 字节数）。超长文档不缓存——rga 同样策略，
/// 否则几本大部头就能把库撑到几个 G，收益还不成正比。
const MAX_ENTRY_BYTES: usize = 4 * 1024 * 1024;

/// 缓存库软上限；超过后停止写入新条目（已有条目继续命中）
const MAX_DB_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// 一条缓存文档（与 `IndexDoc` 对齐，额外带上预计算拼音）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedDoc {
    pub path: String,
    pub title: String,
    pub body: String,
    pub size: u64,
    pub mtime: i64,
    pub pinyin: String,
}

/// 提取结果缓存。所有方法失败都降级为「未命中」，绝不打断索引流程。
pub struct ExtractCache {
    conn: Mutex<Connection>,
    hits: AtomicU64,
    misses: AtomicU64,
    /// 已写入的近似字节数（含索引页开销的粗估），用于软上限判断
    bytes: AtomicU64,
}

impl ExtractCache {
    /// 打开（或新建）缓存库。失败返回 `None`，调用方当作「无缓存」继续跑。
    pub fn open(index_dir: &Path) -> Option<Self> {
        let path = index_dir.join(CACHE_FILE);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let conn = Connection::open(&path).ok()?;
        // WAL：读写不互斥，多线程提取时写缓存不会卡住其他线程
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
        let _ = conn.pragma_update(None, "synchronous", "NORMAL");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS extract (
                 path  TEXT PRIMARY KEY,
                 size  INTEGER NOT NULL,
                 mtime INTEGER NOT NULL,
                 ver   INTEGER NOT NULL,
                 bytes INTEGER NOT NULL,
                 docs  TEXT NOT NULL
             );",
        )
        .ok()?;
        // 版本不符：整体作废（提取逻辑变了，旧文本可能不完整）
        let stale: i64 = conn
            .query_row("SELECT COUNT(*) FROM extract WHERE ver <> ?1", [CACHE_VER], |r| {
                r.get(0)
            })
            .unwrap_or(0);
        if stale > 0 {
            let _ = conn.execute("DELETE FROM extract WHERE ver <> ?1", [CACHE_VER]);
        }
        let bytes: u64 = conn
            .query_row("SELECT COALESCE(SUM(bytes), 0) FROM extract", [], |r| {
                r.get::<_, i64>(0)
            })
            .map(|v| v.max(0) as u64)
            .unwrap_or(0);
        Some(Self {
            conn: Mutex::new(conn),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            bytes: AtomicU64::new(bytes),
        })
    }

    /// 命中则返回缓存的文档列表。路径为空或读取失败都返回 `None`。
    pub fn get(&self, path: &str, size: u64, mtime: i64) -> Option<Vec<CachedDoc>> {
        let conn = self.conn.lock().ok()?;
        let raw: Option<String> = conn
            .query_row(
                "SELECT docs FROM extract WHERE path = ?1 AND size = ?2 AND mtime = ?3 AND ver = ?4",
                params![path, size as i64, mtime, CACHE_VER],
                |r| r.get(0),
            )
            .optional()
            .ok()?;
        drop(conn);
        match raw {
            Some(s) => match serde_json::from_str::<Vec<CachedDoc>>(&s) {
                Ok(v) => {
                    self.hits.fetch_add(1, Ordering::Relaxed);
                    Some(v)
                }
                Err(_) => {
                    self.misses.fetch_add(1, Ordering::Relaxed);
                    None
                }
            },
            None => {
                self.misses.fetch_add(1, Ordering::Relaxed);
                None
            }
        }
    }

    /// 写入一条缓存。空结果（提取不出文字）也照存——避免每次重建都重试损坏文件。
    pub fn put(&self, path: &str, size: u64, mtime: i64, docs: &[CachedDoc]) {
        let Ok(json) = serde_json::to_string(&docs) else {
            return;
        };
        if json.len() > MAX_ENTRY_BYTES {
            return;
        }
        if self.bytes.load(Ordering::Relaxed) > MAX_DB_BYTES {
            return;
        }
        let Ok(mut conn) = self.conn.lock() else {
            return;
        };
        let n = json.len() as u64;
        let ok = conn
            .execute(
                "INSERT INTO extract (path, size, mtime, ver, bytes, docs)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(path) DO UPDATE SET
                     size = excluded.size,
                     mtime = excluded.mtime,
                     ver = excluded.ver,
                     bytes = excluded.bytes,
                     docs = excluded.docs",
                params![path, size as i64, mtime, CACHE_VER, n as i64, json],
            )
            .is_ok();
        drop(conn);
        if ok {
            self.bytes.fetch_add(n, Ordering::Relaxed);
        }
    }

    /// 清掉指定路径的缓存（增量模式下文件变了就没意义了，靠 size+mtime 自然失效，
    /// 这里只为删除场景准备）。
    pub fn forget(&self, path: &str) {
        if let Ok(conn) = self.conn.lock() {
            let _ = conn.execute("DELETE FROM extract WHERE path = ?1", [path]);
        }
    }

    /// 命中/未命中统计，用于索引结束时的日志与冒烟断言
    pub fn stats(&self) -> (u64, u64) {
        (
            self.hits.load(Ordering::Relaxed),
            self.misses.load(Ordering::Relaxed),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("stcache-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn put_then_get_hits() {
        let dir = tmpdir("hit");
        let c = ExtractCache::open(&dir).expect("open");
        let docs = vec![CachedDoc {
            path: "/a.txt".into(),
            title: "a.txt".into(),
            body: "档案管理".into(),
            size: 12,
            mtime: 100,
            pinyin: "dangan".into(),
        }];
        c.put("/a.txt", 12, 100, &docs);
        let got = c.get("/a.txt", 12, 100).expect("hit");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].body, "档案管理");
        assert_eq!(got[0].pinyin, "dangan");
        assert_eq!(c.stats(), (1, 0));
    }

    #[test]
    fn changed_size_or_mtime_misses() {
        let dir = tmpdir("miss");
        let c = ExtractCache::open(&dir).expect("open");
        c.put("/a.txt", 12, 100, &[]);
        assert!(c.get("/a.txt", 13, 100).is_none(), "大小变了应失效");
        assert!(c.get("/a.txt", 12, 101).is_none(), "时间变了应失效");
        assert!(c.get("/a.txt", 12, 100).is_some(), "原键仍命中");
    }

    #[test]
    fn empty_docs_are_cached_too() {
        let dir = tmpdir("empty");
        let c = ExtractCache::open(&dir).expect("open");
        c.put("/bad.pdf", 1, 1, &[]);
        let got = c.get("/bad.pdf", 1, 1).expect("空结果也要命中");
        assert!(got.is_empty());
    }

    #[test]
    fn version_bump_invalidates_old_rows() {
        let dir = tmpdir("ver");
        {
            let c = ExtractCache::open(&dir).expect("open");
            c.put("/a.txt", 1, 1, &[]);
        }
        // 手工把版本改成旧值，模拟升级
        {
            let conn = Connection::open(dir.join(CACHE_FILE)).unwrap();
            conn.execute("UPDATE extract SET ver = ver - 1", []).unwrap();
        }
        let c = ExtractCache::open(&dir).expect("reopen");
        assert!(c.get("/a.txt", 1, 1).is_none(), "旧版本缓存应整体作废");
    }

    #[test]
    fn reopen_keeps_entries() {
        let dir = tmpdir("reopen");
        {
            let c = ExtractCache::open(&dir).expect("open");
            c.put("/a.txt", 1, 1, &[]);
        }
        let c = ExtractCache::open(&dir).expect("reopen");
        assert!(c.get("/a.txt", 1, 1).is_some());
    }
}
