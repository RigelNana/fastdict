import type { Resource } from './types'

export function moveResourceWithinKind(resources: Resource[], id: string, direction: -1 | 1) {
  const from = resources.findIndex((resource) => resource.id === id)
  if (from < 0) return resources
  const peers = resources.flatMap((resource, index) => resource.kind === resources[from].kind ? [index] : [])
  const to = peers[peers.indexOf(from) + direction]
  if (to === undefined) return resources
  const next = [...resources]
  ;[next[from], next[to]] = [next[to], next[from]]
  return next.map((resource, order) => ({ ...resource, order }))
}
