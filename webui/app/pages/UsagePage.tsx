"use client";

import { useEffect, useState } from "react";
import { useRouter } from "next/navigation";
import {
  api,
  formatDuration,
  formatTime,
  formatTokens,
  getMasterKey,
  type UsageLogRow,
} from "@/lib/api";
import Shell from "@/components/Shell";
import { Badge, ErrorBox, Spinner } from "@/components/ui";

const PAGE_SIZE = 50;

export default function UsagePage() {
  const router = useRouter();
  const [rows, setRows] = useState<UsageLogRow[] | null>(null);
  const [offset, setOffset] = useState(0);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!getMasterKey()) {
      router.replace("/login");
      return;
    }
    api
      .usageLog(PAGE_SIZE, offset)
      .then((res) => setRows(res.data.rows))
      .catch((err) =>
        setError(err instanceof Error ? err.message : "failed to load"),
      );
  }, [router, offset]);

  return (
    <Shell>
      <div className="mb-6 flex items-center justify-between">
        <h1 className="text-xl font-semibold">Usage log</h1>
        <div className="flex gap-2">
          <button
            disabled={offset === 0}
            onClick={() => setOffset((o) => Math.max(0, o - PAGE_SIZE))}
            className="rounded-lg border border-zinc-700 px-3 py-1.5 text-sm text-zinc-300 transition-colors hover:bg-zinc-900 disabled:cursor-not-allowed disabled:opacity-40"
          >
            ← Newer
          </button>
          <button
            disabled={!rows || rows.length < PAGE_SIZE}
            onClick={() => setOffset((o) => o + PAGE_SIZE)}
            className="rounded-lg border border-zinc-700 px-3 py-1.5 text-sm text-zinc-300 transition-colors hover:bg-zinc-900 disabled:cursor-not-allowed disabled:opacity-40"
          >
            Older →
          </button>
        </div>
      </div>

      {error && <ErrorBox message={error} />}
      {!rows && !error && <Spinner />}

      {rows && (
        <div className="overflow-x-auto rounded-xl border border-zinc-800 bg-zinc-900/60">
          <table className="w-full min-w-[760px] text-sm">
            <thead>
              <tr className="border-b border-zinc-800 text-left text-xs uppercase tracking-wider text-zinc-500">
                <th className="px-4 py-3">Time</th>
                <th className="px-4 py-3">Key</th>
                <th className="px-4 py-3">Model</th>
                <th className="px-4 py-3">Provider</th>
                <th className="px-4 py-3 text-right">Tokens</th>
                <th className="px-4 py-3 text-right">Latency</th>
                <th className="px-4 py-3">Status</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-zinc-800/60">
              {rows.map((r) => (
                <tr key={r.id} className="hover:bg-zinc-800/40">
                  <td className="whitespace-nowrap px-4 py-2.5 font-mono text-xs text-zinc-400">
                    {formatTime(r.created_at)}
                  </td>
                  <td className="px-4 py-2.5 text-zinc-300">{r.key_name}</td>
                  <td className="px-4 py-2.5 font-mono text-xs text-zinc-400">
                    {r.model}
                  </td>
                  <td className="px-4 py-2.5 font-mono text-xs text-zinc-400">
                    {r.provider}
                  </td>
                  <td className="px-4 py-2.5 text-right font-mono text-zinc-300">
                    {formatTokens(r.total_tokens)}
                  </td>
                  <td className="px-4 py-2.5 text-right font-mono text-xs text-zinc-400">
                    {formatDuration(r.latency_ms)}
                  </td>
                  <td className="px-4 py-2.5">
                    {r.status === 200 ? (
                      <Badge tone="green">200</Badge>
                    ) : (
                      <Badge tone="red">{r.status}</Badge>
                    )}
                  </td>
                </tr>
              ))}
              {rows.length === 0 && (
                <tr>
                  <td colSpan={7} className="py-10 text-center text-zinc-500">
                    no requests recorded yet
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      )}
    </Shell>
  );
}
