"use client";

import { useCallback, useEffect, useState } from "react";
import { useRouter } from "next/navigation";
import { api, formatTokens, getMasterKey, type ApiKey } from "@/lib/api";
import Shell from "@/components/Shell";
import { Badge, Card, ErrorBox, Spinner } from "@/components/ui";

export default function KeysPage() {
  const router = useRouter();
  const [keys, setKeys] = useState<ApiKey[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const [name, setName] = useState("");
  const [quota, setQuota] = useState("0");
  const [busy, setBusy] = useState(false);
  const [newKey, setNewKey] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  const load = useCallback(() => {
    api
      .listKeys()
      .then((res) => setKeys(res.data))
      .catch((err) =>
        setError(err instanceof Error ? err.message : "failed to load"),
      );
  }, []);

  useEffect(() => {
    if (!getMasterKey()) {
      router.replace("/login");
      return;
    }
    load();
  }, [router, load]);

  async function create(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const res = await api.createKey(
        name.trim(),
        Math.max(0, Number(quota) || 0),
      );
      setNewKey(res.data.key);
      setCopied(false);
      setName("");
      setQuota("0");
      load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "create failed");
    } finally {
      setBusy(false);
    }
  }

  async function toggle(key: ApiKey) {
    try {
      await api.updateKey(key.id, { enabled: !key.enabled });
      load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "update failed");
    }
  }

  async function remove(key: ApiKey) {
    if (!window.confirm(`Delete key "${key.name}"? This cannot be undone.`))
      return;
    try {
      await api.deleteKey(key.id);
      load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "delete failed");
    }
  }

  function handleCopy() {
    if (!newKey) return;
    navigator.clipboard?.writeText(newKey);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  }

  return (
    <Shell>
      <div className="mb-6 flex items-center justify-between">
        <div>
          <h1 className="text-xl font-bold tracking-tight text-slate-900">API keys</h1>
          <p className="text-xs text-slate-500">Manage client credentials and token rate limits</p>
        </div>
      </div>

      {error && <ErrorBox message={error} />}

      <div className="mb-6 grid grid-cols-1 gap-6 lg:grid-cols-3">
        {/* Create Key Card */}
        <Card title="Create key" className="lg:col-span-1">
          <form onSubmit={create} className="flex flex-col gap-4">
            <div>
              <label className="mb-1.5 block text-xs font-semibold uppercase tracking-wider text-slate-500">
                Key name / Identifier
              </label>
              <input
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="e.g. discord-bot-prod"
                required
                className="w-full rounded-lg border border-slate-300 bg-slate-50/50 px-3 py-2 text-sm text-slate-900 outline-none transition-all placeholder:text-slate-400 focus:border-indigo-500 focus:bg-white focus:ring-2 focus:ring-indigo-500/20"
              />
            </div>

            <div>
              <label className="mb-1.5 block text-xs font-semibold uppercase tracking-wider text-slate-500">
                Daily quota (tokens, 0 = unlimited)
              </label>
              <input
                value={quota}
                onChange={(e) => setQuota(e.target.value)}
                inputMode="numeric"
                className="w-full rounded-lg border border-slate-300 bg-slate-50/50 px-3 py-2 font-mono text-sm text-slate-900 outline-none transition-all focus:border-indigo-500 focus:bg-white focus:ring-2 focus:ring-indigo-500/20"
              />
            </div>

            <button
              type="submit"
              disabled={busy || !name.trim()}
              className="rounded-lg bg-indigo-600 py-2.5 text-sm font-semibold text-white shadow-xs transition-all hover:bg-indigo-700 hover:shadow-sm disabled:opacity-40"
            >
              {busy ? "Creating key…" : "Generate API Key"}
            </button>
          </form>

          {newKey && (
            <div className="mt-4 rounded-xl border border-emerald-200 bg-emerald-50/80 p-4 shadow-xs">
              <p className="mb-1.5 text-xs font-bold text-emerald-800">
                Key generated successfully — save it now, it will not be shown again:
              </p>
              <div className="rounded-lg border border-emerald-200/80 bg-white p-2.5">
                <code className="break-all font-mono text-xs font-medium text-emerald-950">
                  {newKey}
                </code>
              </div>
              <button
                onClick={handleCopy}
                className="mt-2.5 inline-flex items-center gap-1.5 rounded-md border border-emerald-300 bg-white px-2.5 py-1 text-xs font-semibold text-emerald-700 shadow-xs transition-colors hover:bg-emerald-50"
              >
                {copied ? "✓ Copied to clipboard" : "📋 Copy API Key"}
              </button>
            </div>
          )}
        </Card>

        {/* Key List Card */}
        <Card title={`Active keys (${keys?.length ?? "…"})`} className="lg:col-span-2">
          {!keys && <Spinner />}
          {keys && (
            <div className="overflow-x-auto">
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b border-slate-100 text-left text-xs font-semibold uppercase tracking-wider text-slate-400">
                    <th className="pb-3">Name</th>
                    <th className="pb-3">Prefix</th>
                    <th className="pb-3 text-right">Quota</th>
                    <th className="pb-3 text-center">Status</th>
                    <th className="pb-3 text-right">Actions</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-slate-100">
                  {keys.map((k) => (
                    <tr key={k.id} className="transition-colors hover:bg-slate-50/60">
                      <td className="py-3 font-medium text-slate-800">
                        {k.name}
                      </td>
                      <td className="py-3 font-mono text-xs text-slate-500">
                        {k.key_prefix}…
                      </td>
                      <td className="py-3 text-right font-mono text-xs text-slate-700">
                        {k.quota_daily_tokens > 0
                          ? formatTokens(k.quota_daily_tokens)
                          : "∞ (unlimited)"}
                      </td>
                      <td className="py-3 text-center">
                        {k.enabled ? (
                          <Badge tone="green">Active</Badge>
                        ) : (
                          <Badge tone="red">Disabled</Badge>
                        )}
                      </td>
                      <td className="py-3 text-right">
                        <button
                          onClick={() => toggle(k)}
                          className="mr-3 text-xs font-semibold text-indigo-600 hover:text-indigo-800 hover:underline"
                        >
                          {k.enabled ? "Disable" : "Enable"}
                        </button>
                        <button
                          onClick={() => remove(k)}
                          className="text-xs font-semibold text-rose-600 hover:text-rose-800 hover:underline"
                        >
                          Delete
                        </button>
                      </td>
                    </tr>
                  ))}
                  {keys.length === 0 && (
                    <tr>
                      <td colSpan={5} className="py-10 text-center text-xs font-medium text-slate-400">
                        No API keys configured yet
                      </td>
                    </tr>
                  )}
                </tbody>
              </table>
            </div>
          )}
        </Card>
      </div>
    </Shell>
  );
}
