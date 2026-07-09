import { LayoutGrid, FolderCog, Newspaper, HeartPulse, Settings, User as UserIcon, Users, Menu, LogOut, Moon, Sun, Search, RefreshCw } from "lucide-react";
import { NavLink, Outlet, useNavigate } from "react-router-dom";
import { Button } from "@/components/ui/button";
import { Sheet, SheetContent } from "@/components/ui/sheet";
import { AddFeedModal } from "@/components/feeds/AddFeedModal";
import { cn } from "@/lib/utils";
import type { User } from "@/lib/types";
import { useLogout } from "@/hooks/useAuth";
import { useUnhealthyCount, useRefreshAll } from "@/hooks/useFeeds";
import { useUnreadCount } from "@/hooks/useItems";
import { useUiStore } from "@/stores/ui";
import { toast } from "@/stores/toast";

interface NavItem {
  to: string;
  label: string;
  icon: React.ComponentType<{ className?: string }>;
}

const MAIN_NAV: NavItem[] = [
  { to: "/", label: "All items", icon: LayoutGrid },
  { to: "/manage", label: "Manage", icon: FolderCog },
  { to: "/digests", label: "Digests", icon: Newspaper },
  { to: "/health", label: "Feed health", icon: HeartPulse },
  { to: "/settings", label: "Settings", icon: Settings },
  { to: "/profile", label: "Profile", icon: UserIcon },
];

const ADMIN_NAV: NavItem[] = [
  { to: "/admin/users", label: "Users", icon: Users },
];

function NavItemRow({ item, onNavigate }: { item: NavItem; onNavigate?: () => void }) {
  const unhealthy = useUnhealthyCount();
  return (
    <NavLink
      key={item.to}
      to={item.to}
      end={item.to === "/"}
      onClick={onNavigate}
      className={({ isActive }) =>
        cn(
          "flex items-center gap-3 rounded-md px-3 py-2 text-sm font-medium transition-colors",
          isActive ? "bg-primary/10 text-primary" : "text-muted-foreground hover:bg-muted hover:text-foreground",
        )
      }
    >
      <item.icon className="size-4" />
      <span className="flex-1">{item.label}</span>
      {item.to === "/health" && unhealthy > 0 && (
        <span className="flex size-5 items-center justify-center rounded-full bg-destructive text-xs font-semibold text-destructive-foreground">
          {unhealthy}
        </span>
      )}
    </NavLink>
  );
}

function NavList({ isAdmin, onNavigate }: { isAdmin: boolean; onNavigate?: () => void }) {
  return (
    <nav className="flex flex-col gap-1">
      {MAIN_NAV.map((n) => (
        <NavItemRow key={n.to} item={n} onNavigate={onNavigate} />
      ))}
      {isAdmin && (
        <>
          <p className="mt-4 px-3 text-xs font-semibold uppercase tracking-wider text-muted-foreground">Admin</p>
          {ADMIN_NAV.map((n) => (
            <NavItemRow key={n.to} item={n} onNavigate={onNavigate} />
          ))}
        </>
      )}
    </nav>
  );
}

function Footer({ user }: { user: User }) {
  const navigate = useNavigate();
  const logout = useLogout();
  return (
    <div className="mt-auto flex items-center justify-between gap-2 border-t border-border pt-3">
      <div className="min-w-0">
        <p className="truncate text-sm font-medium">{user.username}</p>
        <p className="text-xs text-muted-foreground">{user.role}</p>
      </div>
      <Button
        variant="ghost"
        size="icon"
        aria-label="Log out"
        onClick={() =>
          logout.mutate(undefined, {
            onSuccess: () => {
              toast("Signed out");
              navigate("/login");
            },
          })
        }
      >
        <LogOut className="size-4" />
      </Button>
    </div>
  );
}

function ThemeToggle() {
  const theme = useUiStore((s) => s.theme);
  const setTheme = useUiStore((s) => s.setTheme);
  return (
    <Button
      variant="outline"
      size="icon"
      aria-label="Toggle theme"
      onClick={() => setTheme(theme === "dark" ? "light" : "dark")}
    >
      {theme === "dark" ? <Sun className="size-4" /> : <Moon className="size-4" />}
    </Button>
  );
}

/** App chrome around authed routes (prompt.md §9.0). */
export function AppShell({ user }: { user: User }) {
  const drawerOpen = useUiStore((s) => s.drawerOpen);
  const setDrawerOpen = useUiStore((s) => s.setDrawerOpen);
  const isAdmin = user.role === "admin";
  const navigate = useNavigate();
  const refreshAll = useRefreshAll();
  const unread = useUnreadCount();

  return (
    <div className="flex min-h-dvh flex-col">
      {/* Top bar */}
      <header className="sticky top-0 z-30 flex items-center gap-3 border-b border-border bg-card px-3 py-2.5">
        <Button
          variant="ghost"
          size="icon"
          className="lg:hidden"
          aria-label="Open menu"
          onClick={() => setDrawerOpen(true)}
        >
          <Menu className="size-5" />
        </Button>
        <button
          type="button"
          onClick={() => navigate("/")}
          className="flex items-center gap-2 font-display text-lg font-bold tracking-tight"
        >
          Digestly
          {unread > 0 && (
            <span className="flex min-w-5 items-center justify-center rounded-full bg-primary px-1.5 text-xs font-semibold text-primary-foreground">
              {unread}
            </span>
          )}
        </button>
        <div className="ml-auto flex items-center gap-2">
          <Button variant="ghost" size="icon" aria-label="Search" onClick={() => navigate("/search")}>
            <Search className="size-5" />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            aria-label="Refresh all feeds"
            disabled={refreshAll.isPending}
            onClick={() => refreshAll.mutate(undefined, { onSuccess: () => toast("Refreshing feeds…") })}
          >
            <RefreshCw className={cn("size-5", refreshAll.isPending && "animate-spin")} />
          </Button>
          <ThemeToggle />
        </div>
      </header>

      <div className="flex flex-1">
        {/* Persistent sidebar ≥ lg */}
        <aside className="sticky top-0 hidden h-dvh w-64 shrink-0 flex-col gap-2 overflow-y-auto border-r border-border p-4 lg:flex">
          <NavList isAdmin={isAdmin} />
          <Footer user={user} />
        </aside>

        {/* Mobile drawer */}
        <Sheet open={drawerOpen} onOpenChange={setDrawerOpen}>
          <SheetContent side="left" className="flex flex-col">
            <span className="mb-4 font-display text-lg font-bold">Digestly</span>
            <NavList isAdmin={isAdmin} onNavigate={() => setDrawerOpen(false)} />
            <Footer user={user} />
          </SheetContent>
        </Sheet>

        <main className="mx-auto w-full max-w-6xl min-w-0 flex-1 p-4 sm:p-6">
          <Outlet />
        </main>
      </div>

      {/* Add-feed modal is mounted once here; opened from the top bar or Manage (§9.3). */}
      <AddFeedModal />
    </div>
  );
}
