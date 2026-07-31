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
      className="w-full rounded-full bg-violet-600 px-4 py-2.5 text-sm font-medium text-white transition-colors hover:bg-violet-500 disabled:opacity-50"
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
      className="w-full rounded-full border border-zinc-600 px-4 py-2.5 text-sm font-medium transition-colors hover:border-zinc-400"
    >
      Sign out
    </button>
  );
}
