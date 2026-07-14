import {
    FolderCog,
    HeartPulse,
    LayoutGrid,
    LogOut,
    Moon,
    Newspaper,
    Server,
    Settings,
    Sun,
    User as UserIcon,
    Users,
} from "lucide-react";
import { NavLink, Outlet, useLocation, useNavigate } from "react-router-dom";
import { toast } from "sonner";
import { AddFeedModal } from "@/components/feeds/AddFeedModal";
import { Button } from "@/components/ui/button";
import {
    DropdownMenu,
    DropdownMenuContent,
    DropdownMenuItem,
    DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
    Sidebar,
    SidebarContent,
    SidebarFooter,
    SidebarGroup,
    SidebarGroupContent,
    SidebarGroupLabel,
    SidebarHeader,
    SidebarInset,
    SidebarMenu,
    SidebarMenuBadge,
    SidebarMenuButton,
    SidebarMenuItem,
    SidebarProvider,
    SidebarRail,
    SidebarTrigger,
    useSidebar,
} from "@/components/ui/sidebar";
import { useLogout } from "@/hooks/useAuth";
import { useUnhealthyCount } from "@/hooks/useFeeds";
import { useIngestEvents } from "@/hooks/useIngest";
import { useUnreadCount } from "@/hooks/useItems";
import type { User } from "@/lib/types";
import { cn } from "@/lib/utils";
import { useUiStore } from "@/stores/ui";

interface NavItem {
    to: string;
    label: string;
    icon: React.ComponentType<{ className?: string }>;
}

const MAIN_NAV: NavItem[] = [
    { to: "/", label: "Articles", icon: LayoutGrid },
    { to: "/manage", label: "Manage", icon: FolderCog },
    { to: "/digests", label: "Digests", icon: Newspaper },
    { to: "/health", label: "Feed health", icon: HeartPulse },
    { to: "/settings", label: "Settings", icon: Settings },
    { to: "/profile", label: "Profile", icon: UserIcon },
];

const ADMIN_NAV: NavItem[] = [
    { to: "/admin/system", label: "System", icon: Server },
    { to: "/admin/users", label: "Users", icon: Users },
];

/** True if `pathname` is on this nav item's route. Prefix match, so a nested route
 *  (`/manage/123`, `/digests/7`) keeps its parent highlighted; "/" is exact or it would match
 *  everything. NavLink applies this rule to itself, but `SidebarMenuButton asChild` needs
 *  `isActive` passed in separately, so the rule is duplicated here on purpose. */
function isNavActive(pathname: string, to: string): boolean {
    return to === "/" ? pathname === "/" : pathname.startsWith(to);
}

function NavItemRow({ item }: { item: NavItem }) {
    const unhealthy = useUnhealthyCount();
    const location = useLocation();
    const { setOpenMobile } = useSidebar();
    const active = isNavActive(location.pathname, item.to);

    return (
        <SidebarMenuItem>
            <SidebarMenuButton asChild isActive={active} tooltip={item.label}>
                <NavLink to={item.to} onClick={() => setOpenMobile(false)}>
                    <item.icon className="size-4" />
                    <span className="truncate transition-[opacity,width] duration-200 ease-linear group-data-[collapsible=icon]:w-0 group-data-[collapsible=icon]:opacity-0">
                        {item.label}
                    </span>
                </NavLink>
            </SidebarMenuButton>
            {item.to === "/health" && unhealthy > 0 && (
                <SidebarMenuBadge className="bg-destructive text-destructive-foreground">
                    {unhealthy}
                </SidebarMenuBadge>
            )}
        </SidebarMenuItem>
    );
}

function AccountMenu({ user }: { user: User }) {
    const navigate = useNavigate();
    const logout = useLogout();
    const initial = user.username.charAt(0).toUpperCase();

    return (
        <DropdownMenu>
            <DropdownMenuTrigger asChild>
                <button
                    type="button"
                    className="flex items-center gap-2 rounded-full py-1 pl-1 pr-2.5 hover:bg-muted hover:cursor-pointer"
                    aria-label="Account menu"
                >
                    <span className="flex size-7 shrink-0 items-center justify-center rounded-full bg-primary/10 text-xs font-semibold text-primary">
                        {initial}
                    </span>
                    <span className="hidden max-w-32 truncate text-sm font-medium sm:inline">
                        {user.username}
                    </span>
                </button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
                <DropdownMenuItem
                    className="text-destructive focus:bg-destructive/10 focus:text-destructive"
                    onClick={() =>
                        logout.mutate(undefined, {
                            onSuccess: () => {
                                toast("Signed out");
                                navigate("/login");
                            },
                        })
                    }
                >
                    <LogOut className="size-4" /> Log out
                </DropdownMenuItem>
            </DropdownMenuContent>
        </DropdownMenu>
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
            {theme === "dark" ? (
                <Sun className="size-4" />
            ) : (
                <Moon className="size-4" />
            )}
        </Button>
    );
}

/** App chrome around authed routes (prompt.md §9.0). */
export function AppShell({ user }: { user: User }) {
    const isAdmin = user.role === "admin";
    const navigate = useNavigate();
    const unread = useUnreadCount();
    // One live connection for the whole session: drives the ingest toast and refreshes the feed
    // the moment ingestion ends.
    useIngestEvents();

    return (
        <SidebarProvider>
            <Sidebar collapsible="icon">
                <SidebarHeader className="relative">
                    <button
                        type="button"
                        onClick={() => navigate("/")}
                        className="flex items-center gap-2 overflow-hidden px-2 py-1.5 font-display text-lg font-bold tracking-tight opacity-100 transition-[opacity,width] duration-200 ease-linear group-data-[collapsible=icon]:pointer-events-none group-data-[collapsible=icon]:w-0 group-data-[collapsible=icon]:opacity-0"
                    >
                        <span className="shrink-0">Digestly</span>
                        {unread > 0 && (
                            <span className="flex min-w-5 shrink-0 items-center justify-center rounded-full bg-primary px-1.5 text-xs font-semibold text-primary-foreground">
                                {unread}
                            </span>
                        )}
                    </button>
                    <button
                        type="button"
                        onClick={() => navigate("/")}
                        aria-label="Digestly"
                        className="pointer-events-none absolute left-2 top-1/2 flex size-8 -translate-y-1/2 items-center justify-center rounded-full bg-primary/10 font-display text-sm font-bold text-primary opacity-0 transition-opacity duration-200 ease-linear group-data-[collapsible=icon]:pointer-events-auto group-data-[collapsible=icon]:opacity-100"
                    >
                        D
                    </button>
                </SidebarHeader>
                <SidebarContent>
                    <SidebarGroup>
                        <SidebarGroupContent>
                            <SidebarMenu>
                                {MAIN_NAV.map((item) => (
                                    <NavItemRow key={item.to} item={item} />
                                ))}
                            </SidebarMenu>
                        </SidebarGroupContent>
                    </SidebarGroup>
                    {isAdmin && (
                        <SidebarGroup>
                            <SidebarGroupLabel>Admin</SidebarGroupLabel>
                            <SidebarGroupContent>
                                <SidebarMenu>
                                    {ADMIN_NAV.map((item) => (
                                        <NavItemRow key={item.to} item={item} />
                                    ))}
                                </SidebarMenu>
                            </SidebarGroupContent>
                        </SidebarGroup>
                    )}
                </SidebarContent>
                <SidebarFooter />
                <SidebarRail />
            </Sidebar>

            <SidebarInset>
                <header className="z-30 flex shrink-0 items-center gap-3 border-b border-border bg-card px-3 py-2.5">
                    <SidebarTrigger />
                    <div className="ml-auto flex items-center gap-2">
                        <ThemeToggle />
                        <AccountMenu user={user} />
                    </div>
                </header>

                <main className={cn("min-h-0 min-w-0 flex-1 overflow-y-auto")}>
                    <div className="mx-auto w-full max-w-6xl p-4 sm:p-6">
                        <Outlet />
                    </div>
                </main>
            </SidebarInset>

            {/* Add-feed modal is mounted once here; opened from the top bar or Manage (§9.3). */}
            <AddFeedModal />
        </SidebarProvider>
    );
}
