//! 应用状态：索引目录 + 懒加载 IndexManager
//!
//! `index_dir` 用 `RwLock<PathBuf>` 包装，以支持"存储目录迁移"运行期热切换（子项 3.3）：
//! 迁移完成后调用 `set_index_dir()`，后续 `with_mgr` 会从新路径重开索引。
//! 读取统一走 `index_dir()`，避免各处 clone 出过期路径。
//!
//! TOCTOU 防护（P1）：缓存以 `(PathBuf, IndexManager)` 存打开时的目录，
//! `with_mgr` 在 `mgr` 锁**内**重读 `index_dir()` 并比对缓存目录，
//! 与迁移热切换（`set_index_dir`）的竞态下不匹配即重开，杜绝锁外快照命中旧目录。
//!
//! 架构 v2 R2：IndexManager 为**只读**缓存（无 writer）；写侧统一走
//! `stcore::index::Writer`（队列线程独占），commit 后读侧 reader 自动 reload。

use std::path::PathBuf;
use std::sync::{Mutex, RwLock};

use stcore::index::IndexManager;
#[cfg(test)]
use stcore::index::{IndexDoc, Writer};

pub struct AppState {
    index_dir: RwLock<PathBuf>,
    /// None = 尚未打开；Some((打开时的目录, IndexManager))，目录用于检测热切换后失效
    pub mgr: Mutex<Option<(PathBuf, IndexManager)>>,
    /// GUI 启动时由 CLI `--open-dir` 传入的初始目录（前端首次拉取后清空，P5 右击搜索）
    pending_open_dir: Mutex<Option<String>>,
}

impl AppState {
    pub fn new(index_dir: PathBuf) -> Self {
        Self {
            index_dir: RwLock::new(index_dir),
            mgr: Mutex::new(None),
            pending_open_dir: Mutex::new(None),
        }
    }

    /// 记录 CLI 传入的初始目录（仅启动时调用）
    pub fn set_pending_open_dir(&self, dir: Option<String>) {
        *self
            .pending_open_dir
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = dir;
    }

    /// 取走初始目录（一次性；前端启动时调用）
    pub fn take_pending_open_dir(&self) -> Option<String> {
        self.pending_open_dir
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
    }

    /// 当前索引目录（快照）
    pub fn index_dir(&self) -> PathBuf {
        self.index_dir
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// 切换索引目录并作废缓存（存储迁移后调用）
    pub fn set_index_dir(&self, dir: PathBuf) {
        *self.index_dir.write().unwrap_or_else(|e| e.into_inner()) = dir;
        self.invalidate();
    }

    /// 获取或打开索引（GUI/CLI 共用入口）
    ///
    /// 锁内重读 `index_dir()` 再比对缓存目录：迁移热切换发生在这把锁外时，
    /// 锁外快照可能已过期；目录不匹配即重开，保证搜索命中当前目录。
    pub fn with_mgr<T>(
        &self,
        f: impl FnOnce(&IndexManager) -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        let mut guard = self.mgr.lock().unwrap_or_else(|e| e.into_inner());
        let dir = self.index_dir();
        if guard.as_ref().map(|(d, _)| d != &dir).unwrap_or(true) {
            let mgr = IndexManager::open(&dir)?;
            *guard = Some((dir, mgr));
        }
        match guard.as_ref() {
            Some((_, mgr)) => f(mgr),
            None => anyhow::bail!("索引缓存异常：打开后仍为空"),
        }
    }

    /// 重建后使缓存失效
    pub fn invalidate(&self) {
        *self.mgr.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

/// 默认索引目录：平台数据目录/search-tool/index
///
/// 目录名沿用 `search-tool`：这是既有安装的数据位置，改掉等于让老用户
/// 的索引与历史记录失效，故只改对外显示名、不动数据路径。
pub fn default_index_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("SEARCH_TOOL_INDEX_DIR") {
        return PathBuf::from(dir);
    }
    let base = dirs::data_dir().unwrap_or_else(|| {
        // dirs 未引入时的兜底（Linux/macOS 惯例）
        PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".into())).join(".local/share")
    });
    base.join("search-tool").join("index")
}

/// 极简 dirs 替代（避免为一个小函数引 crate）
mod dirs {
    use std::path::PathBuf;

    /// 平台数据目录：
    /// - Windows：`%LOCALAPPDATA%`（roaming 用 APPDATA，本地索引数据不该漫游）
    /// - 其它：`$XDG_DATA_HOME` → `$HOME/.local/share`
    ///
    /// 曾经只认 XDG/HOME：Windows 上两者通常均未设置，索引目录会落到
    /// `C:\tmp\search-tool\index`（污染盘根、可能需管理员权限）。
    pub fn data_dir() -> Option<PathBuf> {
        #[cfg(windows)]
        {
            std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
        }
        #[cfg(not(windows))]
        {
            std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    #[test]
    fn with_mgr_detects_hot_switched_dir() {
        let a = TempDir::new("state-a");
        let b = TempDir::new("state-b");
        // a 播种 1 篇文档，b 保持空索引（写侧 Writer）
        {
            let mut w = Writer::open(a.path()).unwrap();
            w.upsert(&IndexDoc {
                path: "/x/a.txt".to_string(),
                title: "a.txt".to_string(),
                body: "hello 世界".to_string(),
                size: 5,
                mtime: 0,
            })
            .unwrap();
            w.commit().unwrap();
        }

        let st = AppState::new(a.path().to_path_buf());
        assert_eq!(st.with_mgr(|m| Ok(m.num_docs())).unwrap(), 1);

        // 热切换到 b：缓存目录不匹配 → 必须重开读到 b 的 0 篇（而非旧 a 的 1 篇）
        st.set_index_dir(b.path().to_path_buf());
        assert_eq!(st.with_mgr(|m| Ok(m.num_docs())).unwrap(), 0);

        // 切回 a 同样重开
        st.set_index_dir(a.path().to_path_buf());
        assert_eq!(st.with_mgr(|m| Ok(m.num_docs())).unwrap(), 1);
    }

    #[test]
    fn mgr_lock_poison_is_recoverable() {
        let a = TempDir::new("state-poison");
        let st = AppState::new(a.path().to_path_buf());
        // 持锁 panic 制造毒化，验证 unwrap_or_else(into_inner) 恢复而非连锁 panic
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = st.mgr.lock().unwrap();
            panic!("模拟持锁线程 panic");
        }));
        assert!(st.with_mgr(|m| Ok(m.num_docs())).is_ok());
        assert!(st.with_mgr(|m| Ok(m.num_docs())).is_ok());
    }
}
