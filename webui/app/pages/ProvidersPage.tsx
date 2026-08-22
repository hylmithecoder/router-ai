"use client";

import { useCallback, useEffect, useState } from "react";
import { useRouter } from "next/navigation";
import { api, formatTime, getMasterKey, type Provider } from "@/lib/api";
import Shell from "@/components/Shell";
import { Badge, Card, ErrorBox, Spinner } from "@/components/ui";

const DEFAULT_GROQ_URL = "https://api.groq.com/openai/v1";
const DEFAULT_GROQ_MODEL = "llama-3.3-70b-versatile";
const DEFAULT_NVIDIA_URL = "https://integrate.api.nvidia.com/v1";
const DEFAULT_NVIDIA_MODEL = "nvidia/nemotron-3-nano-omni-30b-a3b-reasoning";

function cooldownLeft(until: string | null): string | null {
  if (!until) return null;
  const ms = new Date(until).getTime() - Date.now();
  if (ms <= 0) return null;
  return `${Math.ceil(ms / 1000)}s`;
}

function providerLabel(provider: Provider): string {
  switch (provider.kind) {
    case "opencode":
      return "OpenCode";
    case "codex":
      return "Codex CLI";
    case "claude":
      return "Claude Code";
    case "agy":
      return "Agy CLI";
    case "nvidia":
      return "NVIDIA NIM";
    default:
      return "Groq";
  }
}

export default function ProvidersPage() {
  const router = useRouter();
  const [providers, setProviders] = useState<Provider[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [kind, setKind] = useState<"groq" | "nvidia">("groq");
  const [name, setName] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [baseUrl, setBaseUrl] = useState(DEFAULT_GROQ_URL);
  const [model, setModel] = useState(DEFAULT_GROQ_MODEL);
  const [busy, setBusy] = useState(false);

  const load = useCallback(() => {
    api
      .listProviders()
      .then((res) => setProviders(res.data))
      .catch((err) => setError(err instanceof Error ? err.message : "failed to load"));
  }, []);

  useEffect(() => {
    if (!getMasterKey()) {
      router.replace("/login");
      return;
    }
    load();
  }, [router, load]);

  function onKindChange(newKind: "groq" | "nvidia") {
    setKind(newKind);
    if (newKind === "nvidia") {
      setBaseUrl(DEFAULT_NVIDIA_URL);
      setModel(DEFAULT_NVIDIA_MODEL);
    } else {
      setBaseUrl(DEFAULT_GROQ_URL);
      setModel(DEFAULT_GROQ_MODEL);
    }
  }

  async function create(e: React.FormEvent) {
    e.preventDefault();
    if (!apiKey.trim()) return;
    setBusy(true);
    setError(null);
    try {
      await api.createProvider({
        kind,
        name: name.trim() || undefined,
        api_key: apiKey.trim(),
        base_url: baseUrl.trim() || undefined,
        model: model.trim() || undefined,
      });
      setName("");
      setApiKey("");
      load();
    } catch (err) {
      setError(err instanceof Error ? err.message : `failed to add ${kind === "nvidia" ? "NVIDIA" : "Groq"} key`);
    } finally {
      setBusy(false);
    }
  }

  async function toggle(provider: Provider) {
    try {
      await api.toggleProvider(provider.id, !provider.enabled);
      load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "update failed");
    }
  }

  async function remove(provider: Provider) {
    if (!window.confirm(`Delete provider "${provider.name}"?`)) return;
    try {
      await api.deleteProvider(provider.id);
      load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "delete failed");
    }
  }

  return (
    <Shell>
      <div className="mb-6 flex flex-col gap-2 sm:flex-row sm:items-end sm:justify-between">
        <div>
          <h1 className="text-xl font-bold tracking-tight text-slate-900">Providers &amp; Upstreams</h1>
          <p className="mt-1 text-xs text-slate-500">
            Configure Groq and NVIDIA credentials; requests automatically fail over and route by model modality.
          </p>
        </div>
        <p className="text-xs font-medium text-slate-400">Credentials are encrypted at rest with AES-256-GCM.</p>
      </div>

      {error && <ErrorBox message={error} />}

      <div className="mb-6 grid grid-cols-1 gap-6 xl:grid-cols-[360px_1fr]">
        {/* Add Key Card */}
        <Card title="Add upstream provider key">
          <form onSubmit={create} className="flex flex-col gap-4">
            <div>
              <label className="mb-1.5 block text-xs font-semibold uppercase tracking-wider text-slate-500">
                Provider Type
              </label>
              <select
                value={kind}
                onChange={(e) => onKindChange(e.target.value as "groq" | "nvidia")}
                className="w-full rounded-lg border border-slate-300 bg-slate-50/50 px-3 py-2 text-sm text-slate-900 outline-none transition-all focus:border-indigo-500 focus:bg-white focus:ring-2 focus:ring-indigo-500/20"
              >
                <option value="groq">Groq (Llama 3.3, Qwen 3.6, Mixtral)</option>
                <option value="nvidia">NVIDIA NIM (Nemotron, Gemma, MiniMax)</option>
              </select>
            </div>

            <div>
              <label className="mb-1.5 block text-xs font-semibold uppercase tracking-wider text-slate-500">
                Label (optional)
              </label>
              <input
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder={kind === "nvidia" ? "e.g. NVIDIA Nemotron Primary" : "e.g. Groq Production 01"}
                className="w-full rounded-lg border border-slate-300 bg-slate-50/50 px-3 py-2 text-sm text-slate-900 outline-none transition-all placeholder:text-slate-400 focus:border-indigo-500 focus:bg-white focus:ring-2 focus:ring-indigo-500/20"
              />
            </div>

            <div>
              <label className="mb-1.5 block text-xs font-semibold uppercase tracking-wider text-slate-500">
                {kind === "nvidia" ? "NVIDIA API key" : "Groq API key"}
              </label>
              <input
                type="password"
                value={apiKey}
                onChange={(e) => setApiKey(e.target.value)}
                placeholder={kind === "nvidia" ? "nvapi-…" : "gsk_…"}
                required
                className="w-full rounded-lg border border-slate-300 bg-slate-50/50 px-3 py-2 font-mono text-sm text-slate-900 outline-none transition-all placeholder:text-slate-400 focus:border-indigo-500 focus:bg-white focus:ring-2 focus:ring-indigo-500/20"
              />
            </div>

            <div>
              <label className="mb-1.5 block text-xs font-semibold uppercase tracking-wider text-slate-500">
                Base URL
              </label>
              <input
                value={baseUrl}
                onChange={(e) => setBaseUrl(e.target.value)}
                className="w-full rounded-lg border border-slate-300 bg-slate-50/50 px-3 py-2 font-mono text-xs text-slate-900 outline-none transition-all focus:border-indigo-500 focus:bg-white focus:ring-2 focus:ring-indigo-500/20"
              />
            </div>

            <div>
              <label className="mb-1.5 block text-xs font-semibold uppercase tracking-wider text-slate-500">
                Default model
              </label>
              <input
                value={model}
                onChange={(e) => setModel(e.target.value)}
                className="w-full rounded-lg border border-slate-300 bg-slate-50/50 px-3 py-2 font-mono text-xs text-slate-900 outline-none transition-all focus:border-indigo-500 focus:bg-white focus:ring-2 focus:ring-indigo-500/20"
              />
            </div>

            <button
              type="submit"
              disabled={busy || !apiKey.trim()}
              className="mt-2 rounded-lg bg-indigo-600 py-2.5 text-sm font-semibold text-white shadow-xs transition-all hover:bg-indigo-700 hover:shadow-sm disabled:opacity-40"
            >
              {busy ? "Saving…" : `Save ${kind === "nvidia" ? "NVIDIA" : "Groq"} credential`}
            </button>
          </form>
        </Card>

        {/* Configured Providers Card */}
        <Card title={`Configured upstream nodes (${providers?.length ?? "…"})`}>
          {!providers && <Spinner />}
          {providers && (
            <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
              {providers.map((provider) => {
                const cooldown = cooldownLeft(provider.cooldown_until);
                const isHttp = provider.kind === "groq" || provider.kind === "nvidia";
                const statusTone = !provider.available
                  ? "zinc"
                  : !provider.enabled
                    ? "zinc"
                    : cooldown
                      ? "amber"
                      : provider.healthy
                        ? "green"
                        : "red";
                const statusText = !provider.available
                  ? "unavailable"
                  : !provider.enabled
                    ? "disabled"
                    : cooldown
                      ? `cooldown ${cooldown}`
                      : provider.healthy
                        ? "healthy"
                        : "failing";
                return (
                  <div key={provider.id} className="rounded-xl border border-slate-200/90 bg-white p-4 shadow-xs">
                    <div className="flex items-start justify-between gap-3">
                      <div className="min-w-0">
                        <p className="font-semibold text-slate-900">{provider.name}</p>
                        <p className="mt-0.5 font-mono text-[11px] text-slate-400">{provider.id}</p>
                      </div>
                      <Badge tone={statusTone}>{statusText}</Badge>
                    </div>

                    <dl className="mt-4 space-y-2 text-xs">
                      <div className="flex justify-between gap-4">
                        <dt className="text-slate-400">Type</dt>
                        <dd className="font-medium text-slate-700">{providerLabel(provider)}</dd>
                      </div>
                      <div className="flex justify-between gap-4">
                        <dt className="text-slate-400">Model</dt>
                        <dd className="max-w-[65%] truncate font-mono font-medium text-slate-800">
                          {provider.model || "default"}
                        </dd>
                      </div>
                      {isHttp ? (
                        <div className="flex justify-between gap-4">
                          <dt className="text-slate-400">Credential</dt>
                          <dd className="font-mono text-emerald-600 font-semibold">
                            {provider.api_key_configured ? "encrypted / active" : "missing"}
                          </dd>
                        </div>
                      ) : (
                        <div className="flex justify-between gap-4">
                          <dt className="text-slate-400">Binary</dt>
                          <dd className="max-w-[65%] truncate font-mono text-slate-600">
                            {provider.command ?? "not configured"}
                          </dd>
                        </div>
                      )}
                      <div className="flex justify-between gap-4">
                        <dt className="text-slate-400">Failures</dt>
                        <dd className="font-mono text-slate-700">{provider.failure_count}</dd>
                      </div>
                      <div className="flex justify-between gap-4">
                        <dt className="text-slate-400">Last used</dt>
                        <dd className="font-mono text-slate-600">
                          {provider.last_used_at ? formatTime(provider.last_used_at) : "never"}
                        </dd>
                      </div>
                    </dl>

                    {provider.last_error && (
                      <p className="mt-3 rounded-lg border border-rose-200 bg-rose-50/80 px-3 py-2 font-mono text-[11px] leading-relaxed text-rose-700">
                        {provider.last_error}
                      </p>
                    )}

                    <div className="mt-4 flex gap-2">
                      <button
                        onClick={() => toggle(provider)}
                        className="flex-1 rounded-lg border border-slate-200 bg-white py-1.5 text-xs font-semibold text-slate-700 shadow-xs transition-colors hover:bg-slate-50"
                      >
                        {provider.enabled ? "Disable" : "Enable"}
                      </button>
                      {isHttp && (
                        <button
                          onClick={() => remove(provider)}
                          className="rounded-lg border border-rose-200 bg-white px-3 py-1.5 text-xs font-semibold text-rose-600 shadow-xs transition-colors hover:bg-rose-50"
                        >
                          Delete
                        </button>
                      )}
                    </div>
                  </div>
                );
              })}

              {providers.length === 0 && (
                <p className="py-12 text-center text-sm font-medium text-slate-400 md:col-span-2">
                  No upstream providers configured yet.
                </p>
              )}
            </div>
          )}
        </Card>
      </div>

      <Card title="Agent CLI Sandbox &amp; Safety">
        <p className="text-xs leading-relaxed text-slate-600">
          OpenCode, Codex CLI, Claude Code, and Agy are detected from the router host&apos;s system PATH. The router invokes them non-interactively with bounded timeouts; Codex and Agy use sandbox isolation modes, Claude runs with tools disabled, and all system prompts explicitly forbid unauthorized file system or command alterations.
        </p>
      </Card>
    </Shell>
  );
}
