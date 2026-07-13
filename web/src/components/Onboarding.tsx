import { Check, Rss } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { PasskeyManager } from "@/components/PasskeyManager";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Spinner } from "@/components/ui/spinner";
import { useRegistrationStatus } from "@/hooks/useAuth";
import {
    useSettings,
    useSubscribeStarterFeeds,
    useUpdateSettings,
} from "@/hooks/useSettings";
import { passkeysSupported } from "@/lib/webauthn";
import { useUiStore } from "@/stores/ui";

/** First-run onboarding (prompt.md §9.11) - shown once per new account (gated on the `onboarded`
 *  setting). Optional starter feeds + timezone; fully skippable. Finishing/skipping sets
 *  `onboarded=true` so it never shows again. */
export function Onboarding() {
    const settings = useSettings();
    const update = useUpdateSettings();
    const starter = useSubscribeStarterFeeds();
    const reg = useRegistrationStatus();
    const setTheme = useUiStore((s) => s.setTheme);
    const showPasskey = reg.data?.passkeys_enabled && passkeysSupported();

    const guessTz = Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC";
    const [tz, setTz] = useState(
        settings.data?.timezone && settings.data.timezone !== "UTC"
            ? settings.data.timezone
            : guessTz,
    );
    const [addedStarter, setAddedStarter] = useState(false);

    const addStarter = () =>
        starter.mutate(undefined, {
            onSuccess: (r) => {
                setAddedStarter(true);
                toast.success(
                    `Added ${r.added} starter feed${r.added === 1 ? "" : "s"}`,
                );
            },
            onError: (e) =>
                toast.error(
                    e instanceof Error ? e.message : "Could not add feeds",
                ),
        });

    const finish = () => {
        update.mutate(
            { timezone: tz, onboarded: true },
            {
                onError: (e) =>
                    toast.error(
                        e instanceof Error ? e.message : "Could not save",
                    ),
            },
        );
    };

    // Apply saved theme once settings arrive (new device picks up the stored preference).
    if (settings.data && settings.data.theme !== useUiStore.getState().theme) {
        setTheme(settings.data.theme);
    }

    return (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-background/90 p-4">
            <div className="w-full max-w-md space-y-5 rounded-lg border border-border bg-card p-6 shadow-xl">
                <div>
                    <h1 className="font-display text-xl font-bold">
                        Welcome to Digestly 👋
                    </h1>
                    <p className="text-sm text-muted-foreground">
                        A couple of quick things - you can skip and change these
                        later.
                    </p>
                </div>

                <div className="space-y-2">
                    <Label>Starter feeds</Label>
                    <p className="text-xs text-muted-foreground">
                        Subscribe to a handful of popular tech feeds - topics
                        are created for you. Or skip and build your own from
                        scratch.
                    </p>
                    <Button
                        variant="outline"
                        size="sm"
                        disabled={starter.isPending || addedStarter}
                        onClick={addStarter}
                    >
                        {starter.isPending ? (
                            <Spinner className="size-4" />
                        ) : addedStarter ? (
                            <Check className="size-4" />
                        ) : (
                            <Rss className="size-4" />
                        )}
                        {addedStarter ? "Added" : "Add starter feeds"}
                    </Button>
                </div>

                <div className="space-y-1.5">
                    <Label htmlFor="ob-tz">Your timezone</Label>
                    <Input
                        id="ob-tz"
                        value={tz}
                        onChange={(e) => setTz(e.target.value)}
                        placeholder="Europe/Warsaw"
                    />
                    <p className="text-xs text-muted-foreground">
                        Used for “Today/This week” filters and the digest
                        schedule.
                    </p>
                </div>

                {showPasskey && (
                    <div className="space-y-2">
                        <Label>Passkey</Label>
                        <p className="text-xs text-muted-foreground">
                            Add a passkey now for passwordless sign-in - or
                            later from your profile.
                        </p>
                        <PasskeyManager compact />
                    </div>
                )}

                <p className="text-xs text-muted-foreground">
                    Tip: set up push notifications and (for admins) an AI
                    provider under Settings.
                </p>

                <div className="flex justify-end gap-2 pt-1">
                    <Button disabled={update.isPending} onClick={finish}>
                        {update.isPending ? "Saving…" : "Get started"}
                    </Button>
                </div>
            </div>
        </div>
    );
}
