import { Shield, ShieldOff, Trash2 } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { ConfirmDialog } from "@/components/common/ConfirmDialog";
import { ErrorBanner } from "@/components/common/ErrorBanner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import {
    Table,
    TableBody,
    TableCell,
    TableHead,
    TableHeader,
    TableRow,
} from "@/components/ui/table";
import {
    Tooltip,
    TooltipContent,
    TooltipProvider,
    TooltipTrigger,
} from "@/components/ui/tooltip";
import {
    useAdminSettings,
    useDeleteUser,
    useUpdateAdminSettings,
    useUpdateUser,
    useUsers,
} from "@/hooks/useAdmin";
import { useMe } from "@/hooks/useAuth";
import type { AdminUser } from "@/lib/types";

const BUILTIN_ADMIN = "admin";

function RegistrationToggle() {
    const settings = useAdminSettings();
    const update = useUpdateAdminSettings();
    const enabled = settings.data?.allow_registration ?? false;
    return (
        <Card>
            <CardHeader>
                <CardTitle>Open registration</CardTitle>
            </CardHeader>
            <CardContent className="flex items-center justify-between">
                <p className="text-sm text-muted-foreground">
                    When on, anyone can create an account. When off, only you
                    can.
                </p>
                <Switch
                    checked={enabled}
                    onCheckedChange={() =>
                        update.mutate(
                            { allow_registration: !enabled },
                            {
                                onSuccess: () =>
                                    toast.success(
                                        `Registration ${!enabled ? "enabled" : "disabled"}`,
                                    ),
                            },
                        )
                    }
                    disabled={settings.isLoading || update.isPending}
                />
            </CardContent>
        </Card>
    );
}

function UserRow({ user, meId }: { user: AdminUser; meId: number }) {
    const update = useUpdateUser();
    const del = useDeleteUser();
    const isBuiltin = user.username === BUILTIN_ADMIN;
    const isSelf = user.id === meId;
    const busy = update.isPending || del.isPending;
    const [deleting, setDeleting] = useState(false);

    return (
        <TableRow>
            <TableCell className="font-medium">{user.username}</TableCell>
            <TableCell>
                <Badge variant={user.role === "admin" ? "info" : "secondary"}>
                    {user.role}
                </Badge>
            </TableCell>
            <TableCell>
                <div className="flex items-center gap-2">
                    <Switch
                        checked={!user.disabled}
                        onCheckedChange={() =>
                            update.mutate({
                                id: user.id,
                                disabled: !user.disabled,
                            })
                        }
                        disabled={busy || isBuiltin || isSelf}
                        aria-label="Account enabled"
                    />
                    <span className="text-xs text-muted-foreground">
                        {user.disabled ? "Disabled" : "Active"}
                    </span>
                </div>
            </TableCell>
            <TableCell className="hidden text-muted-foreground sm:table-cell">
                {user.subscription_count}
            </TableCell>
            <TableCell className="hidden text-muted-foreground sm:table-cell">
                {user.last_login_at ?? "never"}
            </TableCell>
            <TableCell>
                <TooltipProvider>
                    <div className="flex justify-end gap-2">
                        <Tooltip>
                            <TooltipTrigger asChild>
                                <Button
                                    size="sm"
                                    variant="outline"
                                    disabled={busy || isBuiltin}
                                    onClick={() =>
                                        update.mutate({
                                            id: user.id,
                                            role:
                                                user.role === "admin"
                                                    ? "user"
                                                    : "admin",
                                        })
                                    }
                                    aria-label={
                                        user.role === "admin"
                                            ? "Make user"
                                            : "Make admin"
                                    }
                                >
                                    {user.role === "admin" ? (
                                        <>
                                            <ShieldOff className="size-4" />
                                            <span className="sr-only sm:not-sr-only sm:ml-1.5">
                                                Make user
                                            </span>
                                        </>
                                    ) : (
                                        <>
                                            <Shield className="size-4" />
                                            <span className="sr-only sm:not-sr-only sm:ml-1.5">
                                                Make admin
                                            </span>
                                        </>
                                    )}
                                </Button>
                            </TooltipTrigger>
                            <TooltipContent className="sm:hidden">
                                {user.role === "admin"
                                    ? "Make user"
                                    : "Make admin"}
                            </TooltipContent>
                        </Tooltip>
                        <Tooltip>
                            <TooltipTrigger asChild>
                                <Button
                                    size="sm"
                                    variant="destructive"
                                    disabled={busy || isBuiltin || isSelf}
                                    onClick={() => setDeleting(true)}
                                    aria-label="Delete"
                                >
                                    <Trash2 className="size-4" />
                                    <span className="sr-only sm:not-sr-only sm:ml-1.5">
                                        Delete
                                    </span>
                                </Button>
                            </TooltipTrigger>
                            <TooltipContent className="sm:hidden">
                                Delete
                            </TooltipContent>
                        </Tooltip>
                        <ConfirmDialog
                            open={deleting}
                            onOpenChange={setDeleting}
                            title={`Delete ${user.username}?`}
                            description="All their data will be permanently removed."
                            confirmLabel="Delete"
                            destructive
                            onConfirm={() => {
                                del.mutate(user.id, {
                                    onSuccess: () =>
                                        toast.success("User deleted"),
                                });
                            }}
                        />
                    </div>
                </TooltipProvider>
            </TableCell>
        </TableRow>
    );
}

export function AdminUsers() {
    const { data: me } = useMe();
    const users = useUsers();

    return (
        <div className="space-y-6">
            <h1 className="font-display text-2xl font-semibold tracking-tight">
                Users
            </h1>
            <RegistrationToggle />

            <Card>
                <CardHeader>
                    <CardTitle>All accounts</CardTitle>
                </CardHeader>
                <CardContent>
                    {users.isLoading && (
                        <div className="space-y-2">
                            {[0, 1, 2].map((i) => (
                                <Skeleton key={i} className="h-10 w-full" />
                            ))}
                        </div>
                    )}
                    {users.isError && <ErrorBanner error={users.error} />}
                    {users.data && (
                        <Table>
                            <TableHeader>
                                <TableRow className="hover:bg-card">
                                    <TableHead>Username</TableHead>
                                    <TableHead>Role</TableHead>
                                    <TableHead>Status</TableHead>
                                    <TableHead className="hidden sm:table-cell">
                                        Feeds
                                    </TableHead>
                                    <TableHead className="hidden sm:table-cell">
                                        Last login
                                    </TableHead>
                                    <TableHead className="text-right">
                                        <span className="sr-only">Actions</span>
                                    </TableHead>
                                </TableRow>
                            </TableHeader>
                            <TableBody>
                                {users.data.map((u) => (
                                    <UserRow
                                        key={u.id}
                                        user={u}
                                        meId={me?.id ?? -1}
                                    />
                                ))}
                            </TableBody>
                        </Table>
                    )}
                </CardContent>
            </Card>
        </div>
    );
}
