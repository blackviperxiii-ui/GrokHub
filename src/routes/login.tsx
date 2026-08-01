import { createFileRoute, Link } from "@tanstack/react-router";
import { GROK_PROVIDERS, authEnabled, signIn } from "@/lib/auth/client";
import { useCurrentUserState } from "@/lib/auth/use-current-user";
import { GrokHubMark } from "@/components/GrokLogo";
import { Button } from "@/components/ui/button";
import { Navigate } from "@tanstack/react-router";

export const Route = createFileRoute("/login")({
  component: LoginPage,
});

function LoginPage() {
  const { user, isPending } = useCurrentUserState();

  if (!isPending && user && !user.isDevFallback) {
    return <Navigate to="/" />;
  }

  return (
    <main className="grid min-h-dvh place-items-center bg-[var(--color-bg)] p-6 text-[var(--color-fg)]">
      <div className="w-full max-w-sm space-y-6">
        <div className="flex flex-col items-center gap-3 text-center">
          <GrokHubMark className="h-12 w-12" />
          <div>
            <h1 className="text-xl font-semibold tracking-tight">Sign in to GrokHub</h1>
            <p className="mt-1 text-sm text-[var(--color-muted)]">
              Connect with your Grok account via the Grok auth broker (Google or X).
            </p>
          </div>
        </div>

        {authEnabled ? (
          <div className="space-y-2">
            {GROK_PROVIDERS.map((p) => (
              <Button
                key={p.providerId}
                type="button"
                className="w-full"
                variant={p.idp === "google" ? "default" : "secondary"}
                onClick={() => void signIn(p.providerId, { callbackURL: "/" })}
              >
                Continue with {p.label}
              </Button>
            ))}
            <p className="pt-2 text-center text-xs text-[var(--color-subtle)]">
              OAuth is handled by Grok Build / auth.grok.me — this app never sees Google/X secrets.
            </p>
          </div>
        ) : (
          <p className="text-center text-sm text-[var(--color-muted)]">
            Sign-in is disabled in this build.
          </p>
        )}

        <div className="text-center text-sm">
          <Link to="/" className="text-[var(--color-muted)] underline-offset-4 hover:underline">
            Continue without account
          </Link>
        </div>
      </div>
    </main>
  );
}
