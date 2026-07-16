import { useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { toast } from "sonner";
import { AuthCard, FieldError } from "@/components/common/AuthCard";
import { ErrorBanner } from "@/components/common/ErrorBanner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useRegister, useRegistrationStatus } from "@/hooks/useAuth";

export function Register() {
    const navigate = useNavigate();
    const register = useRegister();
    const reg = useRegistrationStatus();

    const [username, setUsername] = useState("");
    const [password, setPassword] = useState("");
    const [confirm, setConfirm] = useState("");
    const [touched, setTouched] = useState({
        username: false,
        password: false,
        confirm: false,
    });

    const errors = {
        username:
            username.trim().length >= 3 ? undefined : "At least 3 characters",
        password: password.length >= 8 ? undefined : "At least 8 characters",
        confirm: confirm === password ? undefined : "Passwords do not match",
    };
    const canSubmit = !errors.username && !errors.password && !errors.confirm;

    const submit = (e: React.FormEvent) => {
        e.preventDefault();
        if (!canSubmit) return;
        register.mutate(
            { username, password },
            {
                onSuccess: () => {
                    toast.success("Welcome to Digestly");
                    navigate("/", { replace: true });
                },
            },
        );
    };

    if (reg.data && !reg.data.allow_registration) {
        return (
            <AuthCard title="Registration is disabled">
                <p className="text-center text-sm text-muted-foreground">
                    Registration is disabled - ask the admin for an account.
                </p>
                <Button asChild variant="outline" className="w-full">
                    <Link to="/login">Back to sign in</Link>
                </Button>
            </AuthCard>
        );
    }

    return (
        <AuthCard title="Create an account">
            {register.isError && <ErrorBanner error={register.error} />}
            <form className="space-y-4" onSubmit={submit}>
                <div className="space-y-1.5">
                    <Label htmlFor="username">Username</Label>
                    <Input
                        id="username"
                        autoComplete="username"
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
                        autoComplete="new-password"
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

                <div className="space-y-1.5">
                    <Label htmlFor="confirm">Confirm password</Label>
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
                    {touched.confirm && <FieldError message={errors.confirm} />}
                </div>

                <Button
                    type="submit"
                    className="w-full"
                    disabled={!canSubmit || register.isPending}
                >
                    {register.isPending ? "Creating…" : "Create account"}
                </Button>
            </form>

            <p className="text-center text-sm text-muted-foreground">
                Already have an account?{" "}
                <Link to="/login" className="text-primary hover:underline">
                    Sign in
                </Link>
            </p>
        </AuthCard>
    );
}
