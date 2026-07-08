import { useForm } from "@tanstack/react-form";
import { Link, useNavigate } from "react-router-dom";
import { AuthCard, FieldErrors } from "@/components/common/AuthCard";
import { ErrorBanner } from "@/components/common/ErrorBanner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useRegister, useRegistrationStatus } from "@/hooks/useAuth";
import { toast } from "@/stores/toast";

export function Register() {
  const navigate = useNavigate();
  const register = useRegister();
  const reg = useRegistrationStatus();

  const form = useForm({
    defaultValues: { username: "", password: "", confirm: "" },
    onSubmit: async ({ value }) => {
      await register.mutateAsync({ username: value.username, password: value.password });
      toast("Welcome to Digestly", "success");
      navigate("/", { replace: true });
    },
  });

  if (reg.data && !reg.data.allow_registration) {
    return (
      <AuthCard title="Registration is disabled">
        <p className="text-center text-sm text-muted-foreground">
          Registration is disabled — ask the admin for an account.
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
      <form
        className="space-y-4"
        onSubmit={(e) => {
          e.preventDefault();
          form.handleSubmit();
        }}
      >
        <form.Field
          name="username"
          validators={{
            onChange: ({ value }) =>
              value.trim().length >= 3 ? undefined : "At least 3 characters",
          }}
        >
          {(field) => (
            <div className="space-y-1.5">
              <Label htmlFor="username">Username</Label>
              <Input
                id="username"
                autoComplete="username"
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
          validators={{
            onChange: ({ value }) => (value.length >= 8 ? undefined : "At least 8 characters"),
          }}
        >
          {(field) => (
            <div className="space-y-1.5">
              <Label htmlFor="password">Password</Label>
              <Input
                id="password"
                type="password"
                autoComplete="new-password"
                value={field.state.value}
                onBlur={field.handleBlur}
                onChange={(e) => field.handleChange(e.target.value)}
              />
              <FieldErrors errors={field.state.meta.errors} />
            </div>
          )}
        </form.Field>

        <form.Field
          name="confirm"
          validators={{
            onChangeListenTo: ["password"],
            onChange: ({ value, fieldApi }) =>
              value === fieldApi.form.getFieldValue("password") ? undefined : "Passwords do not match",
          }}
        >
          {(field) => (
            <div className="space-y-1.5">
              <Label htmlFor="confirm">Confirm password</Label>
              <Input
                id="confirm"
                type="password"
                autoComplete="new-password"
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
            <Button type="submit" className="w-full" disabled={!canSubmit || register.isPending}>
              {register.isPending ? "Creating…" : "Create account"}
            </Button>
          )}
        </form.Subscribe>
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
