/**
 * 子项 1：搜索模式与 DSL —— 前端 invoke 封装
 * 所有权：子项 1 agent（其他子项勿改）；R3：分页响应
 */
import { invoke } from '@tauri-apps/api/tauri'
import type { SearchRequest, SearchResponse } from '../types'

/**
 * 统一搜索入口（R3 分页版）。
 * mode: fuzzy=模糊 / exact=精准 / dsl=高级 / regex=正则
 * reverse=true 时返回"不包含该关键词"的结果
 * page 从 0 计；mtimeFrom/mtimeTo 为 unix 秒闭区间，null = 不限
 */
export function search(req: SearchRequest): Promise<SearchResponse> {
  return invoke<SearchResponse>('search', { req })
}
