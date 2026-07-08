import { useCallback, useMemo } from "react";
import { useSearchParams } from "react-router-dom";
import type { ItemSort, ItemStatus, ItemType, ItemWhen } from "@/lib/types";

/** The feed's facet + pagination state. The URL query is the single source of truth (§9.1). */
export interface FeedFilters {
  type: ItemType;
  status: ItemStatus;
  cat: number | "all";
  when: ItemWhen;
  sort: ItemSort;
  page: number;
  q: string;
}

const DEFAULTS: FeedFilters = {
  type: "all",
  status: "all",
  cat: "all",
  when: "all",
  sort: "new",
  page: 1,
  q: "",
};

function parse(params: URLSearchParams): FeedFilters {
  const catRaw = params.get("cat");
  return {
    type: (params.get("type") as ItemType) || DEFAULTS.type,
    status: (params.get("status") as ItemStatus) || DEFAULTS.status,
    cat: catRaw && catRaw !== "all" ? Number(catRaw) || "all" : "all",
    when: (params.get("when") as ItemWhen) || DEFAULTS.when,
    sort: (params.get("sort") as ItemSort) || DEFAULTS.sort,
    page: Math.max(1, Number(params.get("page")) || 1),
    q: params.get("q") ?? "",
  };
}

/** Facets that count toward the "active filters" badge (everything except sort/page/q). */
export const FACET_KEYS = ["type", "status", "cat", "when"] as const;

export function activeFilterCount(f: FeedFilters): number {
  let n = 0;
  if (f.type !== DEFAULTS.type) n++;
  if (f.status !== DEFAULTS.status) n++;
  if (f.cat !== DEFAULTS.cat) n++;
  if (f.when !== DEFAULTS.when) n++;
  return n;
}

/**
 * Read/write the feed filters through the URL. Any facet change resets to page 1 (§9.1); page
 * changes preserve facets. `includeQuery` keeps the `q` param (Search screen) — the feed screen
 * drops it so its URL stays clean.
 */
export function useFeedFilters(includeQuery = false) {
  const [params, setParams] = useSearchParams();
  const filters = useMemo(() => parse(params), [params]);

  const write = useCallback(
    (next: FeedFilters) => {
      const p = new URLSearchParams();
      if (next.type !== DEFAULTS.type) p.set("type", next.type);
      if (next.status !== DEFAULTS.status) p.set("status", next.status);
      if (next.cat !== DEFAULTS.cat) p.set("cat", String(next.cat));
      if (next.when !== DEFAULTS.when) p.set("when", next.when);
      if (next.sort !== DEFAULTS.sort) p.set("sort", next.sort);
      if (next.page !== 1) p.set("page", String(next.page));
      if (includeQuery && next.q) p.set("q", next.q);
      setParams(p, { replace: false });
    },
    [includeQuery, setParams],
  );

  const setFacet = useCallback(
    <K extends keyof FeedFilters>(key: K, value: FeedFilters[K]) => {
      // Facet/sort/query changes reset pagination; page changes don't.
      write({ ...filters, [key]: value, page: key === "page" ? (value as number) : 1 });
    },
    [filters, write],
  );

  const setPage = useCallback((page: number) => write({ ...filters, page }), [filters, write]);

  const clear = useCallback(() => write({ ...DEFAULTS, sort: filters.sort, q: filters.q }), [filters, write]);

  return { filters, setFacet, setPage, clear };
}

/** Serialize filters into the `/api/items` query string. */
export function toItemsQuery(f: FeedFilters): string {
  const p = new URLSearchParams();
  if (f.type !== "all") p.set("type", f.type);
  if (f.status !== "all") p.set("status", f.status);
  if (f.cat !== "all") p.set("category", String(f.cat));
  if (f.when !== "all") p.set("when", f.when);
  p.set("sort", f.sort);
  p.set("page", String(f.page));
  if (f.q.trim()) p.set("q", f.q.trim());
  const s = p.toString();
  return s ? `?${s}` : "";
}
