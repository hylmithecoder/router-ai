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
          <h1 className="text-xl font-semibold">Providers &amp; keys</h1>
          <p className="mt-1 text-sm text-zinc-500">
            Add Groq and NVIDIA keys; the router automatically fails over and routes by model or provider.
          </p>
        </div>
        <p className="text-xs text-zinc-600">Secrets are encrypted in SQLite and never shown again.</p>
      </div>

      {error && <ErrorBox message={error} />}

      <div className="mb-6 grid grid-cols-1 gap-6 xl:grid-cols-[340px_1fr]">
        <Card title="Add HTTP upstream key">
          <form onSubmit={create} className="flex flex-col gap-3">
            <div>
              <label className="mb-1 block text-xs text-zinc-500">Provider Type</label>
              <select
                value={kind}
                onChange={(e) => onKindChange(e.target.value as "groq" | "nvidia")}
                className="w-full rounded-lg border border-zinc-700 bg-zinc-950 px-3 py-2 text-sm text-zinc-200 outline-none focus:border-emerald-500"
              >
                <option value="groq">Groq (Llama, Mixtral)</option>
                <option value="nvidia">NVIDIA NIM (Nemotron, Omni)</option>
              </select>
            </div>
            <div>
              <label className="mb-1 block text-xs text-zinc-500">Label (optional)</label>
              <input
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder={kind === "nvidia" ? "NVIDIA Nemotron 01" : "Groq production 03"}
                className="w-full rounded-lg border border-zinc-700 bg-zinc-950 px-3 py-2 text-sm outline-none placeholder:text-zinc-600 focus:border-emerald-500"
              />
            </div>
            <div>
              <label className="mb-1 block text-xs text-zinc-500">
                {kind === "nvidia" ? "NVIDIA API key" : "Groq API key"}
              </label>
              <input
                type="password"
                value={apiKey}
                onChange={(e) => setApiKey(e.target.value)}
                placeholder={kind === "nvidia" ? "nvapi-…" : "gsk_…"}
                required
                className="w-full rounded-lg border border-zinc-700 bg-zinc-950 px-3 py-2 font-mono text-sm outline-none placeholder:text-zinc-600 focus:border-emerald-500"
              />
            </div>
            <div>
              <label className="mb-1 block text-xs text-zinc-500">Base URL</label>
              <input
                value={baseUrl}
                onChange={(e) => setBaseUrl(e.target.value)}
                className="w-full rounded-lg border border-zinc-700 bg-zinc-950 px-3 py-2 font-mono text-xs outline-none focus:border-emerald-500"
              />
            </div>
            <div>
              <label className="mb-1 block text-xs text-zinc-500">Default model</label>
              <input
                value={model}
                onChange={(e) => setModel(e.target.value)}
                className="w-full rounded-lg border border-zinc-700 bg-zinc-950 px-3 py-2 font-mono text-xs outline-none focus:border-emerald-500"
              />
            </div>
            <button
              type="submit"
              disabled={busy || !apiKey.trim()}
              className="rounded-lg bg-emerald-500 py-2 text-sm font-semibold text-zinc-950 transition-colors hover:bg-emerald-400 disabled:opacity-40"
            >
              {busy ? "Saving…" : `Save ${kind === "nvidia" ? "NVIDIA" : "Groq"} key`}
            </button>
          </form>
        </Card>

        <Card title={`Configured providers (${providers?.length ?? "…"})`}>
          {!providers && <Spinner />}
          {providers && (
            <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
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
                  <div key={provider.id} className="rounded-xl border border-zinc-800 bg-zinc-950/60 p-4">
                    <div className="flex items-start justify-between gap-3">
                      <div className="min-w-0">
                        <p className="font-semibold text-zinc-200">{provider.name}</p>
                        <p className="mt-0.5 font-mono text-[11px] text-zinc-600">{provider.id}</p>
                      </div>
                      <Badge tone={statusTone}>{statusText}</Badge>
                    </div>

                    <dl className="mt-4 space-y-1.5 text-xs">
                      <div className="flex justify-between gap-4">
                        <dt className="text-zinc-500">Type</dt>
                        <dd className="text-zinc-300">{providerLabel(provider)}</dd>
                      </div>
                      <div className="flex justify-between gap-4">
                        <dt className="text-zinc-500">Model</dt>
                        <dd className="max-w-[65%] truncate font-mono text-zinc-300">
                          {provider.model || "default"}
                        </dd>
                      </div>
                      {isHttp ? (
                        <div className="flex justify-between gap-4">
                          <dt className="text-zinc-500">Credential</dt>
                          <dd className="font-mono text-emerald-400">
                            {provider.api_key_configured ? "encrypted / configured" : "missing"}
                          </dd>
                        </div>
                      ) : (
                        <div className="flex justify-between gap-4">
                          <dt className="text-zinc-500">Binary</dt>
                          <dd className="max-w-[65%] truncate font-mono text-zinc-400">
                            {provider.command ?? "not configured"}
                          </dd>
                        </div>
                      )}
                      <div className="flex justify-between gap-4">
                        <dt className="text-zinc-500">Failures</dt>
                        <dd className="font-mono text-zinc-300">{provider.failure_count}</dd>
                      </div>
                      <div className="flex justify-between gap-4">
                        <dt className="text-zinc-500">Last used</dt>
                        <dd className="font-mono text-zinc-400">
                          {provider.last_used_at ? formatTime(provider.last_used_at) : "never"}
                        </dd>
                      </div>
                    </dl>

                    {provider.last_error && (
                      <p className="mt-3 rounded-lg border border-red-500/20 bg-red-500/10 px-3 py-2 font-mono text-[11px] leading-relaxed text-red-400">
                        {provider.last_error}
                      </p>
                    )}

                    <div className="mt-4 flex gap-2">
                      <button
                        onClick={() => toggle(provider)}
                        className="flex-1 rounded-lg border border-zinc-700 py-1.5 text-sm text-zinc-300 transition-colors hover:bg-zinc-800"
                      >
                        {provider.enabled ? "Disable" : "Enable"}
                      </button>
                      {isHttp && (
                        <button
                          onClick={() => remove(provider)}
                          className="rounded-lg border border-red-500/30 px-3 py-1.5 text-sm text-red-400 transition-colors hover:bg-red-500/10"
                        >
                          Delete
                        </button>
                      )}
                    </div>
                  </div>
                );
              })}

              {providers.length === 0 && (
                <p className="py-10 text-center text-sm text-zinc-500 md:col-span-2">
                  No providers configured. Add a Groq key or configure the server on a host with an agent CLI.
                </p>
              )}
            </div>
          )}
        </Card>
      </div>

      <Card title="Local agent safety">
        <p className="text-sm leading-relaxed text-zinc-400">
          OpenCode, Codex CLI, Claude Code, and Agy are detected from the router host&apos;s PATH. The router invokes them non-interactively with bounded timeouts; Codex and Agy use sandbox modes, Claude runs with tools disabled, and the prompt explicitly forbids file or command changes.
        </p>
      </Card>
    </Shell>
  );
}
