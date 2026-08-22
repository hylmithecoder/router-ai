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
        <div>
          <h1 className="text-xl font-bold tracking-tight text-slate-900">Usage log</h1>
          <p className="text-xs text-slate-500">Historical API requests, token consumption, and response status</p>
        </div>
        <div className="flex gap-2">
          <button
            disabled={offset === 0}
            onClick={() => setOffset((o) => Math.max(0, o - PAGE_SIZE))}
            className="rounded-lg border border-slate-200 bg-white px-3.5 py-1.5 text-xs font-semibold text-slate-700 shadow-xs transition-all hover:bg-slate-50 hover:text-slate-900 disabled:cursor-not-allowed disabled:opacity-40"
          >
            ← Newer
          </button>
          <button
            disabled={!rows || rows.length < PAGE_SIZE}
            onClick={() => setOffset((o) => o + PAGE_SIZE)}
            className="rounded-lg border border-slate-200 bg-white px-3.5 py-1.5 text-xs font-semibold text-slate-700 shadow-xs transition-all hover:bg-slate-50 hover:text-slate-900 disabled:cursor-not-allowed disabled:opacity-40"
          >
            Older →
          </button>
        </div>
      </div>

      {error && <ErrorBox message={error} />}
      {!rows && !error && <Spinner />}

      {rows && (
        <div className="overflow-x-auto rounded-xl border border-slate-200/90 bg-white shadow-xs">
          <table className="w-full min-w-[760px] text-sm">
            <thead>
              <tr className="border-b border-slate-200/90 bg-slate-50/75 text-left text-xs font-semibold uppercase tracking-wider text-slate-500">
                <th className="px-4 py-3">Time</th>
                <th className="px-4 py-3">Key</th>
                <th className="px-4 py-3">Model</th>
                <th className="px-4 py-3">Provider</th>
                <th className="px-4 py-3 text-right">Tokens</th>
                <th className="px-4 py-3 text-right">Latency</th>
                <th className="px-4 py-3">Status</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-100">
              {rows.map((r) => (
                <tr key={r.id} className="transition-colors hover:bg-slate-50/75">
                  <td className="whitespace-nowrap px-4 py-3 font-mono text-xs text-slate-500">
                    {formatTime(r.created_at)}
                  </td>
                  <td className="px-4 py-3 font-medium text-slate-800">{r.key_name}</td>
                  <td className="px-4 py-3 font-mono text-xs text-slate-600">
                    {r.model}
                  </td>
                  <td className="px-4 py-3 font-mono text-xs text-slate-600">
                    {r.provider}
                  </td>
                  <td className="px-4 py-3 text-right font-mono text-xs font-semibold text-slate-900">
                    {formatTokens(r.total_tokens)}
                  </td>
                  <td className="px-4 py-3 text-right font-mono text-xs text-slate-500">
                    {formatDuration(r.latency_ms)}
                  </td>
                  <td className="px-4 py-3">
                    {r.status === 200 ? (
                      <Badge tone="green">200 OK</Badge>
                    ) : (
                      <Badge tone="red">{r.status}</Badge>
                    )}
                  </td>
                </tr>
              ))}
              {rows.length === 0 && (
                <tr>
                  <td colSpan={7} className="py-12 text-center text-sm font-medium text-slate-400">
                    No requests recorded yet
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
