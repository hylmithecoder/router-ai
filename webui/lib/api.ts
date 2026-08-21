// Empty (default) = same origin as the dashboard itself: the single-binary
// mode serves both API and static dashboard from one port. In dev, set
// NEXT_PUBLIC_ROUTER_API_URL=http://127.0.0.1:5790 in webui/.env.local.
const API_URL = (process.env.NEXT_PUBLIC_ROUTER_API_URL ?? "").replace(/\/+$/, "");

const KEY_STORAGE = "router_master_key";

export function getMasterKey(): string | null {
  if (typeof window === "undefined") return null;
  return window.localStorage.getItem(KEY_STORAGE);
}

export function setMasterKey(key: string) {
  window.localStorage.setItem(KEY_STORAGE, key);
}

export function clearMasterKey() {
  window.localStorage.removeItem(KEY_STORAGE);
}

export interface ApiErrorBody {
  success: false;
  error: { message: string; status: number };
}

export class ApiError extends Error {
  status: number;
  constructor(status: number, message: string) {
    super(message);
    this.status = status;
  }
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const key = getMasterKey();
  const res = await fetch(`${API_URL}${path}`, {
    ...init,
    headers: {
      "content-type": "application/json",
      ...(key ? { authorization: `Bearer ${key}` } : {}),
      ...init?.headers,
    },
  });

  const body = await res.json().catch(() => null);
  if (!res.ok) {
    const msg =
      body?.error?.message ?? `Request failed with status ${res.status}`;
    throw new ApiError(res.status, msg);
  }
  return body as T;
}

export interface ApiResponse<T> {
  success: boolean;
  data: T;
  message?: string;
}

// ---- types matching the backend admin API ----

export interface UsageSummary {
  today: {
    requests: number;
    total_tokens: number;
    prompt_tokens: number;
    completion_tokens: number;
  };
  by_key: {
    key_id: string;
    key_name: string;
    requests: number;
    total_tokens: number;
    quota_daily_tokens: number;
    used_today: number;
  }[];
  by_provider: { provider: string; requests: number; total_tokens: number }[];
  by_day: { day: string; tokens: number }[];
  daily_quota_tokens: number;
}

export interface UsageLogRow {
  id: number;
  key_name: string;
  model: string;
  provider: string;
  prompt_tokens: number;
  completion_tokens: number;
  total_tokens: number;
  latency_ms: number;
  status: number;
  created_at: string;
}

export interface UsageLogPayload {
  rows: UsageLogRow[];
  limit: number;
  offset: number;
}

export interface ApiKey {
  id: string;
  name: string;
  key_prefix: string;
  quota_daily_tokens: number;
  enabled: boolean;
  created_at: string;
}

export interface Provider {
  id: string;
  kind: "groq" | "opencode" | "codex" | "claude" | "agy" | string;
  name: string;
  base_url: string;
  model: string;
  command: string | null;
  api_key_configured: boolean;
  enabled: boolean;
  failure_count: number;
  cooldown_until: string | null;
  last_error: string | null;
  last_used_at: string | null;
  healthy: boolean;
  available: boolean;
}

export const api = {
  usageSummary: () =>
    request<ApiResponse<UsageSummary>>("/api/v1/admin/usage/summary"),
  usageLog: (limit = 50, offset = 0) =>
    request<ApiResponse<UsageLogPayload>>(
      `/api/v1/admin/usage/log?limit=${limit}&offset=${offset}`,
    ),
  listKeys: () => request<ApiResponse<ApiKey[]>>("/api/v1/admin/keys"),
  createKey: (name: string, quota: number) =>
    request<ApiResponse<{ id: string; name: string; key: string; quota_daily_tokens: number }>>(
      "/api/v1/admin/keys",
      { method: "POST", body: JSON.stringify({ name, quota_daily_tokens: quota }) },
    ),
  updateKey: (id: string, patch: { quota_daily_tokens?: number; enabled?: boolean }) =>
    request<ApiResponse<unknown>>(`/api/v1/admin/keys/${id}`, {
      method: "PATCH",
      body: JSON.stringify(patch),
    }),
  deleteKey: (id: string) =>
    request<ApiResponse<unknown>>(`/api/v1/admin/keys/${id}`, { method: "DELETE" }),
  listProviders: () => request<ApiResponse<Provider[]>>("/api/v1/admin/providers"),
  createProvider: (payload: {
    kind: "groq";
    name?: string;
    api_key: string;
    base_url?: string;
    model?: string;
  }) =>
    request<ApiResponse<{ id: string; kind: string; name: string; base_url: string; model: string }>>(
      "/api/v1/admin/providers",
      { method: "POST", body: JSON.stringify(payload) },
    ),
  updateProvider: (
    id: string,
    patch: {
      name?: string;
      api_key?: string;
      base_url?: string;
      model?: string;
    },
  ) =>
    request<ApiResponse<unknown>>(`/api/v1/admin/providers/${id}`, {
      method: "PATCH",
      body: JSON.stringify(patch),
    }),
  deleteProvider: (id: string) =>
    request<ApiResponse<unknown>>(`/api/v1/admin/providers/${id}`, { method: "DELETE" }),
  toggleProvider: (id: string, enabled: boolean) =>
    request<ApiResponse<unknown>>(`/api/v1/admin/providers/${id}/toggle`, {
      method: "POST",
      body: JSON.stringify({ enabled }),
    }),
};

export function formatTokens(n: number): string {
  return new Intl.NumberFormat("en-US").format(n);
}

export function formatTime(iso: string): string {
  const d = new Date(iso);
  return d.toLocaleString();
}

export function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  return `${(ms / 1000).toFixed(1)}s`;
}
