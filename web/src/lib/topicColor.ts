/** Stable pastel badge classes for a topic/category name. Tailwind needs literal
 *  class names, so the palette is a static list; the hash picks an index. */
const PALETTE = [
  "bg-badge-1 text-badge-1-foreground",
  "bg-badge-2 text-badge-2-foreground",
  "bg-badge-3 text-badge-3-foreground",
  "bg-badge-4 text-badge-4-foreground",
  "bg-badge-5 text-badge-5-foreground",
  "bg-badge-6 text-badge-6-foreground",
  "bg-badge-7 text-badge-7-foreground",
  "bg-badge-8 text-badge-8-foreground",
] as const;

export function topicBadgeClass(name: string): string {
  let hash = 0;
  for (let i = 0; i < name.length; i++) hash = (hash * 31 + name.charCodeAt(i)) | 0;
  return PALETTE[Math.abs(hash) % PALETTE.length];
}
