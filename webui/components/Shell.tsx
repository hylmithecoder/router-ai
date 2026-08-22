import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import { clearMasterKey } from "@/lib/api";

const NAV = [
  { href: "/dashboard", label: "Overview", icon: "📊" },
  { href: "/usage", label: "Usage log", icon: "📋" },
  { href: "/keys", label: "API keys", icon: "🔑" },
  { href: "/providers", label: "Providers", icon: "⚡" },
  { href: "/docs", label: "API Docs (Swagger)", icon: "📖" },
];

export default function Shell({ children }: { children: React.ReactNode }) {
  const pathname = usePathname();
  const router = useRouter();

  function logout() {
    clearMasterKey();
    router.push("/login");
  }

  return (
    <div className="flex min-h-screen bg-slate-50 text-slate-900">
      {/* Sidebar */}
      <aside className="flex w-64 flex-col border-r border-slate-200/90 bg-white p-5 shadow-[1px_0_4px_rgba(0,0,0,0.02)]">
        {/* Brand */}
        <div className="mb-8 flex items-center gap-3 px-1">
          <div className="flex h-9 w-9 items-center justify-center rounded-xl bg-gradient-to-tr from-indigo-600 to-indigo-500 font-mono text-sm font-bold text-white shadow-sm ring-1 ring-indigo-700/20">
            AI
          </div>
          <div>
            <p className="text-sm font-bold tracking-tight text-slate-900">AI Router</p>
            <p className="text-[11px] font-medium text-slate-400">Astryx Dashboard</p>
          </div>
        </div>

        {/* Navigation */}
        <nav className="flex flex-col gap-1.5">
          <p className="px-3 pb-1 text-[11px] font-semibold uppercase tracking-wider text-slate-400">
            Navigation
          </p>
          {NAV.map((item) => {
            const active = pathname === item.href;
            return (
              <Link
                key={item.href}
                href={item.href}
                className={`flex items-center gap-2.5 rounded-lg px-3 py-2 text-sm font-medium transition-all ${
                  active
                    ? "bg-indigo-50/80 font-semibold text-indigo-700 shadow-xs ring-1 ring-indigo-500/15"
                    : "text-slate-600 hover:bg-slate-50 hover:text-slate-900"
                }`}
              >
                <span className="text-base leading-none">{item.icon}</span>
                <span>{item.label}</span>
              </Link>
            );
          })}
        </nav>

        {/* Footer info & Logout */}
        <div className="mt-auto border-t border-slate-100 pt-4">
          <div className="mb-3 flex items-center gap-2 px-3 py-2">
            <div className="h-2 w-2 rounded-full bg-emerald-500 ring-4 ring-emerald-100" />
            <span className="text-xs font-medium text-slate-500">Router Online</span>
          </div>
          <button
            onClick={logout}
            className="flex w-full items-center gap-2 rounded-lg px-3 py-2 text-left text-sm font-medium text-slate-500 transition-colors hover:bg-rose-50 hover:text-rose-600"
          >
            <span>🚪</span>
            <span>Sign out</span>
          </button>
        </div>
      </aside>

      {/* Main Content */}
      <main className="flex-1 overflow-x-auto p-8 lg:p-10">{children}</main>
    </div>
  );
}