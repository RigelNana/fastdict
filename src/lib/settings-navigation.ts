import { BookOpen, Palette, ServerCog, SlidersHorizontal } from 'lucide-react'

export const settingsCategories = [
  { id: 'dictionaries', label: '词典与分组', icon: BookOpen },
  { id: 'appearance', label: '外观与排版', icon: Palette },
  { id: 'preferences', label: '查词偏好', icon: SlidersHorizontal },
  { id: 'system', label: '服务与维护', icon: ServerCog },
] as const

export type SettingsCategoryId = (typeof settingsCategories)[number]['id']

export function resolveSettingsCategory(segment?: string): SettingsCategoryId {
  switch (segment) {
    case 'dictionaries':
    case 'indexes':
      return 'dictionaries'
    case 'appearance':
      return 'appearance'
    case 'preferences':
    case 'app':
    case 'spelling':
      return 'preferences'
    case 'system':
    case 'security':
    case 'performance':
    case 'backup':
    case 'logs':
      return 'system'
    default:
      return 'dictionaries'
  }
}
