/**
 * 简体中文资源（源语言）。key 命名 `组件.区块.条目`；与 en-US.ts 严格同 key。
 */
const zhCN: Record<string, string> = {
  /* MenuBar */
  'menu.menubar': '菜单栏',
  'menu.file': '文件(F)',
  'menu.file.export': '导出结果',
  'menu.file.exit': '退出',
  'menu.edit': '编辑(E)',
  'menu.edit.copyPath': '复制路径',
  'menu.edit.copyName': '复制名称',
  'menu.edit.selectAll': '全选结果',
  'menu.edit.findInDoc': '文档内查找',
  'menu.view': '查看(V)',
  'menu.view.themeLight': '浅色皮肤',
  'menu.view.themeDark': '深色皮肤',
  'menu.view.themeSystem': '跟随系统',
  'menu.view.prevPage': '上一个结果',
  'menu.view.nextPage': '下一个结果',
  'menu.view.lineNumbers': '显示行号',
  'menu.view.wrap': '自动换行',
  'menu.view.zoomIn': '放大',
  'menu.view.zoomOut': '缩小',
  'menu.view.zoomReset': '重置字号',
  'menu.settings': '设置(O)',
  'menu.settings.options': '选项…',
  'menu.settings.indexAdmin': '索引管理…',
  'menu.help': '帮助(H)',
  'menu.help.syntax': '高级搜索语法速查',
  'menu.help.homepage': '项目主页',
  'menu.help.about': '关于 EyzSearch',

  /* MenuBar：高级搜索语法速查面板 */
  'syntax.title': '高级搜索语法',
  'syntax.header.op': '语法',
  'syntax.header.meaning': '含义',
  'syntax.header.example': '示例说明',
  'syntax.and.name': '与',
  'syntax.and.desc': '同时包含左右两侧关键词，如：error & timeout',
  'syntax.or.name': '或',
  'syntax.or.desc': '包含任意一侧关键词，如：cat | dog',
  'syntax.not.name': '非',
  'syntax.not.desc': '排除右侧关键词，如：report !draft',
  'syntax.group.name': '分组',
  'syntax.group.desc': '对子表达式分组，如：(a|b) & c',
  'syntax.exact.name': '精确',
  'syntax.exact.desc': '精确短语匹配，如："中华人民共和国"',
  'syntax.hint': '语法可在搜索模式切到「高级」后使用；多个词默认按「与」处理。',

  /* StatusBar（文案在 App.tsx 组合，key 预留给集成方） */
  'status.searching': '正在搜索…',
  'status.ready': '就绪',
  'status.results': '返回 {1} 个结果，耗时 {2} 秒，搜索完成。',
  'status.indexDocs': '{1} 篇 · {2}',
  'status.indexUnavailable': '索引不可用，请重建',
  'status.indexLoading': '索引加载中…',

  /* IndexAdminDialog：B4 查找/浏览添加 */
  'idx.addFind': '查找添加…',
  'idx.addBrowse': '浏览添加…',
  'idx.findTypePlaceholder': '输入类型名，如 chm',
  'idx.stagedTypes': '已暂存',
  'idx.stagedEmpty': '点击上方列表暂存类型',
  'idx.confirmAddTypes': '确定',
  'idx.cancel': '取消',
  'idx.unstage': '取消暂存',
  'idx.noMatch': '无匹配类型',
  'idx.noExt': '无法从文件名识别扩展名',
  'idx.unsupported': '不支持的格式：{1}（该格式暂不支持内容提取）',
  'idx.addedTypes': '已新增 {1} 个类型',

  /* FilterRow：D2 类型描述 */
  'idx.allTypes': '不限文件类型',

  /* SelectionBar：D1 全部删除 */
  'sel.deleteAll': '全部删除',
  'sel.deleteAllConfirm': '将删除全部 {1} 个搜索结果文件（移至回收站，可恢复）。确定继续？',

  /* WelcomePage */
  'welcome.title': '欢迎使用',
  'welcome.steps.title': '搜索步骤',
  'welcome.step1': '在左边搜索框内输入想要搜索的任意关键词；',
  'welcome.step2': '点击左边的「开始」按钮，或者按回车，搜索结果将会立刻呈现；',
  'welcome.step3': '点击搜索结果中的条目，右侧将生成该文件的预览；',
  'welcome.step4': '点击上方的「+」按钮，还可以按目录与文件类型过滤结果。',
  'welcome.tips.title': '使用技巧',
  'welcome.tip1': '右击任意目录选择「用 EyzSearch 搜索」，可以快速按该目录搜索；',
  'welcome.tip2': '开启托盘与全局热键，随时唤起主窗口进行搜索；',
  'welcome.tip3': '搜索结果列表鼠标悬停 1 秒，可快速预览命中的文字片段；',
  'welcome.tip4': '在「设置 → 快捷键」中可自定义唤醒等操作的加速键。',
  'welcome.more': '……更多实用、高效的功能等待你的探索。',
  'welcome.about.title': '关于 EyzSearch 全文搜索',
  'welcome.about.body1':
    'EyzSearch 是一款本地文件全文搜索工具：支持本地 OCR 检索与带保密标签文件的内容检索，把时间留给正事，别耗在找文件上。',
  'welcome.about.body2':
    'EyzSearch 支持 Office、WPS、PDF、电子书等常见格式，支持 .SJZ 加密受控文档透明解密与离线 OCR 文字识别（默认关闭）。索引与检索全部在本机完成，不联网、不上传任何内容。',
  'welcome.about.author': '作者：Eyzer　GitHub：https://github.com/Eyzer',
  'welcome.openHomepage': '访问作者 GitHub',
  /* ---- 7.9 / B6：检测更新 ---- */
  'update.check': '检测更新',
  'update.checking': '正在检查更新…',
  'update.latest': '当前已是最新版本（{version}）',
  'update.available': '发现新版本 {version}（当前 {current}）',
  'update.availableNotes': '更新说明：\n{notes}',
  'update.installConfirm': '发现新版本 {version}，是否现在更新？\n应用将下载并自动重启。',
  'update.installing': '正在下载并安装更新，应用即将重启…',
  'update.failed': '更新检查失败：{error}',
  'update.notConfigured': '更新通道未配置（首个正式版发布后启用）',
}

export default zhCN
