/**
 * R3：搜索历史 / 书签 —— 前端 invoke 封装
 * DB 文件由后端定位（索引目录 search-tool.db）
 */
import { invoke } from '@tauri-apps/api/tauri'
import type { Bookmark, HistoryEntry } from '../types'

/** 最近搜索历史（新在前，默认 10 条） */
export function listHistory(limit = 10): Promise<HistoryEntry[]> {
  return invoke<HistoryEntry[]>('list_history', { limit })
}

export function clearHistory(): Promise<void> {
  return invoke('clear_history')
}

export function addBookmark(path: string, name: string): Promise<void> {
  return invoke('add_bookmark', { path, name })
}

export function listBookmarks(): Promise<Bookmark[]> {
  return invoke<Bookmark[]>('list_bookmarks')
}

export function removeBookmark(path: string): Promise<void> {
  return invoke('remove_bookmark', { path })
}
