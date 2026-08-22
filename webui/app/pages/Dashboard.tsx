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
      <h1 className="mb-6 text-xl font-semibold">Overview</h1>

      {error && <ErrorBox message={error} />}
      {!summary && !error && <Spinner />}

      {summary && (
        <div className="flex flex-col gap-6">
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

          <div className="grid grid-cols-1 gap-6 lg:grid-cols-3">
            <Card title="Daily quota" className="lg:col-span-1">
              <QuotaGauge
                used={summary.today.total_tokens}
                quota={summary.daily_quota_tokens}
              />
            </Card>

            <Card title="Last 7 days (tokens)" className="lg:col-span-2">
              {summary.by_day.length === 0 ? (
                <p className="py-10 text-center text-sm text-zinc-500">
                  no usage yet
                </p>
              ) : (
                <BarChart data={summary.by_day} />
              )}
            </Card>
          </div>

          <div className="grid grid-cols-1 gap-6 lg:grid-cols-2">
            <Card title="Per key (today)">
              <table className="w-full text-sm">
                <thead>
                  <tr className="text-left text-xs uppercase tracking-wider text-zinc-500">
                    <th className="pb-2">Key</th>
                    <th className="pb-2 text-right">Requests</th>
                    <th className="pb-2 text-right">Tokens</th>
                    <th className="pb-2 text-right">Quota</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-zinc-800">
                  {summary.by_key.map((k) => (
                    <tr key={k.key_id}>
                      <td className="py-2 font-medium text-zinc-300">
                        {k.key_name}
                      </td>
                      <td className="py-2 text-right font-mono text-zinc-400">
                        {formatTokens(k.requests)}
                      </td>
                      <td className="py-2 text-right font-mono text-zinc-400">
                        {formatTokens(k.total_tokens)}
                      </td>
                      <td className="py-2 text-right font-mono text-zinc-500">
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
                        className="py-6 text-center text-zinc-500"
                      >
                        no usage today
                      </td>
                    </tr>
                  )}
                </tbody>
              </table>
            </Card>

            <Card title="Per provider (today)">
              <table className="w-full text-sm">
                <thead>
                  <tr className="text-left text-xs uppercase tracking-wider text-zinc-500">
                    <th className="pb-2">Provider</th>
                    <th className="pb-2 text-right">Requests</th>
                    <th className="pb-2 text-right">Tokens</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-zinc-800">
                  {summary.by_provider.map((p) => (
                    <tr key={p.provider}>
                      <td className="py-2 font-mono text-zinc-300">
                        {p.provider}
                      </td>
                      <td className="py-2 text-right font-mono text-zinc-400">
                        {formatTokens(p.requests)}
                      </td>
                      <td className="py-2 text-right font-mono text-zinc-400">
                        {formatTokens(p.total_tokens)}
                      </td>
                    </tr>
                  ))}
                  {summary.by_provider.length === 0 && (
                    <tr>
                      <td
                        colSpan={3}
                        className="py-6 text-center text-zinc-500"
                      >
                        no usage today
                      </td>
                    </tr>
                  )}
                </tbody>
              </table>
            </Card>
          </div>
        </div>
      )}
    </Shell>
  );
}
