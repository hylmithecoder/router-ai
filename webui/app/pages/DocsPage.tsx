"use client";

import { useState, useEffect } from "react";
import Shell from "@/components/Shell";
import { Badge, Card } from "@/components/ui";
import { getMasterKey } from "@/lib/api";

interface ApiEndpoint {
  id: string;
  method: "POST" | "GET" | "DELETE" | "PATCH";
  path: string;
  category: "Chat & AI" | "Computer Vision" | "System" | "Admin";
  summary: string;
  description: string;
  auth: "API Key (Bearer)" | "Master Key (Bearer)" | "None";
  defaultBody?: string;
  presets?: { name: string; body: string }[];
}

const ENDPOINTS: ApiEndpoint[] = [
  {
    id: "chat-completions",
    method: "POST",
    path: "/api/v1/chat/completions",
    category: "Chat & AI",
    summary: "Create Chat Completion (OpenAI-compatible)",
    description:
      "Routes prompt across NVIDIA NIM, Groq, or local CLI agents with automatic failover and model selection.",
    auth: "API Key (Bearer)",
    defaultBody: JSON.stringify(
      {
        model: "nvidia/nemotron-3-nano-omni-30b-a3b-reasoning",
        messages: [
          {
            role: "user",
            content:
              "Explain hybrid Mamba-Transformer MoE architecture in 2 bullet points.",
          },
        ],
        temperature: 0.7,
        max_tokens: 512,
      },
      null,
      2,
    ),
    presets: [
      {
        name: "Groq Qwen 3.6 27B (Ultra-fast Text)",
        body: JSON.stringify(
          {
            model: "qwen/qwen3.6-27b",
            messages: [
              {
                role: "user",
                content: "Explain Rust ownership in 2 simple sentences.",
              },
            ],
            temperature: 0.6,
          },
          null,
          2,
        ),
      },
      {
        name: "Groq OpenAI GPT-OSS 120B",
        body: JSON.stringify(
          {
            model: "openai/gpt-oss-120b",
            messages: [
              {
                role: "user",
                content: "Write a concise summary of distributed computing.",
              },
            ],
            temperature: 0.7,
          },
          null,
          2,
        ),
      },
      {
        name: "Groq Llama 3.3 70B Versatile",
        body: JSON.stringify(
          {
            model: "llama-3.3-70b-versatile",
            messages: [
              {
                role: "user",
                content: "Provide 3 key optimizations for SQLite databases.",
              },
            ],
            temperature: 0.7,
          },
          null,
          2,
        ),
      },
      {
        name: "Groq DeepSeek R1 Distill 70B",
        body: JSON.stringify(
          {
            model: "deepseek-r1-distill-llama-70b",
            messages: [
              {
                role: "user",
                content: "Solve step-by-step: How many r's are in strawberry?",
              },
            ],
            temperature: 0.6,
          },
          null,
          2,
        ),
      },
      {
        name: "NVIDIA Nemotron 550B MoE (Reasoning)",
        body: JSON.stringify(
          {
            model: "nvidia/nemotron-3-ultra-550b-a55b",
            messages: [
              {
                role: "user",
                content:
                  "Design a high-throughput async pipeline architecture in Rust.",
              },
            ],
            temperature: 0.6,
          },
          null,
          2,
        ),
      },
      {
        name: "NVIDIA Google DiffusionGemma 26B (Vision)",
        body: JSON.stringify(
          {
            model: "google/diffusiongemma-26b-a4b-it",
            messages: [
              {
                role: "user",
                content:
                  "Describe the importance of multimodal perception in autonomous systems.",
              },
            ],
            temperature: 0.7,
          },
          null,
          2,
        ),
      },
      {
        name: "Google Gemma-4 31B (Multimodal Thinking)",
        body: JSON.stringify(
          {
            model: "google/gemma-4-31b-it",
            messages: [
              {
                role: "user",
                content: [
                  { type: "text", text: "What is in this image?" },
                  {
                    type: "image_url",
                    image_url: {
                      url: "https://assets.ngc.nvidia.com/products/api-catalog/phi-3-5-vision/example1b.jpg",
                    },
                  },
                ],
              },
            ],
            chat_template_kwargs: { enable_thinking: true },
            max_tokens: 16384,
            temperature: 1,
            top_p: 0.95,
          },
          null,
          2,
        ),
      },
    ],
  },
  {
    id: "ocr-licenseplate",
    method: "POST",
    path: "/api/v1/ocr/licenseplate",
    category: "Computer Vision",
    summary: "Vehicle License Plate OCR",
    description:
      "Multimodal vision endpoint that analyzes vehicle images (base64 or URL) and extracts structured license plate numbers, vehicle type, and details.",
    auth: "API Key (Bearer)",
    defaultBody: JSON.stringify(
      {
        image:
          "https://upload.wikimedia.org/wikipedia/commons/thumb/d/d0/Indonesian_license_plate_B_1234_ABC.jpg/640px-Indonesian_license_plate_B_1234_ABC.jpg",
        instruction:
          "Ekstrak plat nomor kendaraan dan tipe kendaraan dengan teliti.",
        model: "nvidia/nemotron-3-nano-omni-30b-a3b-reasoning",
      },
      null,
      2,
    ),
    presets: [
      {
        name: "Auto-Pick Model (Smart)",
        body: JSON.stringify(
          {
            image:
              "https://images.unsplash.com/photo-1549399542-7e3f8b79c341?w=800",
            instruction:
              "Ekstrak plat nomor kendaraan dan tipe kendaraan dengan teliti.",
            model: "auto",
          },
          null,
          2,
        ),
      },
      {
        name: "Google DiffusionGemma 26B (Vision)",
        body: JSON.stringify(
          {
            image:
              "https://images.unsplash.com/photo-1549399542-7e3f8b79c341?w=800",
            instruction: "Ekstrak plat nomor kendaraan dan tipe kendaraan.",
            model: "google/diffusiongemma-26b-a4b-it",
          },
          null,
          2,
        ),
      },
      {
        name: "Google Gemma-4 31B (Vision)",
        body: JSON.stringify(
          {
            image:
              "https://images.unsplash.com/photo-1549399542-7e3f8b79c341?w=800",
            instruction: "Ekstrak plat nomor kendaraan dan warna kendaraan.",
            model: "google/gemma-4-31b-it",
          },
          null,
          2,
        ),
      },
      {
        name: "MiniMax-M3 427B (Vision)",
        body: JSON.stringify(
          {
            image:
              "https://images.unsplash.com/photo-1549399542-7e3f8b79c341?w=800",
            instruction: "Ekstrak nomor plat dan jenis kendaraan.",
            model: "minimaxai/minimax-m3",
          },
          null,
          2,
        ),
      },
      {
        name: "NVIDIA Nemotron 30B (Omni)",
        body: JSON.stringify(
          {
            image:
              "https://images.unsplash.com/photo-1549399542-7e3f8b79c341?w=800",
            instruction: "Ekstrak nomor polisi kendaraan.",
            model: "nvidia/nemotron-3-nano-omni-30b-a3b-reasoning",
          },
          null,
          2,
        ),
      },
      {
        name: "Raw Base64 Input",
        body: JSON.stringify(
          {
            image:
              "data:image/jpeg;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==",
            instruction: "Read vehicle license plate.",
            model: "auto",
          },
          null,
          2,
        ),
      },
    ],
  },
  {
    id: "list-models",
    method: "GET",
    path: "/api/v1/models",
    category: "Chat & AI",
    summary: "List Available Models",
    description:
      "Returns all active upstream models and provider aliases served by the router.",
    auth: "API Key (Bearer)",
  },
  {
    id: "unified-chat",
    method: "POST",
    path: "/api/v1/chat",
    category: "Chat & AI",
    summary: "Unified Chat Router Endpoint",
    description:
      "Unified router endpoint with automatic fallback across HTTP providers (NVIDIA/Groq) and local CLI agents (OpenCode/Codex/Claude/Agy).",
    auth: "API Key (Bearer)",
    defaultBody: JSON.stringify(
      {
        provider: "auto",
        messages: [
          {
            role: "user",
            content: "Summarize quantum computing in 1 sentence.",
          },
        ],
      },
      null,
      2,
    ),
  },
  {
    id: "health",
    method: "GET",
    path: "/health",
    category: "System",
    summary: "Liveness Probe",
    description: "Checks if the HTTP server is alive and responding.",
    auth: "None",
  },
  {
    id: "health-ready",
    method: "GET",
    path: "/health/ready",
    category: "System",
    summary: "Readiness Probe",
    description: "Verifies SQLite database connectivity and router readiness.",
    auth: "None",
  },
  {
    id: "admin-summary",
    method: "GET",
    path: "/api/v1/admin/usage/summary",
    category: "Admin",
    summary: "Usage Analytics Summary",
    description:
      "Aggregated today metrics, per-key usage, provider distribution, and 7-day volume.",
    auth: "Master Key (Bearer)",
  },
  {
    id: "admin-dns",
    method: "GET",
    path: "/api/v1/admin/dns?host=ilmeee.com&server=1.1.1.1",
    category: "System",
    summary: "Async DNS Dig Query (1.1.1.1)",
    description:
      "Performs real-time UDP DNS queries directly against 1.1.1.1 / 8.8.8.8 to bypass ISP filters and resolve domain IPs.",
    auth: "Master Key (Bearer)",
  },
];

export default function ApiDocsPage() {
  const [selectedEndpoint, setSelectedEndpoint] = useState<ApiEndpoint>(
    ENDPOINTS[0],
  );
  const [apiKey, setApiKey] = useState("");
  const [requestBody, setRequestBody] = useState(
    selectedEndpoint.defaultBody || "",
  );
  const [responseStatus, setResponseStatus] = useState<number | null>(null);
  const [responseDuration, setResponseDuration] = useState<number | null>(null);
  const [responseBody, setResponseBody] = useState<string>("");
  const [loading, setLoading] = useState(false);
  const [copiedTab, setCopiedTab] = useState<string | null>(null);
  const [activeLang, setActiveLang] = useState<
    "curl" | "python" | "typescript" | "rust"
  >("curl");

  useEffect(() => {
    const master = getMasterKey();
    if (master) {
      setApiKey(master);
    }
  }, []);

  function handleSelectEndpoint(ep: ApiEndpoint) {
    setSelectedEndpoint(ep);
    setRequestBody(ep.defaultBody || "");
    setResponseStatus(null);
    setResponseBody("");
    setResponseDuration(null);
  }

  function handlePreset(body: string) {
    setRequestBody(body);
  }

  async function executeRequest() {
    setLoading(true);
    setResponseBody("");
    setResponseStatus(null);
    const start = performance.now();

    try {
      const headers: Record<string, string> = {};
      if (
        selectedEndpoint.method === "POST" ||
        selectedEndpoint.method === "PATCH"
      ) {
        headers["Content-Type"] = "application/json";
      }
      if (selectedEndpoint.auth !== "None" && apiKey.trim()) {
        headers["Authorization"] = `Bearer ${apiKey.trim()}`;
      }

      const res = await fetch(selectedEndpoint.path, {
        method: selectedEndpoint.method,
        headers,
        body:
          selectedEndpoint.method === "POST" ||
          selectedEndpoint.method === "PATCH"
            ? requestBody
            : undefined,
      });

      const elapsed = Math.round(performance.now() - start);
      setResponseDuration(elapsed);
      setResponseStatus(res.status);

      const contentType = res.headers.get("content-type") || "";
      if (contentType.includes("application/json")) {
        const data = await res.json();
        setResponseBody(JSON.stringify(data, null, 2));
      } else {
        const text = await res.text();
        setResponseBody(text);
      }
    } catch (err: unknown) {
      const elapsed = Math.round(performance.now() - start);
      setResponseDuration(elapsed);
      setResponseStatus(0);
      setResponseBody(
        `Network Error: ${err instanceof Error ? err.message : String(err)}`,
      );
    } finally {
      setLoading(false);
    }
  }

  function generateSnippet(
    lang: "curl" | "python" | "typescript" | "rust",
  ): string {
    const url = `http://127.0.0.1:5790${selectedEndpoint.path}`;
    const token = apiKey.trim() || "sk-router-your-key";

    if (lang === "curl") {
      if (selectedEndpoint.method === "GET") {
        if (selectedEndpoint.auth === "None") {
          return `curl -X GET "${url}"`;
        }
        return `curl -X GET "${url}" \\\n  -H "Authorization: Bearer ${token}"`;
      }
      return `curl -X POST "${url}" \\\n  -H "Authorization: Bearer ${token}" \\\n  -H "Content-Type: application/json" \\\n  -d '${requestBody.replace(/'/g, "'\\''")}'`;
    }

    if (lang === "python") {
      if (selectedEndpoint.method === "GET") {
        return `import requests\n\nresponse = requests.get(\n    "${url}",\n    headers={"Authorization": "Bearer ${token}"}\n)\nprint(response.json())`;
      }
      return `import requests\n\npayload = ${requestBody || "{}"}\n\nresponse = requests.post(\n    "${url}",\n    headers={\n        "Authorization": "Bearer ${token}",\n        "Content-Type": "application/json"\n    },\n    json=payload\n)\nprint(response.json())`;
    }

    if (lang === "typescript") {
      if (selectedEndpoint.method === "GET") {
        return `const response = await fetch("${url}", {\n  headers: { "Authorization": "Bearer ${token}" }\n});\nconst data = await response.json();\nconsole.log(data);`;
      }
      return `const response = await fetch("${url}", {\n  method: "POST",\n  headers: {\n    "Authorization": "Bearer ${token}",\n    "Content-Type": "application/json"\n  },\n  body: JSON.stringify(${requestBody || "{}"})\n});\nconst data = await response.json();\nconsole.log(data);`;
    }

    if (lang === "rust") {
      return `use reqwest::Client;\nuse serde_json::json;\n\n#[tokio::main]\nasync fn main() -> Result<(), Box<dyn std::error::Error>> {\n    let client = Client::new();\n    let res = client\n        .post("${url}")\n        .bearer_auth("${token}")\n        .json(&json!(${requestBody || "{}"}))\n        .send()\n        .await?\n        .text()\n        .await?;\n    println!("{res}");\n    Ok(())\n}`;
    }

    return "";
  }

  function copyCode(text: string, label: string) {
    navigator.clipboard.writeText(text);
    setCopiedTab(label);
    setTimeout(() => setCopiedTab(null), 2000);
  }

  function downloadOpenApiSpec() {
    const spec = {
      openapi: "3.0.3",
      info: {
        title: "Router API AI",
        version: "0.1.0",
        description:
          "High-performance AI router gateway with multi-provider scaling (NVIDIA NIM, Groq, Local CLI Agents) and Multimodal License Plate OCR.",
      },
      servers: [{ url: "http://127.0.0.1:5790" }],
      paths: {
        "/api/v1/chat/completions": {
          post: {
            summary: "Chat Completions",
            security: [{ BearerAuth: [] }],
            requestBody: {
              content: {
                "application/json": {
                  schema: { type: "object" },
                },
              },
            },
            responses: { "200": { description: "Successful Completion" } },
          },
        },
        "/api/v1/ocr/licenseplate": {
          post: {
            summary: "Vehicle License Plate OCR",
            security: [{ BearerAuth: [] }],
            requestBody: {
              content: {
                "application/json": {
                  schema: {
                    type: "object",
                    required: ["image"],
                    properties: {
                      image: { type: "string" },
                      instruction: { type: "string" },
                      model: { type: "string" },
                    },
                  },
                },
              },
            },
            responses: { "200": { description: "Structured OCR Data" } },
          },
        },
      },
    };

    const blob = new Blob([JSON.stringify(spec, null, 2)], {
      type: "application/json",
    });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = "router-api-ai-openapi.json";
    a.click();
    URL.revokeObjectURL(url);
  }

  return (
    <Shell>
      <div className="flex flex-col gap-6">
        <div className="flex flex-wrap items-center justify-between gap-4">
          <div>
            <h1 className="text-2xl font-bold text-zinc-100">
              API Documentation & Swagger Playground
            </h1>
            <p className="mt-1 text-sm text-zinc-400">
              Interactive OpenAPI reference, live testing console, and code
              generator for router-api-ai.
            </p>
          </div>
          <button
            onClick={downloadOpenApiSpec}
            className="flex items-center gap-2 rounded-lg border border-zinc-700 bg-zinc-800/80 px-3 py-2 text-xs font-medium text-zinc-200 transition-colors hover:bg-zinc-700 hover:text-white"
          >
            Download OpenAPI 3.0 (JSON)
          </button>
        </div>

        {/* Global Key Input */}
        <Card className="bg-zinc-900/40">
          <div className="flex flex-wrap items-center gap-4">
            <div className="flex items-center gap-2">
              <span className="text-xs font-semibold uppercase tracking-wider text-zinc-400">
                Authorization Token
              </span>
              <Badge tone={apiKey ? "green" : "amber"}>
                {apiKey ? "Configured" : "Not Set"}
              </Badge>
            </div>
            <div className="flex-1 min-w-[280px]">
              <input
                type="text"
                value={apiKey}
                onChange={(e) => setApiKey(e.target.value)}
                placeholder="sk-router-... or master key"
                className="w-full rounded-md border border-zinc-700 bg-zinc-950 px-3 py-1.5 font-mono text-xs text-zinc-200 focus:border-emerald-500 focus:outline-none"
              />
            </div>
            <p className="text-xs text-zinc-500">
              Used automatically in the Authorization Bearer header for live
              testing.
            </p>
          </div>
        </Card>

        {/* Main Grid: Endpoints List + Playground */}
        <div className="grid grid-cols-1 gap-6 lg:grid-cols-12">
          {/* Endpoint Sidebar */}
          <div className="flex flex-col gap-2 lg:col-span-4">
            <h2 className="px-1 text-xs font-bold uppercase tracking-wider text-zinc-500">
              Endpoints
            </h2>
            <div className="flex flex-col gap-1.5 overflow-y-auto max-h-[700px] pr-1">
              {ENDPOINTS.map((ep) => {
                const selected = selectedEndpoint.id === ep.id;
                const methodColor =
                  ep.method === "POST"
                    ? "text-emerald-400 bg-emerald-500/10 border-emerald-500/30"
                    : ep.method === "GET"
                      ? "text-sky-400 bg-sky-500/10 border-sky-500/30"
                      : "text-zinc-400 bg-zinc-800 border-zinc-700";

                return (
                  <button
                    key={ep.id}
                    onClick={() => handleSelectEndpoint(ep)}
                    className={`flex flex-col gap-1 rounded-lg border p-3 text-left transition-colors ${
                      selected
                        ? "border-emerald-500/60 bg-zinc-900 shadow-md"
                        : "border-zinc-800/80 bg-zinc-900/30 hover:border-zinc-700 hover:bg-zinc-900/60"
                    }`}
                  >
                    <div className="flex items-center gap-2">
                      <span
                        className={`rounded border px-1.5 py-0.5 font-mono text-[10px] font-bold ${methodColor}`}
                      >
                        {ep.method}
                      </span>
                      <span className="font-mono text-xs font-semibold text-zinc-200">
                        {ep.path}
                      </span>
                    </div>
                    <p className="text-xs text-zinc-400 line-clamp-1">
                      {ep.summary}
                    </p>
                  </button>
                );
              })}
            </div>
          </div>

          {/* Interactive Playground & Details */}
          <div className="flex flex-col gap-6 lg:col-span-8">
            <Card>
              {/* Endpoint Header */}
              <div className="flex flex-wrap items-start justify-between gap-3 border-b border-zinc-800 pb-4">
                <div className="flex flex-col gap-1.5">
                  <div className="flex items-center gap-2">
                    <span
                      className={`rounded px-2 py-0.5 font-mono text-xs font-bold ${
                        selectedEndpoint.method === "POST"
                          ? "bg-emerald-500/20 text-emerald-400"
                          : "bg-sky-500/20 text-sky-400"
                      }`}
                    >
                      {selectedEndpoint.method}
                    </span>
                    <span className="font-mono text-sm font-bold text-zinc-100">
                      {selectedEndpoint.path}
                    </span>
                  </div>
                  <p className="text-sm text-zinc-300">
                    {selectedEndpoint.description}
                  </p>
                </div>
                <Badge
                  tone={selectedEndpoint.auth === "None" ? "zinc" : "green"}
                >
                  {selectedEndpoint.auth}
                </Badge>
              </div>

              {/* Presets if available */}
              {selectedEndpoint.presets &&
                selectedEndpoint.presets.length > 0 && (
                  <div className="mt-4 flex flex-wrap items-center gap-2">
                    <span className="text-xs font-medium text-zinc-400">
                      Quick Presets:
                    </span>
                    {selectedEndpoint.presets.map((preset) => (
                      <button
                        key={preset.name}
                        onClick={() => handlePreset(preset.body)}
                        className="rounded border border-zinc-700 bg-zinc-800 px-2 py-1 text-xs text-zinc-200 transition-colors hover:border-emerald-500 hover:text-emerald-400"
                      >
                        {preset.name}
                      </button>
                    ))}
                  </div>
                )}

              {/* Request Body Editor */}
              {selectedEndpoint.method !== "GET" && (
                <div className="mt-4 flex flex-col gap-2">
                  <label className="text-xs font-semibold uppercase tracking-wider text-zinc-400">
                    Request Body (JSON)
                  </label>
                  <textarea
                    rows={8}
                    value={requestBody}
                    onChange={(e) => setRequestBody(e.target.value)}
                    className="w-full rounded-lg border border-zinc-800 bg-zinc-950 p-3 font-mono text-xs text-zinc-200 focus:border-emerald-500 focus:outline-none"
                  />
                </div>
              )}

              {/* Action Bar */}
              <div className="mt-4 flex flex-wrap items-center justify-between gap-3 pt-2">
                <button
                  onClick={executeRequest}
                  disabled={loading}
                  className="flex items-center gap-2 rounded-lg bg-emerald-500 px-5 py-2 font-medium text-sm text-zinc-950 transition-colors hover:bg-emerald-400 disabled:opacity-50 font-semibold"
                >
                  {loading ? (
                    <>
                      <div className="h-4 w-4 animate-spin rounded-full border-2 border-zinc-900 border-t-transparent" />
                      Executing...
                    </>
                  ) : (
                    "Send Request"
                  )}
                </button>
                {responseDuration !== null && (
                  <span className="font-mono text-xs text-zinc-400">
                    Latency:{" "}
                    <strong className="text-zinc-200">
                      {responseDuration} ms
                    </strong>
                  </span>
                )}
              </div>

              {/* Response Section */}
              {responseStatus !== null && (
                <div className="mt-6 flex flex-col gap-2 border-t border-zinc-800 pt-4">
                  <div className="flex items-center justify-between">
                    <span className="text-xs font-bold uppercase tracking-wider text-zinc-400">
                      Response Output
                    </span>
                    <Badge
                      tone={
                        responseStatus >= 200 && responseStatus < 300
                          ? "green"
                          : responseStatus === 429
                            ? "amber"
                            : "red"
                      }
                    >
                      Status {responseStatus}
                    </Badge>
                  </div>
                  <pre className="max-h-96 overflow-auto rounded-lg border border-zinc-800 bg-zinc-950 p-3 font-mono text-xs text-zinc-200">
                    {responseBody}
                  </pre>
                </div>
              )}
            </Card>

            {/* Code Snippets Generator */}
            <Card title="Client Code Snippets">
              <div className="flex flex-col gap-3">
                <div className="flex items-center justify-between border-b border-zinc-800 pb-2">
                  <div className="flex gap-2">
                    {(["curl", "python", "typescript", "rust"] as const).map(
                      (lang) => (
                        <button
                          key={lang}
                          onClick={() => setActiveLang(lang)}
                          className={`rounded px-2.5 py-1 text-xs font-semibold transition-colors ${
                            activeLang === lang
                              ? "bg-zinc-800 text-emerald-400"
                              : "text-zinc-400 hover:text-zinc-200"
                          }`}
                        >
                          {lang.toUpperCase()}
                        </button>
                      ),
                    )}
                  </div>
                  <button
                    onClick={() =>
                      copyCode(generateSnippet(activeLang), activeLang)
                    }
                    className="text-xs text-zinc-400 hover:text-white"
                  >
                    {copiedTab === activeLang ? "Copied!" : "Copy Code"}
                  </button>
                </div>
                <pre className="overflow-x-auto rounded-lg bg-zinc-950 p-3 font-mono text-xs text-zinc-300">
                  {generateSnippet(activeLang)}
                </pre>
              </div>
            </Card>
          </div>
        </div>
      </div>
    </Shell>
  );
}
