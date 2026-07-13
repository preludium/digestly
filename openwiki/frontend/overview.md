# Frontend

One React app (`web/`) powers the browser, the installed PWA, and (in future) Tauri. The frontend is built to static assets and served by the Rust binary via `tower-http::ServeDir`.

**Tech stack:** React 19, TypeScript, Vite 8, TanStack Query 5, Zustand 5, Tailwind 4, Radix UI primitives (shadcn-style), React Router 7, Lucide icons, Biome (linting/formatting)

## Build & dev

```bash
cd web
pnpm install
pnpm dev          # Vite dev server on http://localhost:5173
pnpm build        # TypeScript check + Vite production build → web/dist/
pnpm test         # Vitest
pnpm format       # Biome format
pnpm check        # Biome lint + format
```

The Vite dev server proxies `/api` to the Rust backend (see `web/vite.config.ts`).

## Route map (`web/src/App.tsx`)

| Path | Component | Auth | Notes |
|------|-----------|------|-------|
| `/login` | `Login` | Public (redirect if logged in) | Password + passkey login |
| `/register` | `Register` | Public (redirect if logged in) | Self-registration (gated by `allow_registration`) |
| `/` | `Feed` | Required | Main card-grid reader with filter bar |
| `/manage` | `Manage` | Required | Category + feed management |
| `/digests` | `Digests` | Required | Digest history list |
| `/digests/:id` | `DigestDetail` | Required | Single digest view |
| `/health` | `Health` | Required | Feed health dashboard |
| `/settings` | `Settings` | Required | Per-user + admin settings |
| `/profile` | `Profile` | Required | Passkey management, account info |
| `/admin/users` | `AdminUsers` | Admin only | User management |
| `/admin/system` | `System` | Admin only | System stats + config |

## App shell (`web/src/components/AppShell.tsx`)

Sidebar-based layout using shadcn sidebar components:
- **Sidebar header:** app name + "Add feed" button
- **Navigation:** Feed, Manage, Digests, Feed Health (with unhealthy count badge), Settings
- **Sidebar footer:** admin section (Users, System — admin only), theme toggle, profile link, logout
- **Content area:** `<Outlet />` renders the matched route

## Component tree

```
App
├── AppBanners (offline, update-available)
├── OnboardingGate (first-run category setup)
└── Routes
    ├── Login / Register (unauthenticated)
    └── AppShell (authenticated)
        ├── Sidebar
        │   ├── AddFeedModal
        │   └── Navigation + theme/profile/logout
        └── Outlet
            ├── Feed → ItemGrid → ItemCard, FilterBar, ItemPreview
            ├── Manage → category list, FeedEditModal
            ├── Digests / DigestDetail
            ├── Health → feed status table
            ├── Settings → GeneralSettings, AiSettings, NotificationsSettings, etc.
            ├── Profile → PasskeyManager
            ├── AdminUsers
            └── System
```

### Key component directories

| Directory | Contents |
|-----------|----------|
| `components/ui/` | shadcn-style primitives: button, card, dialog, input, select, badge, tabs, checkbox, switch, skeleton, spinner, sidebar, tooltip, dropdown-menu, etc. |
| `components/common/` | Shared: AuthCard, ConfirmDialog, EmptyState, ErrorBanner, LoadingSkeleton, NameDialog, Markdown, Pagination, Toaster |
| `components/feeds/` | AddFeedModal, FeedEditModal |
| `components/items/` | ItemCard, ItemGrid, ItemPreview, FilterBar |
| `components/settings/` | AddProviderModal, AiSettings, ConnectedAccounts, DigestSettings, GeneralSettings, ImportExport, IngestionSettings, NotificationsSettings, SettingsTile |

## State management

- **TanStack Query** (`@tanstack/react-query`): all server state — reads and mutations via custom hooks in `web/src/hooks/`. Configured in `web/src/lib/queryClient.ts`.
- **Zustand** (`web/src/stores/`): ephemeral UI state only — toast notifications (`toast.ts`), theme, sidebar state (`ui.ts`).

## Data hooks (`web/src/hooks/`)

Each hook wraps TanStack Query for a domain:

| Hook | Domain |
|------|--------|
| `useAuth`, `useMe`, `useLogout` | Authentication |
| `useFeeds`, `useFeedFilters` | Feed subscriptions |
| `useItems`, `useUnreadCount` | Items/cards |
| `useCategories` | Categories |
| `useDigest` | Digest history + generation |
| `useSettings` | Per-user preferences |
| `useAi` | AI providers (admin) |
| `useAdmin` | User management (admin) |
| `useNotifications` | ntfy config |
| `useOauth` | OAuth import (YouTube/Reddit) |
| `useOpml` | OPML import/export |
| `usePasskeys` | WebAuthn passkey management |

## API client (`web/src/lib/api.ts`)

A thin wrapper around `fetch` with:
- Base URL from Vite env or same-origin
- JSON request/response handling
- Credential inclusion (cookies)
- Error extraction from JSON `{ error: "..." }` responses

## URL-encoded filters

The main Feed page encodes all filter state in the URL (`?type=&status=&when=&cat=&sort=&page=`), so state survives refresh/back and is shareable. The `FilterBar` component reads and writes these params.

## PWA (`web/src/lib/pwa.ts`, `web/public/sw.js`, `web/public/manifest.webmanifest`)

- **Service worker** (`sw.js`): caches app-shell assets (HTML, JS, CSS) and API responses for cached items. Registered on load via `pwa.ts`.
- **Update flow:** when a new service worker is detected, an "update available" banner appears (via `AppBanners`). Accepting reloads with the new version.
- **Offline reading:** cached items are viewable offline. The offline banner (`AppBanners`) appears when the API is unreachable.
- **`manifest.webmanifest`:** provides app name, icons (192px, 512px), theme color, and `display: standalone` for installability.

## Offline write-sync (`web/src/lib/outbox.ts`, `web/src/lib/sync.ts`)

Read/star mutations made offline are applied optimistically to the query cache and appended to a persistent **outbox** (localStorage-backed):

- Each entry carries an _explicit_ value (is_read: true/false, is_starred: true/false)
- Before replay, the outbox coalesces per `(kind, item)` to the latest intent — replay is idempotent and last-write-wins
- Flushing is driven on the `online` event, on app start, and by the service worker's Background Sync (`sync` tag `hf-outbox` → `postMessage` to clients) where supported
- Server read/star endpoints are already idempotent explicit-value upserts — no server change needed
- Unit-tested with Vitest (`outbox.test.ts`); server convergence covered by a Rust isolation test

## UI design system

- **Tailwind 4** with design tokens; no raw hex/arbitrary values in components
- **shadcn-style components** using Radix UI primitives (`@radix-ui/react-*`)
- **class-variance-authority** (`cva`) for variant-based component styling
- **tailwind-merge** + `clsx` for conditional class merging (`web/src/lib/utils.ts`)
- **Lucide React** for icons
- **react-markdown** for rendering markdown content (AI summaries, digest content)
- **topicColor** (`web/src/lib/topicColor.ts`, `web/src/lib/topicColor.test.ts`): deterministic pastel color assignment for category/topic badges
- Two fonts: Fraunces (display) and Instrument Sans (body), via `@fontsource-variable`
