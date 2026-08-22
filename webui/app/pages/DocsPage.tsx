"use client";

import { useState, useEffect } from "react";
import Shell from "@/components/Shell";
import { Badge, Card } from "@/components/ui";
import { getMasterKey } from "@/lib/api";
import {
  LuDownload,
  LuCopy,
  LuCheck,
  LuSend,
  LuKey,
  LuCode,
  LuZap,
  LuFileText,
} from "react-icons/lu";

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
        name: "Groq LLaMA 3.3 70B (Versatile)",
        body: JSON.stringify(
          {
            model: "llama-3.3-70b-versatile",
            messages: [
              {
                role: "user",
                content: "List top 3 advantages of building on Linux with Rust.",
              },
            ],
          },
          null,
          2,
        ),
      },
      {
        name: "Groq DeepSeek R1 Distill 70B (Reasoning)",
        body: JSON.stringify(
          {
            model: "deepseek-r1-distill-llama-70b",
            messages: [
              {
                role: "user",
                content: "Solve: What is the sum of integers from 1 to 100?",
              },
            ],
          },
          null,
          2,
        ),
      },
      {
        name: "Google DiffusionGemma 26B (Vision & OCR)",
        body: JSON.stringify(
          {
            model: "google/diffusiongemma-26b-a4b-it",
            messages: [
              {
                role: "user",
                content: [
                  {
                    type: "text",
                    text: "Identify objects and read visible text in this scene.",
                  },
                  {
                    type: "image_url",
                    image_url: {
                      url: "https://assets.ngc.nvidia.com/products/api-catalog/phi-3-5-vision/example1b.jpg",
                    },
                  },
                ],
              },
            ],
            max_tokens: 1024,
          },
          null,
          2,
        ),
      },
      {
        name: "MiniMax-M3 427B (Dense Vision)",
        body: JSON.stringify(
          {
            model: "minimaxai/minimax-m3",
            messages: [
              {
                role: "user",
                content: [
                  { type: "text", text: "Describe what you see in the photo." },
                  {
                    type: "image_url",
                    image_url: {
                      url: "https://assets.ngc.nvidia.com/products/api-catalog/phi-3-5-vision/example1b.jpg",
                    },
                  },
                ],
              },
            ],
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
    id: "ocr-description",
    method: "POST",
    path: "/api/v1/ocr/description",
    category: "Computer Vision",
    summary: "General Visual Description & OCR Extractor",
    description:
      "Analyzes any image with multi-teacher vision failover, extracting comprehensive natural description, visible typography, and safety tags.",
    auth: "API Key (Bearer)",
    defaultBody: JSON.stringify(
      {
        image: "https://images.unsplash.com/photo-1549399542-7e3f8b79c341?w=800",
        instruction: "Analisis isi gambar, deteksi teks OCR, dan evaluasi keamanan konten.",
        model: "auto",
      },
      null,
      2,
    ),
    presets: [
      {
        name: "Discord AutoMod Scan",
        body: JSON.stringify(
          {
            image: "https://images.unsplash.com/photo-1549399542-7e3f8b79c341?w=800",
            instruction: "Evaluasi konten untuk Discord AutoMod: deteksi teks, NSFW, atau toxic.",
            model: "auto",
          },
          null,
          2,
        ),
      },
      {
        name: "Typography & Watermark Extract",
        body: JSON.stringify(
          {
            image: "https://images.unsplash.com/photo-1549399542-7e3f8b79c341?w=800",
            instruction: "Fokuskan pada pembacaan teks spanduk, banner, dan watermark.",
            model: "auto",
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
    summary: "Vehicle License Plate OCR & Local ALPR Fallback",
    description:
      "Multimodal vision endpoint with cloud vision teacher models and automatic failover to local YOLOv11 ONNX + OpenCV engine.",
    auth: "API Key (Bearer)",
    defaultBody: JSON.stringify(
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
    presets: [
      {
        name: "⚡ Local Fast ALPR (Millisecond <100ms)",
        body: JSON.stringify(
          {
            image:
              "https://images.unsplash.com/photo-1549399542-7e3f8b79c341?w=800",
            instruction:
              "Ekstrak plat nomor kendaraan dan tipe kendaraan dengan teliti.",
            model: "fast",
          },
          null,
          2,
        ),
      },
      {
        name: "Auto-Pick Model (Smart Cloud)",
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
        model: "qwen/qwen3.6-27b",
        messages: [
          {
            role: "user",
            content: "Write a high-performance HTTP client snippet in Rust.",
          },
        ],
      },
      null,
      2,
    ),
  },
  {
    id: "admin-dns-query",
    method: "GET",
    path: "/api/v1/admin/dns?domain=integrate.api.nvidia.com",
    category: "Admin",
    summary: "Direct Async UDP DNS Resolver (RFC 1035)",
    description:
      "Executes real-time DNS resolution via Cloudflare 1.1.1.1 or Google 8.8.8.8 bypassing local ISP DNS poisoning.",
    auth: "Master Key (Bearer)",
  },
];

export default function DocsPage() {
  const [selectedEndpoint, setSelectedEndpoint] = useState<ApiEndpoint>(
    ENDPOINTS[0],
  );
  const [apiKey, setApiKey] = useState<string>("");
  const [requestBody, setRequestBody] = useState<string>(
    ENDPOINTS[0].defaultBody || "",
  );
  const [responseBody, setResponseBody] = useState<string>("");
  const [responseStatus, setResponseStatus] = useState<number | null>(null);
  const [responseDuration, setResponseDuration] = useState<number | null>(null);
  const [loading, setLoading] = useState<boolean>(false);
  const [activeLang, setActiveLang] = useState<
    "curl" | "python" | "typescript" | "rust" | "golang"
  >("curl");
  const [copiedTab, setCopiedTab] = useState<string | null>(null);

  useEffect(() => {
    const master = getMasterKey();
    if (master) {
      setApiKey(master);
    }
  }, []);

  function handleSelectEndpoint(ep: ApiEndpoint) {
    setSelectedEndpoint(ep);
    setRequestBody(ep.defaultBody || "");
    setResponseBody("");
    setResponseStatus(null);
    setResponseDuration(null);
  }

  function handlePreset(presetBody: string) {
    setRequestBody(presetBody);
  }

  async function executeRequest() {
    setLoading(true);
    setResponseBody("");
    setResponseStatus(null);
    setResponseDuration(null);

    const startTime = performance.now();
    const headers: Record<string, string> = {
      "Content-Type": "application/json",
    };

    if (apiKey.trim()) {
      headers["Authorization"] = `Bearer ${apiKey.trim()}`;
    }

    try {
      const options: RequestInit = {
        method: selectedEndpoint.method,
        headers,
      };

      if (selectedEndpoint.method !== "GET" && requestBody) {
        options.body = requestBody;
      }

      const res = await fetch(selectedEndpoint.path, options);
      const duration = Math.round(performance.now() - startTime);
      setResponseDuration(duration);
      setResponseStatus(res.status);

      const text = await res.text();
      try {
        const json = JSON.parse(text);
        setResponseBody(JSON.stringify(json, null, 2));
      } catch {
        setResponseBody(text);
      }
    } catch (err: unknown) {
      const duration = Math.round(performance.now() - startTime);
      setResponseDuration(duration);
      setResponseStatus(0);
      setResponseBody(
        JSON.stringify(
          {
            error: "Network / Client Error",
            message: err instanceof Error ? err.message : String(err),
          },
          null,
          2,
        ),
      );
    } finally {
      setLoading(false);
    }
  }

  function generateSnippet(
    lang: "curl" | "python" | "typescript" | "rust" | "golang",
  ): string {
    const token = apiKey ? apiKey.trim() : "YOUR_API_KEY";
    const origin =
      typeof window !== "undefined"
        ? window.location.origin
        : "http://127.0.0.1:5790";
    const fullUrl = `${origin}${selectedEndpoint.path}`;

    if (lang === "curl") {
      if (selectedEndpoint.method === "GET") {
        return `curl -X GET "${fullUrl}" \\\n  -H "Authorization: Bearer ${token}"`;
      }
      return `curl -X POST "${fullUrl}" \\\n  -H "Content-Type: application/json" \\\n  -H "Authorization: Bearer ${token}" \\\n  -d '${requestBody.replace(/'/g, "\\'")}'`;
    }

    if (lang === "python") {
      if (selectedEndpoint.method === "GET") {
        return `import requests\n\nurl = "${fullUrl}"\nheaders = {"Authorization": "Bearer ${token}"}\n\nresponse = requests.get(url, headers=headers)\nprint(response.json())`;
      }
      return `import requests\n\nurl = "${fullUrl}"\nheaders = {\n    "Content-Type": "application/json",\n    "Authorization": "Bearer ${token}"\n}\npayload = ${requestBody || "{}"}\n\nresponse = requests.post(url, json=payload, headers=headers)\nprint(response.json())`;
    }

    if (lang === "typescript") {
      if (selectedEndpoint.method === "GET") {
        return `const response = await fetch("${fullUrl}", {\n  method: "GET",\n  headers: {\n    "Authorization": "Bearer ${token}"\n  }\n});\nconst data = await response.json();\nconsole.log(data);`;
      }
      return `const response = await fetch("${fullUrl}", {\n  method: "POST",\n  headers: {\n    "Content-Type": "application/json",\n    "Authorization": "Bearer ${token}"\n  },\n  body: JSON.stringify(${requestBody || "{}"})\n});\nconst data = await response.json();\nconsole.log(data);`;
    }

    if (lang === "rust") {
      return `use reqwest::Client;\n\n#[tokio::main]\nasync fn main() -> Result<(), Box<dyn std::error::Error>> {\n    let client = Client::new();\n    let resp = client.post("${fullUrl}")\n        .header("Authorization", "Bearer ${token}")\n        .json(&serde_json::json!(${requestBody || "{}"}))\n        .send()\n        .await?;\n    \n    println!("Status: {}", resp.status());\n    println!("Body: {}", resp.text().await?);\n    Ok(())\n}`;
    }

    if (lang === "golang") {
      return `package main\n\nimport (\n    "bytes"\n    "fmt"\n    "io"\n    "net/http"\n)\n\nfunc main() {\n    url := "${fullUrl}"\n    payload := []byte(\`${requestBody || "{}"}\`)\n    req, _ := http.NewRequest("POST", url, bytes.NewBuffer(payload))\n    req.Header.Set("Content-Type", "application/json")\n    req.Header.Set("Authorization", "Bearer ${token}")\n\n    client := &http.Client{}\n    resp, err := client.Do(req)\n    if err != nil {\n        panic(err)\n    }\n    defer resp.Body.Close()\n    body, _ := io.ReadAll(resp.Body)\n    fmt.Println(string(body))\n}`;
    }

    return "";
  }

  function copyCode(text: string, tabName: string) {
    if (!navigator.clipboard) return;
    navigator.clipboard.writeText(text);
    setCopiedTab(tabName);
    setTimeout(() => setCopiedTab(null), 2000);
  }

  function downloadOpenApiSpec() {
    const spec = {
      openapi: "3.0.3",
      info: {
        title: "Router AI Intelligent Multimodal Gateway",
        version: "1.0.0",
        description:
          "High-performance AI Gateway proxying OpenAI-compatible Chat completions, NVIDIA NIM Vision models, and ALPR License Plate Recognition with local failover.",
      },
      servers: [{ url: "/" }],
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
        "/api/v1/ocr/description": {
          post: {
            summary: "Visual Description & OCR Analysis",
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
            responses: { "200": { description: "Structured Description & OCR" } },
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
        {/* Page Title & Export */}
        <div className="flex flex-wrap items-center justify-between gap-4">
          <div>
            <h1 className="text-xl font-bold tracking-tight text-slate-900">
              API Documentation &amp; Swagger Console
            </h1>
            <p className="mt-1 text-xs text-slate-500">
              Interactive OpenAPI reference, live testing console, and multi-language client snippets.
            </p>
          </div>
          <button
            onClick={downloadOpenApiSpec}
            className="inline-flex items-center gap-2 rounded-lg border border-slate-200 bg-white px-3.5 py-2 text-xs font-semibold text-slate-700 shadow-xs transition-all hover:bg-slate-50 hover:text-slate-900"
          >
            <LuDownload className="h-4 w-4 text-slate-500" />
            <span>Download OpenAPI 3.0</span>
          </button>
        </div>

        {/* Global Key Input */}
        <Card>
          <div className="flex flex-wrap items-center gap-4">
            <div className="flex items-center gap-2">
              <LuKey className="h-4 w-4 text-slate-400" />
              <span className="text-xs font-semibold uppercase tracking-wider text-slate-500">
                Authorization Token
              </span>
              <Badge tone={apiKey ? "green" : "amber"}>
                {apiKey ? "Configured" : "Unset"}
              </Badge>
            </div>
            <div className="min-w-[280px] flex-1">
              <input
                type="text"
                value={apiKey}
                onChange={(e) => setApiKey(e.target.value)}
                placeholder="sk-router-... or master key"
                className="w-full rounded-lg border border-slate-300 bg-slate-50/50 px-3 py-1.5 font-mono text-xs text-slate-900 outline-none transition-all placeholder:text-slate-400 focus:border-indigo-500 focus:bg-white focus:ring-2 focus:ring-indigo-500/20"
              />
            </div>
            <p className="text-xs text-slate-400">
              Injected automatically into Bearer header for live request execution.
            </p>
          </div>
        </Card>

        {/* Main Grid: Endpoints List + Playground */}
        <div className="grid grid-cols-1 gap-6 lg:grid-cols-12">
          {/* Endpoint Sidebar */}
          <div className="flex flex-col gap-2 lg:col-span-4">
            <div className="flex items-center gap-2 px-1">
              <LuFileText className="h-3.5 w-3.5 text-slate-400" />
              <h2 className="text-xs font-semibold uppercase tracking-wider text-slate-400">
                Endpoints
              </h2>
            </div>
            <div className="flex max-h-[720px] flex-col gap-2 overflow-y-auto pr-1">
              {ENDPOINTS.map((ep) => {
                const selected = selectedEndpoint.id === ep.id;
                const methodBadgeTone =
                  ep.method === "POST"
                    ? "green"
                    : ep.method === "GET"
                      ? "blue"
                      : "zinc";

                return (
                  <button
                    key={ep.id}
                    onClick={() => handleSelectEndpoint(ep)}
                    className={`flex flex-col gap-1 rounded-xl border p-3.5 text-left transition-all ${
                      selected
                        ? "border-indigo-500 bg-indigo-50/70 shadow-xs ring-1 ring-indigo-500/20"
                        : "border-slate-200/90 bg-white shadow-xs hover:border-slate-300 hover:bg-slate-50/60"
                    }`}
                  >
                    <div className="flex items-center gap-2">
                      <Badge tone={methodBadgeTone}>
                        {ep.method}
                      </Badge>
                      <span className="font-mono text-xs font-semibold text-slate-800">
                        {ep.path}
                      </span>
                    </div>
                    <p className="text-xs text-slate-500 line-clamp-1">
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
              <div className="flex flex-wrap items-start justify-between gap-3 border-b border-slate-100 pb-4">
                <div className="flex flex-col gap-1.5">
                  <div className="flex items-center gap-2">
                    <Badge
                      tone={
                        selectedEndpoint.method === "POST" ? "green" : "blue"
                      }
                    >
                      {selectedEndpoint.method}
                    </Badge>
                    <span className="font-mono text-sm font-bold text-slate-900">
                      {selectedEndpoint.path}
                    </span>
                  </div>
                  <p className="text-xs text-slate-600">
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
                    <div className="flex items-center gap-1 text-xs font-semibold text-slate-500">
                      <LuZap className="h-3.5 w-3.5 text-amber-500" />
                      <span>Presets:</span>
                    </div>
                    {selectedEndpoint.presets.map((preset) => (
                      <button
                        key={preset.name}
                        onClick={() => handlePreset(preset.body)}
                        className="rounded-lg border border-slate-200 bg-slate-50 px-2.5 py-1 text-xs font-medium text-slate-700 shadow-xs transition-colors hover:border-indigo-400 hover:bg-white hover:text-indigo-700"
                      >
                        {preset.name}
                      </button>
                    ))}
                  </div>
                )}

              {/* Request Body Editor */}
              {selectedEndpoint.method !== "GET" && (
                <div className="mt-4 flex flex-col gap-2">
                  <label className="text-xs font-semibold uppercase tracking-wider text-slate-500">
                    Request Body (JSON)
                  </label>
                  <textarea
                    rows={8}
                    value={requestBody}
                    onChange={(e) => setRequestBody(e.target.value)}
                    className="w-full rounded-xl border border-slate-300 bg-slate-50/50 p-3.5 font-mono text-xs text-slate-900 outline-none transition-all placeholder:text-slate-400 focus:border-indigo-500 focus:bg-white focus:ring-2 focus:ring-indigo-500/20"
                  />
                </div>
              )}

              {/* Action Bar */}
              <div className="mt-4 flex flex-wrap items-center justify-between gap-3 pt-2">
                <button
                  onClick={executeRequest}
                  disabled={loading}
                  className="inline-flex items-center gap-2 rounded-xl bg-indigo-600 px-5 py-2.5 text-xs font-semibold text-white shadow-xs transition-all hover:bg-indigo-700 hover:shadow-sm disabled:opacity-50"
                >
                  {loading ? (
                    <>
                      <div className="h-4 w-4 animate-spin rounded-full border-2 border-white border-t-transparent" />
                      <span>Sending payload...</span>
                    </>
                  ) : (
                    <>
                      <LuSend className="h-3.5 w-3.5" />
                      <span>Send Request</span>
                    </>
                  )}
                </button>
                {responseDuration !== null && (
                  <span className="font-mono text-xs text-slate-500">
                    Latency:{" "}
                    <strong className="text-slate-900">
                      {responseDuration} ms
                    </strong>
                  </span>
                )}
              </div>

              {/* Response Section */}
              {responseStatus !== null && (
                <div className="mt-6 flex flex-col gap-2.5 border-t border-slate-100 pt-4">
                  <div className="flex items-center justify-between">
                    <span className="text-xs font-bold uppercase tracking-wider text-slate-500">
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
                  <pre className="max-h-96 overflow-auto rounded-xl border border-slate-200 bg-slate-900 p-4 font-mono text-xs leading-relaxed text-emerald-400 shadow-inner">
                    {responseBody}
                  </pre>
                </div>
              )}
            </Card>

            {/* Code Snippets Generator */}
            <Card title="Client Code Snippets">
              <div className="flex flex-col gap-3">
                <div className="flex items-center justify-between border-b border-slate-100 pb-2.5">
                  <div className="flex items-center gap-1.5">
                    <LuCode className="h-4 w-4 text-indigo-600 mr-1" />
                    {(
                      [
                        "curl",
                        "python",
                        "typescript",
                        "rust",
                        "golang",
                      ] as const
                    ).map((lang) => (
                      <button
                        key={lang}
                        onClick={() => setActiveLang(lang)}
                        className={`rounded-lg px-3 py-1 text-xs font-semibold transition-all ${
                          activeLang === lang
                            ? "bg-indigo-50 text-indigo-700 shadow-xs ring-1 ring-indigo-500/20"
                            : "text-slate-500 hover:bg-slate-50 hover:text-slate-900"
                        }`}
                      >
                        {lang.toUpperCase()}
                      </button>
                    ))}
                  </div>
                  <button
                    onClick={() =>
                      copyCode(generateSnippet(activeLang), activeLang)
                    }
                    className="inline-flex items-center gap-1 rounded-md border border-slate-200 bg-white px-2.5 py-1 text-xs font-semibold text-slate-700 shadow-xs transition-colors hover:bg-slate-50"
                  >
                    {copiedTab === activeLang ? (
                      <>
                        <LuCheck className="h-3.5 w-3.5 text-emerald-600" />
                        <span>Copied</span>
                      </>
                    ) : (
                      <>
                        <LuCopy className="h-3.5 w-3.5" />
                        <span>Copy Code</span>
                      </>
                    )}
                  </button>
                </div>
                <pre className="overflow-x-auto rounded-xl border border-slate-200 bg-slate-900 p-4 font-mono text-xs leading-relaxed text-slate-100 shadow-inner">
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
