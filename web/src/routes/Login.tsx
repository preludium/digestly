import { useForm } from "@tanstack/react-form";
import { KeyRound } from "lucide-react";
import { useEffect, useRef } from "react";
import { Link, useNavigate } from "react-router-dom";
import { AuthCard, FieldErrors } from "@/components/common/AuthCard";
import { ErrorBanner } from "@/components/common/ErrorBanner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useLogin, useRegistrationStatus } from "@/hooks/useAuth";
import { useDiscoverablePasskeyLogin, usePasskeyLogin } from "@/hooks/usePasskeys";
import { conditionalMediationAvailable, isCancellation, passkeysSupported } from "@/lib/webauthn";
import { toast } from "@/stores/toast";

export function Login() {
  const navigate = useNavigate();
  const login = useLogin();
  const reg = useRegistrationStatus();
  const passkeyLogin = usePasskeyLogin();
  const discoverableLogin = useDiscoverablePasskeyLogin();
  const showPasskey = reg.data?.passkeys_enabled && passkeysSupported();
  const conditionalAbort = useRef<AbortController | null>(null);

  const form = useForm({
    defaultValues: { username: "", password: "" },
    onSubmit: async ({ value }) => {
      conditionalAbort.current?.abort(); // manual submit wins over any background autofill request
      await login.mutateAsync(value);
      navigate("/", { replace: true });
    },
  });

  // Conditional UI (autofill): on mount, if supported, prime the username field with saved passkeys.
  // Purely a background enhancement — any failure/cancellation is swallowed; the forms stay usable.
  useEffect(() => {
    if (!showPasskey) return;
    let cancelled = false;
    const controller = new AbortController();
    conditionalAbort.current = controller;
    (async () => {
      if (!(await conditionalMediationAvailable()) || cancelled) return;
      try {
        await discoverableLogin.mutateAsync(controller.signal);
        if (!cancelled) navigate("/", { replace: true });
      } catch {
        // user dismissed the picker, aborted, or the browser declined — no error surfaced
      }
    })();
    return () => {
      cancelled = true;
      controller.abort();
    };
  }, [showPasskey, discoverableLogin, navigate]);

  const signInWithPasskey = async () => {
    const username = form.getFieldValue("username").trim();
    if (!username) {
      toast("Enter your username, then use your passkey", "error");
      return;
    }
    conditionalAbort.current?.abort();
    try {
      await passkeyLogin.mutateAsync(username);
      navigate("/", { replace: true });
    } catch (e) {
      if (isCancellation(e)) return; // user dismissed the prompt — no error
      toast(e instanceof Error ? e.message : "Passkey sign-in failed", "error");
    }
  };

  return (
    <AuthCard title="Sign in to your account">
      {login.isError && <ErrorBanner error="Invalid username or password" />}
      <form
        className="space-y-4"
        onSubmit={(e) => {
          e.preventDefault();
          form.handleSubmit();
        }}
      >
        <form.Field
          name="username"
          validators={{ onChange: ({ value }) => (value.trim() ? undefined : "Username is required") }}
        >
          {(field) => (
            <div className="space-y-1.5">
              <Label htmlFor="username">Username</Label>
              <Input
                id="username"
                autoComplete={showPasskey ? "username webauthn" : "username"}
                value={field.state.value}
                onBlur={field.handleBlur}
                onChange={(e) => field.handleChange(e.target.value)}
              />
              <FieldErrors errors={field.state.meta.errors} />
            </div>
          )}
        </form.Field>

        <form.Field
          name="password"
          validators={{ onChange: ({ value }) => (value ? undefined : "Password is required") }}
        >
          {(field) => (
            <div className="space-y-1.5">
              <Label htmlFor="password">Password</Label>
              <Input
                id="password"
                type="password"
                autoComplete="current-password"
                value={field.state.value}
                onBlur={field.handleBlur}
                onChange={(e) => field.handleChange(e.target.value)}
              />
              <FieldErrors errors={field.state.meta.errors} />
            </div>
          )}
        </form.Field>

        <form.Subscribe selector={(s) => s.canSubmit}>
          {(canSubmit) => (
            <Button type="submit" className="w-full" disabled={!canSubmit || login.isPending}>
              {login.isPending ? "Signing in…" : "Sign in"}
            </Button>
          )}
        </form.Subscribe>
      </form>

      {showPasskey && (
        <>
          <div className="flex items-center gap-3 text-xs text-muted-foreground">
            <span className="h-px flex-1 bg-border" />
            or
            <span className="h-px flex-1 bg-border" />
          </div>
          <Button
            type="button"
            variant="outline"
            className="w-full"
            disabled={passkeyLogin.isPending}
            onClick={signInWithPasskey}
          >
            <KeyRound className="size-4" />
            {passkeyLogin.isPending ? "Waiting for passkey…" : "Sign in with a passkey"}
          </Button>
        </>
      )}

      {reg.data?.allow_registration && (
        <p className="text-center text-sm text-muted-foreground">
          No account?{" "}
          <Link to="/register" className="text-primary hover:underline">
            Register
          </Link>
        </p>
      )}
    </AuthCard>
  );
}
