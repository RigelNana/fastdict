import { BookOpen, Database, Download, FileText, Monitor, Palette, ShieldCheck, SpellCheck } from 'lucide-react'

export const settingsCategories = [
  { id: 'app', label: '通用', icon: Monitor },
  { id: 'appearance', label: '外观', icon: Palette },
  { id: 'dictionaries', label: '词典', icon: BookOpen },
  { id: 'security', label: '连接与安全', icon: ShieldCheck },
  { id: 'spelling', label: '拼写', icon: SpellCheck },
  { id: 'backup', label: '备份', icon: Download },
  { id: 'logs', label: '日志', icon: FileText },
  { id: 'performance', label: '性能', icon: Database },
] as const
