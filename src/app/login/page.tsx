import { getIdentity } from "@/lib/auth-server";
import { supabaseServer } from "@/lib/supabase/server";
import { redirect } from "next/navigation";
import { SignInButton, SignOutButton } from "./buttons";

export const metadata = { title: "Sign in" };

export default async function LoginPage({
  searchParams,
}: {
  searchParams: Promise<{ error?: string }>;
}) {
  const identity = await getIdentity();
  if (identity.role === "host") redirect("/");
  if (identity.role === "guest") redirect("/jam");

  // A signed-in user who is not the owner. Offer a way out instead of a redirect
  // loop back through the proxy.
  const supabase = await supabaseServer();
  const { data } = await supabase.auth.getUser();
  const signedInStranger = Boolean(data?.user);

  const { error } = await searchParams;

  return (
    <main className="flex min-h-dvh items-center justify-center p-6">
      <div className="w-full max-w-sm space-y-6 text-center">
        <div className="space-y-1">
          <h1 className="text-2xl font-semibold tracking-tight">Cpz Music</h1>
          <p className="text-sm text-neutral-500">Self-hosted personal music streamer</p>
        </div>

        {signedInStranger ? (
          <div className="space-y-4">
            <p className="text-sm text-neutral-400">
              You are signed in, but this library belongs to someone else.
            </p>
            <SignOutButton />
          </div>
        ) : (
          <SignInButton />
        )}

        {error ? (
          <p className="text-sm text-red-400">
            {error === "missing_code" ? "Sign in was cancelled." : "Sign in failed. Try again."}
          </p>
        ) : null}
      </div>
    </main>
  );
}
