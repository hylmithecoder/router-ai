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

  return (
    <Shell>
      <h1 className="mb-6 text-xl font-semibold">API keys</h1>

      {error && <ErrorBox message={error} />}

      <div className="mb-6 grid grid-cols-1 gap-6 lg:grid-cols-3">
        <Card title="Create key" className="lg:col-span-1">
          <form onSubmit={create} className="flex flex-col gap-3">
            <div>
              <label className="mb-1 block text-xs text-zinc-500">Name</label>
              <input
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="discord-bot"
                required
                className="w-full rounded-lg border border-zinc-700 bg-zinc-950 px-3 py-2 text-sm outline-none placeholder:text-zinc-600 focus:border-emerald-500"
              />
            </div>
            <div>
              <label className="mb-1 block text-xs text-zinc-500">
                Daily quota (tokens, 0 = unlimited)
              </label>
              <input
                value={quota}
                onChange={(e) => setQuota(e.target.value)}
                inputMode="numeric"
                className="w-full rounded-lg border border-zinc-700 bg-zinc-950 px-3 py-2 font-mono text-sm outline-none focus:border-emerald-500"
              />
            </div>
            <button
              type="submit"
              disabled={busy || !name.trim()}
              className="rounded-lg bg-emerald-500 py-2 text-sm font-semibold text-zinc-950 transition-colors hover:bg-emerald-400 disabled:opacity-40"
            >
              {busy ? "Creating…" : "Create"}
            </button>
          </form>

          {newKey && (
            <div className="mt-4 rounded-lg border border-emerald-500/30 bg-emerald-500/10 p-3">
              <p className="mb-1 text-xs font-medium text-emerald-400">
                Key created — copy it now, it will not be shown again:
              </p>
              <code className="break-all font-mono text-xs text-emerald-300">
                {newKey}
              </code>
              <button
                onClick={() => navigator.clipboard?.writeText(newKey)}
                className="mt-2 text-xs text-emerald-400 underline-offset-2 hover:underline"
              >
                copy
              </button>
            </div>
          )}
        </Card>

        <Card title={`Keys (${keys?.length ?? "…"})`} className="lg:col-span-2">
          {!keys && <Spinner />}
          {keys && (
            <table className="w-full text-sm">
              <thead>
                <tr className="text-left text-xs uppercase tracking-wider text-zinc-500">
                  <th className="pb-2">Name</th>
                  <th className="pb-2">Prefix</th>
                  <th className="pb-2 text-right">Quota</th>
                  <th className="pb-2 text-center">Status</th>
                  <th className="pb-2 text-right">Actions</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-zinc-800">
                {keys.map((k) => (
                  <tr key={k.id}>
                    <td className="py-2.5 font-medium text-zinc-300">
                      {k.name}
                    </td>
                    <td className="py-2.5 font-mono text-xs text-zinc-500">
                      {k.key_prefix}
                    </td>
                    <td className="py-2.5 text-right font-mono text-zinc-400">
                      {k.quota_daily_tokens > 0
                        ? formatTokens(k.quota_daily_tokens)
                        : "∞"}
                    </td>
                    <td className="py-2.5 text-center">
                      {k.enabled ? (
                        <Badge tone="green">enabled</Badge>
                      ) : (
                        <Badge tone="red">disabled</Badge>
                      )}
                    </td>
                    <td className="py-2.5 text-right">
                      <button
                        onClick={() => toggle(k)}
                        className="mr-2 text-xs text-zinc-400 underline-offset-2 hover:text-zinc-200 hover:underline"
                      >
                        {k.enabled ? "disable" : "enable"}
                      </button>
                      <button
                        onClick={() => remove(k)}
                        className="text-xs text-red-400 underline-offset-2 hover:underline"
                      >
                        delete
                      </button>
                    </td>
                  </tr>
                ))}
                {keys.length === 0 && (
                  <tr>
                    <td colSpan={5} className="py-6 text-center text-zinc-500">
                      no keys yet
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
          )}
        </Card>
      </div>
    </Shell>
  );
}
