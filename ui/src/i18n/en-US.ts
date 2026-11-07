/**
 * English resources。key 与 zh-CN.ts 严格同增同删。
 */
const enUS: Record<string, string> = {
  /* MenuBar */
  'menu.menubar': 'Menu bar',
  'menu.file': 'File(F)',
  'menu.file.export': 'Export Results',
  'menu.file.exit': 'Exit',
  'menu.edit': 'Edit(E)',
  'menu.edit.copyPath': 'Copy Path',
  'menu.edit.copyName': 'Copy Name',
  'menu.edit.selectAll': 'Select All',
  'menu.edit.findInDoc': 'Find in Document',
  'menu.view': 'View(V)',
  'menu.view.themeLight': 'Light Theme',
  'menu.view.themeDark': 'Dark Theme',
  'menu.view.themeSystem': 'Follow System',
  'menu.view.prevPage': 'Previous Result',
  'menu.view.nextPage': 'Next Result',
  'menu.view.lineNumbers': 'Line Numbers',
  'menu.view.wrap': 'Wrap Lines',
  'menu.view.zoomIn': 'Zoom In',
  'menu.view.zoomOut': 'Zoom Out',
  'menu.view.zoomReset': 'Reset Font Size',
  'menu.settings': 'Options(O)',
  'menu.settings.options': 'Options…',
  'menu.settings.indexAdmin': 'Index Manager…',
  'menu.help': 'Help(H)',
  'menu.help.syntax': 'Advanced Search Syntax',
  'menu.help.homepage': 'Project Home',
  'menu.help.about': 'About EyzSearch',

  /* MenuBar: advanced search syntax panel */
  'syntax.title': 'Advanced Search Syntax',
  'syntax.header.op': 'Syntax',
  'syntax.header.meaning': 'Meaning',
  'syntax.header.example': 'Example',
  'syntax.and.name': 'AND',
  'syntax.and.desc': 'Documents must contain both keywords, e.g. error & timeout',
  'syntax.or.name': 'OR',
  'syntax.or.desc': 'Documents contain either keyword, e.g. cat | dog',
  'syntax.not.name': 'NOT',
  'syntax.not.desc': 'Exclude the keyword on the right, e.g. report !draft',
  'syntax.group.name': 'Group',
  'syntax.group.desc': 'Group sub-expressions, e.g. (a|b) & c',
  'syntax.exact.name': 'Exact',
  'syntax.exact.desc': 'Exact phrase match, e.g. "quantum computing"',
  'syntax.hint':
    'Syntax is available after switching the search mode to "Advanced"; multiple words are combined with AND by default.',

  /* StatusBar (composed in App.tsx; keys reserved for integrator) */
  'status.searching': 'Searching…',
  'status.ready': 'Ready',
  'status.results': '{1} results in {2} s, search complete.',
  'status.indexDocs': '{1} docs · {2}',
  'status.indexUnavailable': 'Index unavailable, please rebuild',
  'status.indexLoading': 'Loading index…',

  /* IndexAdminDialog: B4 find/browse add */
  'idx.addFind': 'Find & Add…',
  'idx.addBrowse': 'Browse & Add…',
  'idx.findTypePlaceholder': 'Type a file type name, e.g. chm',
  'idx.stagedTypes': 'Staged',
  'idx.stagedEmpty': 'Click an entry above to stage it',
  'idx.confirmAddTypes': 'OK',
  'idx.cancel': 'Cancel',
  'idx.unstage': 'Unstage',
  'idx.noMatch': 'No matching types',
  'idx.noExt': 'Cannot determine the extension from the file name',
  'idx.unsupported': 'Unsupported format: {1} (content extraction not supported yet)',
  'idx.addedTypes': 'Added {1} type(s)',

  /* FilterRow: D2 type description */
  'idx.allTypes': 'Any file type',

  /* SelectionBar: D1 delete all */
  'sel.deleteAll': 'Delete All',
  'sel.deleteAllConfirm':
    'This will delete all {1} search result files (moved to Recycle Bin, recoverable). Continue?',

  /* WelcomePage */
  'welcome.title': 'Welcome',
  'welcome.steps.title': 'Getting Started',
  'welcome.step1': 'Type any keyword into the search box on the left;',
  'welcome.step2': 'Click the "Start" button on the left, or press Enter, and results appear instantly;',
  'welcome.step3': 'Click an entry in the results to preview the file on the right;',
  'welcome.step4': 'Click the "+" button above to filter results by directory and file type.',
  'welcome.tips.title': 'Tips',
  'welcome.tip1':
    'Right-click any folder and choose "Search with EyzSearch" to search within it quickly;',
  'welcome.tip2': 'Enable the tray icon and global hotkey to summon the main window anytime;',
  'welcome.tip3': 'Hover over a result for 1 second to preview the matched text snippet;',
  'welcome.tip4': 'Customize accelerator keys under "Options → Hotkeys".',
  'welcome.more': '…More useful and efficient features are waiting for you to explore.',
  'welcome.about.title': 'About EyzSearch',
  'welcome.about.body1':
    'EyzSearch is a local file content search tool: local OCR search and content search across confidentiality-labelled documents, so your time goes to real work instead of hunting for files.',
  'welcome.about.body2':
    'EyzSearch supports Office, WPS, PDF, ebooks and more, with transparent decryption of .SJZ controlled documents and offline OCR (off by default). Indexing and search run entirely on your machine — no network, no uploads.',
  'welcome.about.author': 'Author: Eyzer   GitHub: https://github.com/Eyzer',
  'welcome.openHomepage': "Visit Author's GitHub",
  /* ---- 7.9 / B6: check for updates ---- */
  'update.check': 'Check for Updates',
  'update.checking': 'Checking for updates…',
  'update.latest': 'You are on the latest version ({version})',
  'update.available': 'New version available: {version} (current {current})',
  'update.availableNotes': 'Release notes:\n{notes}',
  'update.installConfirm': 'Version {version} is available. Update now?\nThe app will download the update and restart.',
  'update.installing': 'Downloading and installing the update. The app will restart shortly…',
  'update.failed': 'Update check failed: {error}',
  'update.notConfigured': 'Update channel not configured (enabled after the first stable release)',
}

export default enUS
