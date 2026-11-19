import type { Dictionary, Resource } from './types'

export const maxAutoProbeMddBytes = 64 * 1024 * 1024

function fileStem(path: string) {
  const filename = path.split(/[\\/]/).at(-1) ?? path
  const dot = filename.lastIndexOf('.')
  return (dot > 0 ? filename.slice(0, dot) : filename).trim().toLowerCase()
}

export function iconCandidates(dictionary: Pick<Dictionary, 'name' | 'icon' | 'resources'>): {
  resource: Resource
  path: string
}[] {
  const selected = dictionary.icon
  const images = dictionary.resources.filter((item) => item.enabled && item.kind === 'image')
  if (selected) {
    const direct = images.find(
      (item) => item.id === selected || item.key === selected || item.path === selected,
    )
    if (direct) return [{ resource: direct, path: '' }]
    return dictionary.resources
      .filter((item) => item.enabled && item.kind === 'mdd')
      .map((item) => ({ resource: item, path: selected }))
  }
  const named = images.find((item) =>
    /(?:^|[\\/])(?:icon|favicon|logo|cover)\.(?:png|jpe?g|svg|ico|webp|gif)$/i.test(item.path),
  )
  if (named) return [{ resource: named, path: '' }]
  const dictStem = dictionary.name.trim().toLowerCase()
  const stemMatched = images.find((item) => fileStem(item.path) === dictStem)
  if (stemMatched) return [{ resource: stemMatched, path: '' }]
  if (images.length === 1) return [{ resource: images[0], path: '' }]
  // Avoid scanning multi-gigabyte media archives when no icon was configured.
  return dictionary.resources
    .filter((item) => item.enabled && item.kind === 'mdd' && item.size <= maxAutoProbeMddBytes)
    .slice(0, 2)
    .map((item) => ({ resource: item, path: 'icon.png' }))
}
