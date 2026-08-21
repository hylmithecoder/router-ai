"use client";

import { useState } from "react";
import { useRouter } from "next/navigation";
import { api, setMasterKey } from "@/lib/api";

export default function LoginPage() {
  const router = useRouter();
  const [key, setKey] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    setMasterKey(key.trim());
    try {
      await api.usageSummary();
      router.push("/dashboard");
    } catch (err) {
      setMasterKey("");
      setError(err instanceof Error ? err.message : "login failed");
      setBusy(false);
    }
  }

  return (
    <div className="flex min-h-screen items-center justify-center bg-zinc-950 p-4 text-zinc-100">
      <form
        onSubmit={submit}
        className="w-full max-w-sm rounded-2xl border border-zinc-800 bg-zinc-900/60 p-8"
      >
        <div className="mb-6 flex items-center gap-2">
          <div className="flex h-9 w-9 items-center justify-center rounded-lg bg-emerald-500 font-mono text-sm font-bold text-zinc-950">
            AI
          </div>
          <div>
            <p className="font-semibold">AI Router</p>
            <p className="text-xs text-zinc-500">admin dashboard</p>
          </div>
        </div>

        <label className="mb-2 block text-sm font-medium text-zinc-300">
          Master key
        </label>
        <input
          type="password"
          value={key}
          onChange={(e) => setKey(e.target.value)}
          placeholder="ROUTER_MASTER_KEY"
          autoFocus
          className="w-full rounded-lg border border-zinc-700 bg-zinc-950 px-3 py-2 text-sm outline-none transition-colors placeholder:text-zinc-600 focus:border-emerald-500"
        />

        {error && (
          <p className="mt-3 rounded-lg border border-red-500/30 bg-red-500/10 px-3 py-2 text-xs text-red-400">
            {error}
          </p>
        )}

        <button
          type="submit"
          disabled={busy || !key.trim()}
          className="mt-5 w-full rounded-lg bg-emerald-500 py-2 text-sm font-semibold text-zinc-950 transition-colors hover:bg-emerald-400 disabled:cursor-not-allowed disabled:opacity-40"
        >
          {busy ? "Checking…" : "Sign in"}
        </button>
      </form>
    </div>
  );
}