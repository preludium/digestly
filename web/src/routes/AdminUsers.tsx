import { useState } from "react";
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
    useAdminSettings,
    useDeleteUser,
    useUpdateAdminSettings,
    useUpdateUser,
    useUsers,
} from "@/hooks/useAdmin";
import { useMe } from "@/hooks/useAuth";
import type { AdminUser } from "@/lib/types";
import { toast } from "@/stores/toast";

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
                                    toast(
                                        `Registration ${!enabled ? "enabled" : "disabled"}`,
                                        "success",
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
            <TableCell className="text-muted-foreground">
                {user.subscription_count}
            </TableCell>
            <TableCell className="text-muted-foreground">
                {user.last_login_at ?? "never"}
            </TableCell>
            <TableCell>
                <div className="flex flex-wrap justify-end gap-2">
                    <Button
                        size="sm"
                        variant="outline"
                        disabled={busy || isBuiltin}
                        onClick={() =>
                            update.mutate({
                                id: user.id,
                                role: user.role === "admin" ? "user" : "admin",
                            })
                        }
                    >
                        {user.role === "admin" ? "Make user" : "Make admin"}
                    </Button>
                    <Button
                        size="sm"
                        variant="destructive"
                        disabled={busy || isBuiltin || isSelf}
                        onClick={() => setDeleting(true)}
                    >
                        Delete
                    </Button>
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
                                    toast("User deleted", "success"),
                            });
                        }}
                    />
                </div>
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
                                    <TableHead>Feeds</TableHead>
                                    <TableHead>Last login</TableHead>
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
