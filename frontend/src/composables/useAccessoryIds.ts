/**
 * Parse a namespaced accessory ID into its groupKey and local itemId.
 * Backward-compatible: IDs without ":" return groupKey=undefined.
 *
 * Namespaced:  "pocket:mount"  → { groupKey: "pocket", itemId: "mount" }
 * Legacy flat: "mount"         → { groupKey: undefined, itemId: "mount" }
 */
export function parseAccessoryId(id: string): { groupKey?: string; itemId: string } {
  const colon = id.indexOf(':')
  if (colon === -1) return { itemId: id }
  return { groupKey: id.slice(0, colon), itemId: id.slice(colon + 1) }
}
