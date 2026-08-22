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
      setError(err instanceof Error ? err.message : "Authentication failed");
      setBusy(false);
    }
  }

  return (
    <div className="flex min-h-screen items-center justify-center bg-slate-50 p-4 text-slate-900">
      <form
        onSubmit={submit}
        className="w-full max-w-sm rounded-2xl border border-slate-200/90 bg-white p-8 shadow-sm ring-1 ring-slate-900/5"
      >
        <div className="mb-6 flex items-center gap-3">
          <div className="flex h-10 w-10 items-center justify-center rounded-xl bg-gradient-to-tr from-indigo-600 to-indigo-500 font-mono text-sm font-bold text-white shadow-xs">
            AI
          </div>
          <div>
            <p className="font-bold tracking-tight text-slate-900">AI Router</p>
            <p className="text-xs text-slate-400">Admin Authentication</p>
          </div>
        </div>

        <label className="mb-2 block text-xs font-semibold uppercase tracking-wider text-slate-600">
          Master key
        </label>
        <input
          type="password"
          value={key}
          onChange={(e) => setKey(e.target.value)}
          placeholder="Enter ROUTER_MASTER_KEY"
          autoFocus
          className="w-full rounded-xl border border-slate-300 bg-slate-50/50 px-3.5 py-2.5 text-sm text-slate-900 outline-none transition-all placeholder:text-slate-400 focus:border-indigo-500 focus:bg-white focus:ring-2 focus:ring-indigo-500/20"
        />

        {error && (
          <div className="mt-3 rounded-lg border border-rose-200 bg-rose-50/80 px-3 py-2 text-xs font-medium text-rose-700">
            {error}
          </div>
        )}

        <button
          type="submit"
          disabled={busy || !key.trim()}
          className="mt-6 w-full rounded-xl bg-indigo-600 py-2.5 text-sm font-semibold text-white shadow-xs transition-all hover:bg-indigo-700 hover:shadow-sm disabled:cursor-not-allowed disabled:opacity-40"
        >
          {busy ? "Authenticating…" : "Sign in"}
        </button>

        <p className="mt-4 text-center text-xs text-slate-400">
          Protected by master key authentication
        </p>
      </form>
    </div>
  );
}
