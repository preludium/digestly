import { useForm } from "@tanstack/react-form";
import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { ConfirmDialog } from "@/components/common/ConfirmDialog";
import { FieldErrors } from "@/components/common/AuthCard";
import { ErrorBanner } from "@/components/common/ErrorBanner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { PasskeyManager } from "@/components/PasskeyManager";
import {
  useChangePassword,
  useDeleteAccount,
  useLogoutEverywhere,
  useMe,
} from "@/hooks/useAuth";
import { toast } from "@/stores/toast";

const ADMIN = "admin";

export function Profile() {
  const navigate = useNavigate();
  const { data: me } = useMe();
  const changePassword = useChangePassword();
  const logoutAll = useLogoutEverywhere();
  const deleteAccount = useDeleteAccount();
  const [deleting, setDeleting] = useState(false);

  const form = useForm({
    defaultValues: { current_password: "", new_password: "", confirm: "" },
    onSubmit: async ({ value, formApi }) => {
      await changePassword.mutateAsync({
        current_password: value.current_password,
        new_password: value.new_password,
      });
      toast("Password changed", "success");
      formApi.reset();
    },
  });

  if (!me) return null;
  const isBuiltinAdmin = me.username === ADMIN;

  return (
    <div className="space-y-6">
      <h1 className="font-display text-2xl font-semibold tracking-tight">Profile</h1>

      <Card>
        <CardHeader>
          <CardTitle>Account</CardTitle>
        </CardHeader>
        <CardContent className="flex items-center justify-between">
          <div>
            <p className="font-medium">{me.username}</p>
            <p className="text-sm text-muted-foreground">Signed in</p>
          </div>
          <Badge variant={me.role === "admin" ? "info" : "secondary"}>{me.role}</Badge>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Security</CardTitle>
        </CardHeader>
        <CardContent className="space-y-6">
          <div>
            <p className="mb-3 text-xs font-semibold uppercase tracking-wider text-muted-foreground">Password</p>
            {changePassword.isError && <div className="mb-4"><ErrorBanner error={changePassword.error} /></div>}
            <form
              className="space-y-4"
              onSubmit={(e) => {
                e.preventDefault();
                form.handleSubmit();
              }}
            >
              <form.Field
                name="current_password"
                validators={{ onChange: ({ value }) => (value ? undefined : "Required") }}
              >
                {(field) => (
                  <div className="space-y-1.5">
                    <Label htmlFor="current">Current password</Label>
                    <Input id="current" type="password" autoComplete="current-password"
                      value={field.state.value} onBlur={field.handleBlur}
                      onChange={(e) => field.handleChange(e.target.value)} />
                    <FieldErrors errors={field.state.meta.errors} />
                  </div>
                )}
              </form.Field>
              <form.Field
                name="new_password"
                validators={{ onChange: ({ value }) => (value.length >= 8 ? undefined : "At least 8 characters") }}
              >
                {(field) => (
                  <div className="space-y-1.5">
                    <Label htmlFor="new">New password</Label>
                    <Input id="new" type="password" autoComplete="new-password"
                      value={field.state.value} onBlur={field.handleBlur}
                      onChange={(e) => field.handleChange(e.target.value)} />
                    <FieldErrors errors={field.state.meta.errors} />
                  </div>
                )}
              </form.Field>
              <form.Field
                name="confirm"
                validators={{
                  onChangeListenTo: ["new_password"],
                  onChange: ({ value, fieldApi }) =>
                    value === fieldApi.form.getFieldValue("new_password") ? undefined : "Passwords do not match",
                }}
              >
                {(field) => (
                  <div className="space-y-1.5">
                    <Label htmlFor="confirm">Confirm new password</Label>
                    <Input id="confirm" type="password" autoComplete="new-password"
                      value={field.state.value} onBlur={field.handleBlur}
                      onChange={(e) => field.handleChange(e.target.value)} />
                    <FieldErrors errors={field.state.meta.errors} />
                  </div>
                )}
              </form.Field>
              <form.Subscribe selector={(s) => s.canSubmit}>
                {(canSubmit) => (
                  <Button type="submit" disabled={!canSubmit || changePassword.isPending}>
                    {changePassword.isPending ? "Saving…" : "Change password"}
                  </Button>
                )}
              </form.Subscribe>
            </form>
          </div>
          <div className="border-t border-border" />
          <div>
            <p className="mb-3 text-xs font-semibold uppercase tracking-wider text-muted-foreground">Passkeys</p>
            <p className="mb-3 text-sm text-muted-foreground">
              Sign in without a password using Touch ID, Windows Hello, or a security key.
            </p>
            <PasskeyManager />
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-destructive">Danger zone</CardTitle>
        </CardHeader>
        <CardContent className="flex flex-wrap gap-3">
          <Button
            variant="outline"
            onClick={() => logoutAll.mutate(undefined, { onSuccess: () => navigate("/login") })}
            disabled={logoutAll.isPending}
          >
            Log out everywhere
          </Button>
          {!isBuiltinAdmin && (
            <Button
              variant="destructive"
              disabled={deleteAccount.isPending}
              onClick={() => setDeleting(true)}
            >
              Delete my account
            </Button>
          )}
        </CardContent>
      </Card>

      <ConfirmDialog
        open={deleting}
        onOpenChange={setDeleting}
        title="Delete your account?"
        description="This will permanently delete your account and all your data. This cannot be undone."
        confirmLabel="Delete my account"
        destructive
        onConfirm={() => {
          deleteAccount.mutate(undefined, { onSuccess: () => navigate("/login") });
        }}
      />
    </div>
  );
}
