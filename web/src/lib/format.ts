import type { FeedKind } from "@/lib/types";

/** Source-type glyph for a feed kind (prompt.md §9.5: 📄 RSS · 🎬 YouTube · 👽 Reddit). */
export function kindIcon(kind: FeedKind): string {
  switch (kind) {
    case "youtube":
      return "🎬";
    case "reddit":
      return "👽";
    default:
      return "📄";
  }
}

export function kindLabel(kind: FeedKind): string {
  switch (kind) {
    case "youtube":
      return "YouTube";
    case "reddit":
      return "Reddit";
    case "atom":
      return "Atom";
    case "jsonfeed":
      return "JSON";
    default:
      return "RSS";
  }
}

/** Render a stored UTC timestamp ("YYYY-MM-DD HH:MM:SS") in the browser's locale, or "never". */
export function formatDateTime(value: string | null | undefined): string {
  if (!value) return "never";
  const iso = value.includes("T") ? value : value.replace(" ", "T") + "Z";
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? value : d.toLocaleString();
}

/** Parse a stored UTC timestamp into a Date, or null. */
function toDate(value: string | null | undefined): Date | null {
  if (!value) return null;
  const iso = value.includes("T") ? value : value.replace(" ", "T") + "Z";
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? null : d;
}

/** Compact relative time for cards ("just now", "3h", "2d", or a date). */
export function relativeTime(value: string | null | undefined): string {
  const d = toDate(value);
  if (!d) return "";
  const secs = Math.round((Date.now() - d.getTime()) / 1000);
  if (secs < 60) return "just now";
  const mins = Math.round(secs / 60);
  if (mins < 60) return `${mins}m`;
  const hrs = Math.round(mins / 60);
  if (hrs < 24) return `${hrs}h`;
  const days = Math.round(hrs / 24);
  if (days < 7) return `${days}d`;
  return d.toLocaleDateString();
}

/** Reading-time pill (§9.1): "📖 6 min" for reading, "🎬 2 min read" for video. */
export function readingTimeLabel(secs: number | null, contentType: "reading" | "video"): string | null {
  if (!secs || secs <= 0) return contentType === "video" ? "🎬 video" : null;
  const mins = Math.max(1, Math.round(secs / 60));
  return contentType === "video" ? `🎬 ${mins} min read` : `📖 ${mins} min`;
}

/** Video duration badge ("mm:ss" or "h:mm:ss"). */
export function formatDuration(secs: number | null): string | null {
  if (!secs || secs <= 0) return null;
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = secs % 60;
  const mm = h > 0 ? String(m).padStart(2, "0") : String(m);
  const ss = String(s).padStart(2, "0");
  return h > 0 ? `${h}:${mm}:${ss}` : `${mm}:${ss}`;
}
