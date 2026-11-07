/**
 * 子项 4：预览交互全家桶 —— 前端 invoke 封装
 * 所有权：子项 4 agent（其他子项勿改）
 */
import { invoke } from '@tauri-apps/api/tauri'
import type {
  DocumentPayload,
  FileInfo,
  FindMatch,
  FindOptions,
  MatchGroup,
  PageTextBox,
  SearchMode,
  WordStat,
} from '../types'

/** 读文件全文 + 元信息（行数/字节数/截断标志） */
export function getDocument(path: string): Promise<DocumentPayload> {
  return invoke<DocumentPayload>('get_document', { path })
}

/** 词组统计（后端 jieba 词频，降序，取前 N；minLen 为最小字数） */
export function wordStats(
  path: string,
  mode: SearchMode,
  topN = 200,
  minLen?: number,
): Promise<WordStat[]> {
  return invoke<WordStat[]>('word_stats', { path, mode, topN, minLen })
}

/**
 * 匹配列表：按查询词项分组，offset 与高亮/minimap 三段共用（aho-corasick）
 */
export function matchGroups(path: string, query: string, mode: SearchMode): Promise<MatchGroup[]> {
  return invoke<MatchGroup[]>('match_groups', { path, query, mode })
}

/** 文档内再搜索（大小写 / 全词 / 正则三选项） */
export function findInDoc(path: string, needle: string, opts: FindOptions): Promise<FindMatch[]> {
  return invoke<FindMatch[]>('find_in_doc', { path, needle, opts })
}

/** 文件信息 Tab */
export function getFileInfo(path: string): Promise<FileInfo> {
  return invoke<FileInfo>('get_file_info', { path })
}

/* ---------------- P4-D：OOXML 文档视图 ---------------- */

/** 读取文件原始字节（前端 WASM 渲染器用；后端带 50MB 闸门） */
export function readFileBytes(path: string): Promise<number[]> {
  return invoke<number[]>('read_file_bytes', { path })
}

/* ---------------- P3-B：PDF 页视图（渲染 + 命中 bbox） ---------------- */

/** PDF 页数（非 PDF 或无 PDFium 时报错，前端据此隐藏页视图） */
export function pdfPageCount(path: string): Promise<number> {
  return invoke<number>('pdf_page_count', { path })
}

/** 渲染指定页为 PNG 字节（前端转 Blob URL） */
export function renderPdfPage(
  path: string,
  pageIndex: number,
  targetWidth = 800,
): Promise<number[]> {
  return invoke<number[]>('render_pdf_page', { path, pageIndex, targetWidth })
}

/** 指定页的文本块（归一化 bbox，供命中框叠加） */
export function pdfPageBoxes(path: string, pageIndex: number): Promise<PageTextBox[]> {
  return invoke<PageTextBox[]>('pdf_page_boxes', { path, pageIndex })
}
