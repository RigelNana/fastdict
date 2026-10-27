export type Theme = 'system' | 'light' | 'dark'
export type ResourceKind = 'mdd' | 'css' | 'js' | 'font' | 'image' | 'audio' | 'file'
export interface Resource {
  id: string
  path: string
  kind: ResourceKind
  size: number
  enabled: boolean
  order: number
  key: string
}
export interface Metadata {
  title: string
  description: string
  format_version: string
  encoding: string
  encrypted: number
  key_blocks: number
  record_blocks: number
  attributes: Record<string, string>
  created: string | null
  dictionary_version: string | null
}
export interface IndexStatus {
  state: 'missing' | 'building' | 'ready' | 'error'
  indexed: number
  total: number
  error: string | null
}
export interface Dictionary {
  id: string
  name: string
  revision: string
  created: string
  enabled: boolean
  icon: string | null
  entries: number
  metadata: Metadata
  resources: Resource[]
  trust_scripts: boolean
  custom_css: string
  font_family: string
  typography_mode: 'original' | 'global' | 'override'
  warnings: string[]
  index_status: IndexStatus
}
export interface SortRule {
  field: 'name' | 'entries' | 'created'
  direction: 'asc' | 'desc'
}
export interface Group {
  id: string
  name: string
  dictionary_ids: string[]
  sort: SortRule[]
}
export interface Settings {
  application: { default_group: string; history_enabled: boolean; history_limit: number }
  appearance: {
    theme: Theme
    font_size: number
    line_height: number
    font_family: string
    content_width: number
    motion: boolean
    reading_mode: boolean
  }
  spelling: { enabled: boolean; distance: number }
  security: { external_links: 'ask' | 'allow' | 'block' }
  performance: { max_open_readers: number; cache_mb: number; query_workers: number }
}
export interface HistoryItem {
  id: string
  word: string
  group_id: string
  at: string
}
export interface LogItem {
  id: string
  at: string
  level: string
  scope: string
  message: string
}
export interface Job {
  id: string
  kind: string
  dictionary_id: string | null
  state: 'queued' | 'running' | 'done' | 'error'
  progress: number
  total: number
  error: string | null
}
export interface Bootstrap {
  dictionaries: Dictionary[]
  groups: Group[]
  settings: Settings
  history: HistoryItem[]
  stats: { words: number; open_readers: number; cache_bytes: number; jobs: Job[] }
}
export interface Suggestion {
  word: string
  dictionary_ids: string[]
  match_kind: 'exact' | 'prefix' | 'fuzzy' | 'fulltext'
  preview: string | null
}
export interface SuggestionResponse {
  items: Suggestion[]
  truncated: boolean
  elapsed_ms: number
}
export interface Article {
  html: string
  source_ids: number[]
  resolved_ids: number[]
  redirects: string[]
}
export interface LookupResponse {
  dictionary_id: string
  word: string
  articles: Article[]
  elapsed_ms: number
}
export interface ImportPreview {
  id: string
  files: { path: string; size: number; kind: ResourceKind | 'mdx' }[]
  candidates: { mdx: string; name: string; resources: string[]; warnings: string[] }[]
  warnings: string[]
}
export interface ResourceEntries {
  items: { path: string; size: number }[]
  truncated: boolean
}
export interface Connection {
  base_url: string
  token: string
}
