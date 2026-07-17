import { KeyRound } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { toast } from "sonner";
import { AuthCard, FieldError } from "@/components/common/AuthCard";
import { ErrorBanner } from "@/components/common/ErrorBanner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useLogin, useRegistrationStatus } from "@/hooks/useAuth";
import {
    useDiscoverablePasskeyLogin,
    usePasskeyLogin,
} from "@/hooks/usePasskeys";
import { apiError } from "@/lib/apiError";
import {
    conditionalMediationAvailable,
    isCancellation,
    passkeysSupported,
} from "@/lib/webauthn";

export function Login() {
    const navigate = useNavigate();
    const login = useLogin();
    const reg = useRegistrationStatus();
    const passkeyLogin = usePasskeyLogin();
    const discoverableLogin = useDiscoverablePasskeyLogin();
    const showPasskey = reg.data?.passkeys_enabled && passkeysSupported();
    const conditionalAbort = useRef<AbortController | null>(null);

    const [username, setUsername] = useState("");
    const [password, setPassword] = useState("");
    const [touched, setTouched] = useState({
        username: false,
        password: false,
    });

    const errors = {
        username: username.trim() ? undefined : "Username is required",
        password: password ? undefined : "Password is required",
    };
    const canSubmit = !errors.username && !errors.password;

    const submit = (e: React.FormEvent) => {
        e.preventDefault();
        if (!canSubmit) return;
        conditionalAbort.current?.abort(); // manual submit wins over any background autofill request
        login.mutate(
            { username, password },
            { onSuccess: () => navigate("/", { replace: true }) },
        );
    };

    // Conditional UI (autofill): on mount, if supported, prime the username field with saved passkeys.
    // Purely a background enhancement - any failure/cancellation is swallowed; the forms stay usable.
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
                // user dismissed the picker, aborted, or the browser declined - no error surfaced
            }
        })();
        return () => {
            cancelled = true;
            controller.abort();
        };
    }, [showPasskey, discoverableLogin, navigate]);

    const signInWithPasskey = async () => {
        if (!username.trim()) {
            toast.error("Enter your username, then use your passkey");
            return;
        }
        conditionalAbort.current?.abort();
        try {
            await passkeyLogin.mutateAsync(username.trim());
            navigate("/", { replace: true });
        } catch (e) {
            if (isCancellation(e)) return; // user dismissed the prompt - no error
            toast.error(apiError(e, "Passkey sign-in failed"));
        }
    };

    return (
        <AuthCard title="Sign in to your account">
            {login.isError && (
                <ErrorBanner error="Invalid username or password" />
            )}
            <form className="space-y-4" onSubmit={submit}>
                <div className="space-y-1.5">
                    <Label htmlFor="username">Username</Label>
                    <Input
                        id="username"
                        autoComplete={
                            showPasskey ? "username webauthn" : "username"
                        }
                        value={username}
                        onBlur={() =>
                            setTouched((t) => ({ ...t, username: true }))
                        }
                        onChange={(e) => setUsername(e.target.value)}
                    />
                    {touched.username && (
                        <FieldError message={errors.username} />
                    )}
                </div>

                <div className="space-y-1.5">
                    <Label htmlFor="password">Password</Label>
                    <Input
                        id="password"
                        type="password"
                        autoComplete="current-password"
                        value={password}
                        onBlur={() =>
                            setTouched((t) => ({ ...t, password: true }))
                        }
                        onChange={(e) => setPassword(e.target.value)}
                    />
                    {touched.password && (
                        <FieldError message={errors.password} />
                    )}
                </div>

                <Button
                    type="submit"
                    className="w-full"
                    disabled={!canSubmit || login.isPending}
                >
                    {login.isPending ? "Signing in…" : "Sign in"}
                </Button>
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
                        {passkeyLogin.isPending
                            ? "Waiting for passkey…"
                            : "Sign in with a passkey"}
                    </Button>
                </>
            )}

            {reg.data?.allow_registration && (
                <p className="text-center text-sm text-muted-foreground">
                    No account?{" "}
                    <Link
                        to="/register"
                        className="text-primary hover:underline"
                    >
                        Register
                    </Link>
                </p>
            )}
        </AuthCard>
    );
}
