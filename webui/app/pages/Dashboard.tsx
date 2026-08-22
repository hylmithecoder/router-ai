"use client";

import { useEffect, useState } from "react";
import { useRouter } from "next/navigation";
import { api, formatTokens, getMasterKey, type UsageSummary } from "@/lib/api";
import Shell from "@/components/Shell";
import {
  BarChart,
  Card,
  ErrorBox,
  QuotaGauge,
  Spinner,
  Stat,
} from "@/components/ui";

export default function DashboardPage() {
  const router = useRouter();
  const [summary, setSummary] = useState<UsageSummary | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!getMasterKey()) {
      router.replace("/login");
      return;
    }
    api
      .usageSummary()
      .then((res) => setSummary(res.data))
      .catch((err) =>
        setError(err instanceof Error ? err.message : "failed to load"),
      );
  }, [router]);

  return (
    <Shell>
      <div className="mb-6 flex items-center justify-between">
        <div>
          <h1 className="text-xl font-bold tracking-tight text-slate-900">Overview</h1>
          <p className="text-xs text-slate-500">Real-time router metrics and token analytics</p>
        </div>
      </div>

      {error && <ErrorBox message={error} />}
      {!summary && !error && <Spinner />}

      {summary && (
        <div className="flex flex-col gap-6">
          {/* Top Metric Cards */}
          <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-4">
            <Stat
              label="Tokens today"
              value={formatTokens(summary.today.total_tokens)}
              sub={`${formatTokens(summary.today.prompt_tokens)} prompt · ${formatTokens(summary.today.completion_tokens)} completion`}
            />
            <Stat
              label="Requests today"
              value={formatTokens(summary.today.requests)}
            />
            <Stat
              label="Providers used"
              value={formatTokens(summary.by_provider.length)}
              sub={
                summary.by_provider.map((p) => p.provider).join(", ") || "none"
              }
            />
            <Stat
              label="Active keys"
              value={formatTokens(summary.by_key.length)}
              sub={summary.by_key.map((k) => k.key_name).join(", ") || "none"}
            />
          </div>

          {/* Quota & 7-Day Chart */}
          <div className="grid grid-cols-1 gap-6 lg:grid-cols-3">
            <Card title="Daily quota" className="lg:col-span-1">
              <QuotaGauge
                used={summary.today.total_tokens}
                quota={summary.daily_quota_tokens}
              />
            </Card>

            <Card title="Last 7 days usage (tokens)" className="lg:col-span-2">
              {summary.by_day.length === 0 ? (
                <p className="py-12 text-center text-sm font-medium text-slate-400">
                  No recorded usage in the last 7 days
                </p>
              ) : (
                <BarChart data={summary.by_day} />
              )}
            </Card>
          </div>

          {/* Breakdown Tables */}
          <div className="grid grid-cols-1 gap-6 lg:grid-cols-2">
            <Card title="Usage per key (today)">
              <div className="overflow-x-auto">
                <table className="w-full text-sm">
                  <thead>
                    <tr className="border-b border-slate-100 text-left text-xs font-semibold uppercase tracking-wider text-slate-400">
                      <th className="pb-3">Key</th>
                      <th className="pb-3 text-right">Requests</th>
                      <th className="pb-3 text-right">Tokens</th>
                      <th className="pb-3 text-right">Quota</th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-slate-100">
                    {summary.by_key.map((k) => (
                      <tr key={k.key_id} className="hover:bg-slate-50/60">
                        <td className="py-2.5 font-medium text-slate-800">
                          {k.key_name}
                        </td>
                        <td className="py-2.5 text-right font-mono text-xs text-slate-600">
                          {formatTokens(k.requests)}
                        </td>
                        <td className="py-2.5 text-right font-mono text-xs font-semibold text-slate-900">
                          {formatTokens(k.total_tokens)}
                        </td>
                        <td className="py-2.5 text-right font-mono text-xs text-slate-500">
                          {k.quota_daily_tokens > 0
                            ? formatTokens(k.quota_daily_tokens)
                            : "∞"}
                        </td>
                      </tr>
                    ))}
                    {summary.by_key.length === 0 && (
                      <tr>
                        <td
                          colSpan={4}
                          className="py-8 text-center text-xs font-medium text-slate-400"
                        >
                          No active key usage today
                        </td>
                      </tr>
                    )}
                  </tbody>
                </table>
              </div>
            </Card>

            <Card title="Usage per provider (today)">
              <div className="overflow-x-auto">
                <table className="w-full text-sm">
                  <thead>
                    <tr className="border-b border-slate-100 text-left text-xs font-semibold uppercase tracking-wider text-slate-400">
                      <th className="pb-3">Provider</th>
                      <th className="pb-3 text-right">Requests</th>
                      <th className="pb-3 text-right">Tokens</th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-slate-100">
                    {summary.by_provider.map((p) => (
                      <tr key={p.provider} className="hover:bg-slate-50/60">
                        <td className="py-2.5 font-mono text-xs font-medium text-slate-800">
                          {p.provider}
                        </td>
                        <td className="py-2.5 text-right font-mono text-xs text-slate-600">
                          {formatTokens(p.requests)}
                        </td>
                        <td className="py-2.5 text-right font-mono text-xs font-semibold text-slate-900">
                          {formatTokens(p.total_tokens)}
                        </td>
                      </tr>
                    ))}
                    {summary.by_provider.length === 0 && (
                      <tr>
                        <td
                          colSpan={3}
                          className="py-8 text-center text-xs font-medium text-slate-400"
                        >
                          No provider traffic today
                        </td>
                      </tr>
                    )}
                  </tbody>
                </table>
              </div>
            </Card>
          </div>
        </div>
      )}
    </Shell>
  );
}
