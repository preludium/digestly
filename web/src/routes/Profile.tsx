import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { FieldError } from "@/components/common/AuthCard";
import { ConfirmDialog } from "@/components/common/ConfirmDialog";
import { ErrorBanner } from "@/components/common/ErrorBanner";
import { PasskeyManager } from "@/components/PasskeyManager";
import {
    SETTINGS_TILE_CLASS,
    TileTitle,
} from "@/components/settings/SettingsTile";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
    useChangePassword,
    useDeleteAccount,
    useLogoutEverywhere,
    useMe,
} from "@/hooks/useAuth";
import { cn } from "@/lib/utils";
import { toast } from "@/stores/toast";

const ADMIN = "admin";

export function Profile() {
    const navigate = useNavigate();
    const { data: me } = useMe();
    const changePassword = useChangePassword();
    const logoutAll = useLogoutEverywhere();
    const deleteAccount = useDeleteAccount();
    const [deleting, setDeleting] = useState(false);

    const [current, setCurrent] = useState("");
    const [next, setNext] = useState("");
    const [confirm, setConfirm] = useState("");
    const [touched, setTouched] = useState({
        current: false,
        next: false,
        confirm: false,
    });

    const errors = {
        current: current ? undefined : "Required",
        next: next.length >= 8 ? undefined : "At least 8 characters",
        confirm: confirm === next ? undefined : "Passwords do not match",
    };
    const canSubmit = !errors.current && !errors.next && !errors.confirm;

    const submit = (e: React.FormEvent) => {
        e.preventDefault();
        if (!canSubmit) return;
        changePassword.mutate(
            { current_password: current, new_password: next },
            {
                onSuccess: () => {
                    toast("Password changed", "success");
                    setCurrent("");
                    setNext("");
                    setConfirm("");
                    setTouched({
                        current: false,
                        next: false,
                        confirm: false,
                    });
                },
            },
        );
    };

    if (!me) return null;
    const isBuiltinAdmin = me.username === ADMIN;

    return (
        <div className="space-y-6">
            <h1 className="font-display text-2xl font-semibold tracking-tight">
                Profile
            </h1>

            <div className="space-y-5">
                <div className="space-y-3.5">
                    <h3 className="border-b border-border pb-2 text-[13px] font-bold tracking-wide">
                        Password
                    </h3>
                    {changePassword.isError && (
                        <ErrorBanner error={changePassword.error} />
                    )}
                    <form className="space-y-4" onSubmit={submit}>
                        <div className="space-y-1.5">
                            <Label htmlFor="current">Current password</Label>
                            <Input
                                id="current"
                                type="password"
                                autoComplete="current-password"
                                value={current}
                                onBlur={() =>
                                    setTouched((t) => ({ ...t, current: true }))
                                }
                                onChange={(e) => setCurrent(e.target.value)}
                            />
                            {touched.current && (
                                <FieldError message={errors.current} />
                            )}
                        </div>
                        <div className="space-y-1.5">
                            <Label htmlFor="new">New password</Label>
                            <Input
                                id="new"
                                type="password"
                                autoComplete="new-password"
                                value={next}
                                onBlur={() =>
                                    setTouched((t) => ({ ...t, next: true }))
                                }
                                onChange={(e) => setNext(e.target.value)}
                            />
                            {touched.next && (
                                <FieldError message={errors.next} />
                            )}
                        </div>
                        <div className="space-y-1.5">
                            <Label htmlFor="confirm">
                                Confirm new password
                            </Label>
                            <Input
                                id="confirm"
                                type="password"
                                autoComplete="new-password"
                                value={confirm}
                                onBlur={() =>
                                    setTouched((t) => ({ ...t, confirm: true }))
                                }
                                onChange={(e) => setConfirm(e.target.value)}
                            />
                            {touched.confirm && (
                                <FieldError message={errors.confirm} />
                            )}
                        </div>
                        <Button
                            type="submit"
                            disabled={!canSubmit || changePassword.isPending}
                        >
                            {changePassword.isPending
                                ? "Saving…"
                                : "Change password"}
                        </Button>
                    </form>
                </div>

                <div className="space-y-3.5">
                    <h3 className="border-b border-border pb-2 text-[13px] font-bold tracking-wide">
                        Passkeys
                    </h3>
                    <p className="text-[13px] text-muted-foreground">
                        Sign in without a password using Touch ID, Windows
                        Hello, or a security key.
                    </p>
                    <PasskeyManager />
                </div>

                <div className="space-y-3.5">
                    <h3 className="border-b border-border pb-2 text-[13px] font-bold tracking-wide">
                        Session
                    </h3>
                    <div
                        className={cn(
                            SETTINGS_TILE_CLASS,
                            "flex items-center justify-between gap-4",
                        )}
                    >
                        <TileTitle
                            title="Log out everywhere"
                            description="Sign out of all devices and browsers."
                        />
                        <Button
                            variant="outline"
                            className="shrink-0 bg-card"
                            onClick={() =>
                                logoutAll.mutate(undefined, {
                                    onSuccess: () => navigate("/login"),
                                })
                            }
                            disabled={logoutAll.isPending}
                        >
                            Log out everywhere
                        </Button>
                    </div>
                </div>

                {!isBuiltinAdmin && (
                    <div className="space-y-3 border-t border-border pt-5">
                        <h3 className="border-b border-border pb-2 text-[13px] font-bold tracking-wide text-destructive">
                            Danger zone
                        </h3>
                        <div className="flex flex-col gap-3 rounded-lg border border-destructive/30 bg-destructive/5 p-3.5 sm:flex-row sm:items-center sm:justify-between">
                            <TileTitle
                                title="Delete my account"
                                description="Permanently deletes your account and all your data. This cannot be undone."
                            />
                            <Button
                                variant="destructive"
                                className="shrink-0"
                                disabled={deleteAccount.isPending}
                                onClick={() => setDeleting(true)}
                            >
                                Delete my account
                            </Button>
                        </div>
                    </div>
                )}
            </div>

            <ConfirmDialog
                open={deleting}
                onOpenChange={setDeleting}
                title="Delete your account?"
                description="This will permanently delete your account and all your data. This cannot be undone."
                confirmLabel="Delete my account"
                destructive
                onConfirm={() => {
                    deleteAccount.mutate(undefined, {
                        onSuccess: () => navigate("/login"),
                    });
                }}
            />
        </div>
    );
}
