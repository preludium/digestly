import { Navigate, Route, Routes } from "react-router-dom";
import { AppShell } from "./components/AppShell";
import { AppBanners } from "./components/AppBanners";
import { Onboarding } from "./components/Onboarding";
import { ErrorBanner } from "./components/common/ErrorBanner";
import { Spinner } from "./components/ui/spinner";
import { useMe } from "./hooks/useAuth";
import { useSettings } from "./hooks/useSettings";
import { AdminUsers } from "./routes/AdminUsers";
import { Digests } from "./routes/Digests";
import { DigestDetail } from "./routes/DigestDetail";
import { Feed } from "./routes/Feed";
import { Health } from "./routes/Health";
import { Login } from "./routes/Login";
import { Manage } from "./routes/Manage";
import { NotFound } from "./routes/NotFound";
import { Profile } from "./routes/Profile";
import { Register } from "./routes/Register";
import { Search } from "./routes/Search";
import { Settings } from "./routes/Settings";

function FullScreen({ children }: { children: React.ReactNode }) {
  return <div className="flex min-h-dvh items-center justify-center p-6">{children}</div>;
}

export function App() {
  const { data: me, isLoading, isError, error } = useMe();

  if (isLoading) {
    return (
      <FullScreen>
        <Spinner className="size-8" />
      </FullScreen>
    );
  }

  // A hard failure (server unreachable) — not merely "not logged in" (which resolves to null).
  if (isError) {
    return (
      <FullScreen>
        <div className="w-full max-w-md">
          <ErrorBanner error={error} />
        </div>
      </FullScreen>
    );
  }

  return (
    <>
      <AppBanners />
      {me && <OnboardingGate />}
      <Routes>
        <Route path="/login" element={me ? <Navigate to="/" replace /> : <Login />} />
      <Route path="/register" element={me ? <Navigate to="/" replace /> : <Register />} />

      <Route element={me ? <AppShell user={me} /> : <Navigate to="/login" replace />}>
        <Route path="/" element={<Feed />} />
        <Route path="/search" element={<Search />} />
        <Route path="/manage" element={<Manage />} />
        <Route path="/digests" element={<Digests />} />
        <Route path="/digests/:id" element={<DigestDetail />} />
        <Route path="/health" element={<Health />} />
        <Route path="/settings" element={<Settings />} />
        <Route path="/profile" element={<Profile />} />
        <Route
          path="/admin/users"
          element={me?.role === "admin" ? <AdminUsers /> : <Navigate to="/" replace />}
        />
      </Route>

        <Route path="*" element={<NotFound />} />
      </Routes>
    </>
  );
}

/** Shows the first-run onboarding overlay once per account (gated on the `onboarded` setting). */
function OnboardingGate() {
  const settings = useSettings();
  if (!settings.data || settings.data.onboarded) return null;
  return <Onboarding />;
}
