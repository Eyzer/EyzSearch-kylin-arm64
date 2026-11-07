//! 配置层（子项 3 所有权）：文件类型表 / per-format 三态目录规则 / 定时更新 / 索引根目录
//!
//! 持久化：`<index_dir>/config.json` —— 规则单一事实源（R10：扫描器与查询共用同一判定函数）。
//!
//! 本文件由集成负责人搭骨架（编译可跑的最小实现），子项 3 agent 在其上补齐：
//!   - 类型表 UI 所需的分页/排序（如需）
//!   - 目录规则与 scanner 的接线（`RuleSet::allows`）
//!   - 存储目录迁移、定时更新三态、重建取消（→ schedule.rs / app 层）

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 配置文件名（位于 index_dir 下）
pub const CONFIG_FILE: &str = "config.json";

/// 文件类型（对应前端 FileType）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileType {
    pub id: i64,
    pub name: String,
    /// 扩展名（不含点，小写）
    pub exts: Vec<String>,
    /// 内置类型不可删
    #[serde(default)]
    pub builtin: bool,
}

/// per-format 目录规则三态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleState {
    /// 强制包含（覆盖上级 exclude）
    Include,
    /// 强制排除
    Exclude,
    /// 继承上级判定（默认）
    Inherit,
}

/// 目录规则（glob 模式 + 三态）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirRule {
    pub id: i64,
    /// 归属文件类型；null = 对所有类型生效
    pub file_type_id: Option<i64>,
    /// glob 模式，如 `**/node_modules/**`
    pub pattern: String,
    pub state: RuleState,
}

/// 定时更新三态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScheduleMode {
    Never,
    Interval,
    Daily,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleConfig {
    pub mode: ScheduleMode,
    /// mode=interval 时的间隔分钟数
    pub interval_minutes: u64,
    /// mode=daily 时的触发时刻 "HH:mm"
    pub daily_at: String,
}

impl Default for ScheduleConfig {
    fn default() -> Self {
        Self {
            mode: ScheduleMode::Never,
            interval_minutes: 60,
            daily_at: "03:00".to_string(),
        }
    }
}

/// 默认参与索引的文件类型 id：「常用办公文档」。
///
/// 索引默认只收办公文档（Word/Excel/PPT/PDF/纯文本/Markdown/WPS），
/// 代码、二进制、图片等按需在「索引管理 → 索引构建」里勾选后再纳入。
pub const DEFAULT_INDEX_TYPE_ID: i64 = 13;

/// 全局配置（config.json 顶层结构）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    #[serde(default)]
    pub file_types: Vec<FileType>,
    #[serde(default)]
    pub dir_rules: Vec<DirRule>,
    #[serde(default)]
    pub schedule: ScheduleConfig,
    /// 已索引根目录（"指定目录"下拉数据源）
    #[serde(default)]
    pub indexed_roots: Vec<String>,
    /// 参与索引的文件类型 id 集合。
    ///
    /// `None` = 旧配置（字段缺失）→ 按 [`DEFAULT_INDEX_TYPE_ID`] 处理；
    /// `Some(vec![])` = 用户选择「全部类型」。
    #[serde(default)]
    pub index_types: Option<Vec<i64>>,
    /// 自增主键分配器
    #[serde(default)]
    pub next_id: i64,
}

impl Default for Config {
    fn default() -> Self {
        let file_types = builtin_types();
        Self {
            next_id: file_types.iter().map(|t| t.id).max().unwrap_or(0) + 1,
            file_types,
            dir_rules: Vec::new(),
            schedule: ScheduleConfig::default(),
            indexed_roots: Vec::new(),
            index_types: Some(vec![DEFAULT_INDEX_TYPE_ID]),
        }
    }
}

/// 内置文件类型
pub fn builtin_types() -> Vec<FileType> {
    let mk = |id: i64, name: &str, exts: &[&str]| FileType {
        id,
        name: name.to_string(),
        exts: exts.iter().map(|s| s.to_string()).collect(),
        builtin: true,
    };
    vec![
        mk(1, "文本", &["txt", "text", "log", "ini", "conf", "cfg"]),
        mk(2, "Markdown", &["md", "markdown", "mdown"]),
        mk(
            3,
            "Office 文档",
            &[
                "doc", "docx", "xls", "xlsx", "ppt", "pptx", "csv", "tsv", "rtf", "wps", "wpt",
                "et", "ett", "dps", "dpt",
            ],
        ),
        mk(4, "PDF", &["pdf"]),
        mk(5, "网页", &["html", "htm", "xhtml", "xml", "mht"]),
        mk(6, "邮件", &["eml", "msg"]),
        mk(
            7,
            "电子书",
            &["epub", "mobi", "azw3", "azw", "fb2", "chm", "djvu"],
        ),
        mk(
            8,
            "代码",
            &[
                "rs", "java", "kt", "c", "h", "cpp", "hpp", "py", "js", "ts", "tsx", "jsx", "go",
                "rb", "php", "sh", "sql", "json", "yaml", "yml", "toml",
            ],
        ),
        mk(
            9,
            "压缩包",
            &["zip", "tar", "gz", "tgz", "bz2", "7z", "rar"],
        ),
        // 图片：需 OCR 通道（P4-A，默认关；见 SEARCH_TOOL_OCR）
        mk(
            10,
            "图片",
            &["png", "jpg", "jpeg", "webp", "bmp", "tiff", "tif", "gif"],
        ),
        // 旧格式 / ODF（OLE 二进制与 ODF、RTF，走 anydoc）
        mk(
            11,
            "旧格式",
            &[
                "doc", "dot", "xls", "xlsb", "xlt", "ppt", "pps", "pot", "odt", "ods", "odp",
                "rtf", "wps", "wpt", "et", "ett", "dps", "dpt",
            ],
        ),
        // 二进制可执行/库文件：strings 提取（R4；ASCII ≥5 + UTF-16LE，256KB 截断）
        // 不含 bin：bin 是 static_ext 的兜底标签，语义上不是确定格式
        mk(12, "二进制", &["exe", "dll", "so", "dylib"]),
        // 默认索引类型：只覆盖日常办公文档，避免一上来就把代码/二进制/图片全扫一遍
        mk(
            13,
            "常用办公文档",
            &[
                "doc", "docx", "xls", "xlsx", "ppt", "pptx", "pdf", "txt", "md", "rtf", "csv",
                "wps", "wpt", "et", "ett", "dps", "dpt",
            ],
        ),
    ]
}

impl Config {
    /// config.json 路径
    pub fn path_in(index_dir: &Path) -> PathBuf {
        index_dir.join(CONFIG_FILE)
    }

    /// 读取配置；文件不存在时返回内置默认（不落盘）
    pub fn load(index_dir: &Path) -> anyhow::Result<Self> {
        let p = Self::path_in(index_dir);
        if !p.exists() {
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(&p)?;
        let mut cfg: Config = serde_json::from_str(&raw)?;
        // 新版本新增的内置类型补种 + 既有内置类型的扩展名补齐
        // （只增不删，不覆盖用户手工增删的自定义类型；用于让老索引目录也能识别新格式，如 WPS）
        for bt in builtin_types() {
            if let Some(pos) = cfg.file_types.iter().position(|t| t.id == bt.id) {
                for e in &bt.exts {
                    if !cfg.file_types[pos]
                        .exts
                        .iter()
                        .any(|x| x.eq_ignore_ascii_case(e))
                    {
                        cfg.file_types[pos].exts.push(e.clone());
                    }
                }
            } else {
                cfg.file_types.push(bt);
            }
        }
        // next_id 防碰撞：至少为所有现有 id（类型 + 目录规则，含内置种子类型）最大值 + 1。
        // 旧版本/手改的 config.json 可能带回退 next_id，直接分配会撞 id。
        let max_id = cfg
            .file_types
            .iter()
            .map(|t| t.id)
            .chain(cfg.dir_rules.iter().map(|r| r.id))
            .max()
            .unwrap_or(0);
        cfg.next_id = cfg.next_id.max(max_id + 1);
        Ok(cfg)
    }

    /// 写入配置（原子：先写临时文件再 rename）
    pub fn save(&self, index_dir: &Path) -> anyhow::Result<()> {
        std::fs::create_dir_all(index_dir)?;
        let p = Self::path_in(index_dir);
        let tmp = p.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        std::fs::rename(&tmp, &p)?;
        Ok(())
    }

    /// 类型名 → 扩展名列表（搜索层"指定文件类型"过滤用）
    pub fn exts_of(&self, type_name: &str) -> Option<Vec<String>> {
        self.file_types
            .iter()
            .find(|t| t.name == type_name)
            .map(|t| t.exts.clone())
    }

    /// 全部受支持扩展名（扫描器用）
    pub fn all_exts(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .file_types
            .iter()
            .flat_map(|t| t.exts.iter().cloned())
            .collect();
        v.sort();
        v.dedup();
        v
    }

    /// 实际参与索引的类型 id 集合（`None` → 默认「常用办公文档」）
    pub fn index_type_ids(&self) -> Vec<i64> {
        match &self.index_types {
            Some(v) if !v.is_empty() => v.clone(),
            Some(_) => Vec::new(), // 显式「全部类型」
            None => vec![DEFAULT_INDEX_TYPE_ID],
        }
    }

    /// 实际参与索引的扩展名集合（索引遍历的唯一判据）。
    ///
    /// - `index_types` 为 `Some(vec![])`（用户全选）→ 全部类型
    /// - 否则取所选类型的扩展名并集
    pub fn index_exts(&self) -> Vec<String> {
        let ids = self.index_type_ids();
        let mut v: Vec<String> = if ids.is_empty() {
            self.all_exts()
        } else {
            self.file_types
                .iter()
                .filter(|t| ids.contains(&t.id))
                .flat_map(|t| t.exts.iter().cloned())
                .collect()
        };
        v.sort();
        v.dedup();
        v
    }

    /// 设置参与索引的类型（空 vec = 全部类型）
    pub fn set_index_types(&mut self, ids: Vec<i64>) {
        self.index_types = Some(ids);
    }

    /* ---------------------- 类型表 CRUD ---------------------- */

    pub fn add_type(&mut self, name: impl Into<String>, exts: Vec<String>) -> FileType {
        let id = self.next_id;
        self.next_id += 1;
        let ft = FileType {
            id,
            name: name.into(),
            exts: exts
                .into_iter()
                .map(|e| e.trim_start_matches('.').to_lowercase())
                .collect(),
            builtin: false,
        };
        self.file_types.push(ft.clone());
        // 手动新增的类型默认纳入索引范围（否则用户建了类型却扫不到文件）
        let mut ids = self.index_type_ids();
        if !ids.is_empty() && !ids.contains(&id) {
            ids.push(id);
            self.index_types = Some(ids);
        }
        ft
    }

    pub fn update_type(&mut self, id: i64, name: String, exts: Vec<String>) -> anyhow::Result<()> {
        let ft = self
            .file_types
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or_else(|| anyhow::anyhow!("文件类型不存在: {id}"))?;
        ft.name = name;
        ft.exts = exts
            .into_iter()
            .map(|e| e.trim_start_matches('.').to_lowercase())
            .collect();
        Ok(())
    }

    /// 删除类型（内置不可删；连带删除其目录规则）
    pub fn remove_type(&mut self, id: i64) -> anyhow::Result<()> {
        let Some(ft) = self.file_types.iter().find(|t| t.id == id) else {
            anyhow::bail!("文件类型不存在: {id}");
        };
        if ft.builtin {
            anyhow::bail!("内置类型不可删除: {}", ft.name);
        }
        self.file_types.retain(|t| t.id != id);
        self.dir_rules.retain(|r| r.file_type_id != Some(id));
        Ok(())
    }

    /* ---------------------- 目录规则 CRUD ---------------------- */

    /// 新增或覆盖同 (fileTypeId, pattern) 的规则
    pub fn set_rule(
        &mut self,
        file_type_id: Option<i64>,
        pattern: impl Into<String>,
        state: RuleState,
    ) -> DirRule {
        let pattern = pattern.into();
        if let Some(r) = self
            .dir_rules
            .iter_mut()
            .find(|r| r.file_type_id == file_type_id && r.pattern == pattern)
        {
            r.state = state;
            return r.clone();
        }
        let id = self.next_id;
        self.next_id += 1;
        let rule = DirRule {
            id,
            file_type_id,
            pattern,
            state,
        };
        self.dir_rules.push(rule.clone());
        rule
    }

    pub fn remove_rule(&mut self, id: i64) -> anyhow::Result<()> {
        let before = self.dir_rules.len();
        self.dir_rules.retain(|r| r.id != id);
        if self.dir_rules.len() == before {
            anyhow::bail!("目录规则不存在: {id}");
        }
        Ok(())
    }

    pub fn rules_of(&self, file_type_id: Option<i64>) -> Vec<&DirRule> {
        self.dir_rules
            .iter()
            .filter(|r| r.file_type_id == file_type_id)
            .collect()
    }

    /// 编译为可判定规则集（globset；S12 已验证与 ripgrep 行为一致）。
    ///
    /// 合并语义（"对所有类型生效"）：`file_type_id = None` 的全局规则对**所有类型**均生效，
    /// 与指定类型的规则按 config 内的原始顺序合并，交给 `RuleSet` 做"后匹配覆盖先匹配"。
    /// `file_type_id = None` 查询时只取全局规则。
    pub fn compile_rules(&self, file_type_id: Option<i64>) -> anyhow::Result<RuleSet> {
        let refs: Vec<&DirRule> = self
            .dir_rules
            .iter()
            .filter(|r| r.file_type_id.is_none() || r.file_type_id == file_type_id)
            .collect();
        RuleSet::build(&refs)
    }

    /* ---------------------- 索引根目录 ---------------------- */

    pub fn add_root(&mut self, root: impl Into<String>) {
        let root = root.into();
        if !self.indexed_roots.contains(&root) {
            self.indexed_roots.push(root);
        }
    }

    pub fn remove_root(&mut self, root: &str) {
        self.indexed_roots.retain(|r| r != root);
    }
}

/// 编译后的三态规则集。
///
/// 判定顺序（后匹配覆盖先匹配，与 ripgrep 的 `--include/--exclude` 优先级一致）：
///   1. 依次遍历规则，记录最后一条命中的 `state`；
///   2. `Exclude` → 拒绝，`Include` → 接受，`Inherit` 或不命中 → 沿用默认（接受）。
#[derive(Debug, Clone)]
pub struct RuleSet {
    includes: Vec<(globset::GlobMatcher, usize)>,
    excludes: Vec<(globset::GlobMatcher, usize)>,
}

impl RuleSet {
    pub fn build(rules: &[&DirRule]) -> anyhow::Result<Self> {
        let mut includes = Vec::new();
        let mut excludes = Vec::new();
        for (i, r) in rules.iter().enumerate() {
            let matcher = globset::Glob::new(&r.pattern)?.compile_matcher();
            match r.state {
                RuleState::Include => includes.push((matcher, i)),
                RuleState::Exclude => excludes.push((matcher, i)),
                RuleState::Inherit => {}
            }
        }
        Ok(Self { includes, excludes })
    }

    /// 空规则集
    pub fn empty() -> Self {
        Self {
            includes: Vec::new(),
            excludes: Vec::new(),
        }
    }

    /// 该路径是否允许入库
    pub fn allows(&self, path: &Path) -> bool {
        let last_inc = self
            .includes
            .iter()
            .filter(|(m, _)| m.is_match(path))
            .map(|(_, i)| *i)
            .max();
        let last_exc = self
            .excludes
            .iter()
            .filter(|(m, _)| m.is_match(path))
            .map(|(_, i)| *i)
            .max();
        match (last_inc, last_exc) {
            (Some(i), Some(e)) => i > e,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_builtin_types() {
        let cfg = Config::default();
        assert!(cfg.file_types.iter().any(|t| t.name == "文本"));
        assert!(cfg.file_types.iter().all(|t| t.builtin));
        assert_eq!(cfg.schedule.mode, ScheduleMode::Never);
    }

    #[test]
    fn config_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        cfg.add_type("自定义", vec![".FOO".into(), "bar".into()]);
        cfg.set_rule(Some(1), "**/node_modules/**", RuleState::Exclude);
        cfg.add_root("/data/docs");
        cfg.schedule = ScheduleConfig {
            mode: ScheduleMode::Daily,
            interval_minutes: 30,
            daily_at: "02:30".into(),
        };
        cfg.save(dir.path()).unwrap();

        let back = Config::load(dir.path()).unwrap();
        let t = back.file_types.iter().find(|t| t.name == "自定义").unwrap();
        assert_eq!(t.exts, vec!["foo".to_string(), "bar".to_string()]);
        assert!(!t.builtin);
        assert_eq!(back.dir_rules.len(), 1);
        assert_eq!(back.indexed_roots, vec!["/data/docs".to_string()]);
        assert_eq!(back.schedule.daily_at, "02:30");
    }

    #[test]
    fn builtin_type_cannot_be_removed() {
        let mut cfg = Config::default();
        assert!(cfg.remove_type(1).is_err());
    }

    #[test]
    fn custom_type_crud() {
        let mut cfg = Config::default();
        let t = cfg.add_type("日志", vec!["logx".into()]);
        assert_eq!(cfg.exts_of("日志").unwrap(), vec!["logx".to_string()]);
        cfg.update_type(t.id, "日志2".into(), vec!["logy".into()])
            .unwrap();
        assert!(cfg.exts_of("日志").is_none());
        assert_eq!(cfg.exts_of("日志2").unwrap(), vec!["logy".to_string()]);
        cfg.remove_type(t.id).unwrap();
        assert!(cfg.exts_of("日志2").is_none());
    }

    #[test]
    fn ruleset_three_state_semantics() {
        let mut cfg = Config::default();
        // 先全目录排除 dist，再对代码类型强制包含 dist
        cfg.set_rule(None, "**/dist/**", RuleState::Exclude);
        cfg.set_rule(Some(1), "**/dist/**", RuleState::Include);

        let global = cfg.compile_rules(None).unwrap();
        assert!(!global.allows(Path::new("/p/dist/a.txt")));
        assert!(global.allows(Path::new("/p/src/a.txt")));

        let code = cfg.compile_rules(Some(1)).unwrap();
        assert!(code.allows(Path::new("/p/dist/a.txt")));

        // inherit 规则不参与判定
        cfg.set_rule(None, "**/tmp/**", RuleState::Inherit);
        let again = cfg.compile_rules(None).unwrap();
        assert!(again.allows(Path::new("/p/tmp/a.txt")));
    }

    #[test]
    fn set_rule_overwrites_same_key() {
        let mut cfg = Config::default();
        cfg.set_rule(Some(1), "**/a/**", RuleState::Exclude);
        cfg.set_rule(Some(1), "**/a/**", RuleState::Include);
        assert_eq!(cfg.dir_rules.len(), 1);
        assert_eq!(cfg.dir_rules[0].state, RuleState::Include);
    }

    #[test]
    fn remove_missing_rule_errors() {
        let mut cfg = Config::default();
        assert!(cfg.remove_rule(999).is_err());
    }

    #[test]
    fn empty_ruleset_allows_everything() {
        let rs = RuleSet::empty();
        assert!(rs.allows(Path::new("/any/path/a.txt")));
        assert!(rs.allows(Path::new("/any/path")));
    }

    #[test]
    fn later_rule_wins_regardless_of_direction() {
        // 先 include 后 exclude（同 pattern）→ exclude 覆盖
        let mut cfg = Config::default();
        cfg.set_rule(None, "**/x/**", RuleState::Include);
        cfg.set_rule(None, "**/x/**", RuleState::Exclude);
        assert!(!cfg.compile_rules(None).unwrap().allows(Path::new("/p/x/a")));

        // 先 exclude 后 include → include 覆盖
        let mut cfg2 = Config::default();
        cfg2.set_rule(None, "**/x/**", RuleState::Exclude);
        cfg2.set_rule(None, "**/x/**", RuleState::Include);
        assert!(cfg2
            .compile_rules(None)
            .unwrap()
            .allows(Path::new("/p/x/a")));
    }

    #[test]
    fn exclude_then_include_other_pattern_is_include() {
        // 两个不同 pattern 命中同一路径：后设置的（include）优先
        let mut cfg = Config::default();
        cfg.set_rule(None, "**/build/**", RuleState::Exclude);
        cfg.set_rule(None, "**/*.rs", RuleState::Include);
        let rs = cfg.compile_rules(None).unwrap();
        assert!(
            rs.allows(Path::new("/p/build/main.rs")),
            "Include 更晚 → 放行"
        );
        assert!(
            !rs.allows(Path::new("/p/build/main.txt")),
            "仅 Exclude 命中 → 拒绝"
        );
    }

    #[test]
    fn inherit_only_ruleset_allows() {
        let mut cfg = Config::default();
        cfg.set_rule(None, "**/tmp/**", RuleState::Inherit);
        cfg.set_rule(Some(1), "**/tmp/**", RuleState::Inherit);
        assert!(cfg
            .compile_rules(None)
            .unwrap()
            .allows(Path::new("/p/tmp/a")));
        assert!(cfg
            .compile_rules(Some(1))
            .unwrap()
            .allows(Path::new("/p/tmp/a")));
    }

    #[test]
    fn per_type_rules_merge_global_and_specific() {
        // 全局排除 node_modules；类型 1 覆盖为 include
        let mut cfg = Config::default();
        cfg.set_rule(None, "**/node_modules/**", RuleState::Exclude);
        cfg.set_rule(Some(1), "**/node_modules/**", RuleState::Include);
        // 类型 2 无自有规则 → 沿用全局排除
        assert!(!cfg
            .compile_rules(Some(2))
            .unwrap()
            .allows(Path::new("/p/node_modules/a")));
        // 类型 1 有覆盖规则 → 放行
        assert!(cfg
            .compile_rules(Some(1))
            .unwrap()
            .allows(Path::new("/p/node_modules/a")));
    }

    #[test]
    fn non_matching_paths_default_allow() {
        let mut cfg = Config::default();
        cfg.set_rule(None, "**/cache/**", RuleState::Exclude);
        let rs = cfg.compile_rules(None).unwrap();
        assert!(rs.allows(Path::new("/p/src/main.rs")));
        // 同级目录名相似但不命中 glob
        assert!(rs.allows(Path::new("/p/cache2/a")));
    }
}
