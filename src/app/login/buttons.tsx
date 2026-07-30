"use client";

import { useState } from "react";
import { supabaseBrowser } from "@/lib/supabase/client";

export function SignInButton() {
  const [busy, setBusy] = useState(false);

  async function signIn() {
    setBusy(true);
    const supabase = supabaseBrowser();
    const { error } = await supabase.auth.signInWithOAuth({
      provider: "github",
      options: { redirectTo: `${window.location.origin}/auth/callback` },
    });
    if (error) setBusy(false);
  }

  return (
    <button
      onClick={signIn}
      disabled={busy}
      className="w-full rounded-md bg-neutral-100 px-4 py-2.5 text-sm font-medium text-neutral-900 transition hover:bg-white disabled:opacity-50"
    >
      {busy ? "Redirecting..." : "Sign in with GitHub"}
    </button>
  );
}

export function SignOutButton() {
  async function signOut() {
    await supabaseBrowser().auth.signOut();
    window.location.href = "/login";
  }

  return (
    <button
      onClick={signOut}
      className="w-full rounded-md border border-neutral-700 px-4 py-2.5 text-sm font-medium transition hover:bg-neutral-800"
    >
      Sign out
    </button>
  );
}
