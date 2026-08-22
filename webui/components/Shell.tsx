"use client";

import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import { clearMasterKey } from "@/lib/api";
import {
  LuLayoutDashboard,
  LuActivity,
  LuKeyRound,
  LuServer,
  LuBookOpen,
  LuLogOut,
  LuShieldCheck,
  LuCpu,
} from "react-icons/lu";
import { RiSparklingFill } from "react-icons/ri";

const NAV = [
  { href: "/dashboard", label: "Overview", icon: LuLayoutDashboard },
  { href: "/usage", label: "Usage log", icon: LuActivity },
  { href: "/keys", label: "API keys", icon: LuKeyRound },
  { href: "/providers", label: "Providers", icon: LuServer },
  { href: "/docs", label: "API Docs (Swagger)", icon: LuBookOpen },
];

export default function Shell({ children }: { children: React.ReactNode }) {
  const pathname = usePathname();
  const router = useRouter();

  function logout() {
    clearMasterKey();
    router.push("/login");
  }

  const currentNav = NAV.find((item) => item.href === pathname) || {
    label: "Dashboard",
    icon: LuLayoutDashboard,
  };
  const CurrentIcon = currentNav.icon;

  return (
    <div className="flex min-h-screen bg-slate-50 text-slate-900">
      {/* Left Sidebar */}
      <aside className="sticky top-0 flex h-screen w-64 flex-col border-r border-slate-200/90 bg-white p-5 shadow-[1px_0_4px_rgba(0,0,0,0.02)]">
        {/* Brand Header */}
        <div className="mb-8 flex items-center gap-3 px-1">
          <div className="flex h-9 w-9 items-center justify-center rounded-xl bg-gradient-to-tr from-indigo-600 to-indigo-500 text-white shadow-xs ring-1 ring-indigo-700/20">
            <LuCpu className="h-5 w-5" />
          </div>
          <div>
            <div className="flex items-center gap-1.5">
              <p className="text-sm font-bold tracking-tight text-slate-900">AI Router</p>
              <span className="flex items-center text-[10px] font-semibold text-indigo-600 bg-indigo-50 px-1.5 py-0.2 rounded-full border border-indigo-200/60">
                <RiSparklingFill className="mr-0.5 h-2.5 w-2.5" /> Core
              </span>
            </div>
            <p className="text-[11px] font-medium text-slate-400">Astryx Gateway</p>
          </div>
        </div>

        {/* Sidebar Nav Items */}
        <nav className="flex flex-col gap-1.5">
          <p className="px-3 pb-1 text-[11px] font-semibold uppercase tracking-wider text-slate-400">
            Menu
          </p>
          {NAV.map((item) => {
            const active = pathname === item.href;
            const Icon = item.icon;
            return (
              <Link
                key={item.href}
                href={item.href}
                className={`flex items-center gap-3 rounded-lg px-3 py-2.5 text-sm font-medium transition-all ${
                  active
                    ? "bg-indigo-50/80 font-semibold text-indigo-700 shadow-xs ring-1 ring-indigo-500/15"
                    : "text-slate-600 hover:bg-slate-50 hover:text-slate-900"
                }`}
              >
                <Icon
                  className={`h-4 w-4 transition-colors ${
                    active ? "text-indigo-600" : "text-slate-400 group-hover:text-slate-600"
                  }`}
                />
                <span>{item.label}</span>
              </Link>
            );
          })}
        </nav>

        {/* Sidebar Footer: Service Status */}
        <div className="mt-auto border-t border-slate-100 pt-4">
          <div className="flex items-center justify-between rounded-lg border border-slate-100 bg-slate-50/80 px-3 py-2">
            <div className="flex items-center gap-2">
              <div className="relative flex h-2 w-2">
                <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-emerald-400 opacity-75" />
                <span className="relative inline-flex h-2 w-2 rounded-full bg-emerald-500" />
              </div>
              <span className="text-xs font-semibold text-slate-700">Router Online</span>
            </div>
            <span className="font-mono text-[10px] font-medium text-slate-400">v0.1.0</span>
          </div>
        </div>
      </aside>

      {/* Main Content Area with Top Navbar */}
      <div className="flex flex-1 flex-col min-w-0">
        {/* Top Navbar */}
        <header className="sticky top-0 z-30 flex h-16 w-full items-center justify-between border-b border-slate-200/90 bg-white/90 px-8 backdrop-blur-md shadow-xs">
          {/* Left: Breadcrumbs / Active Page Title */}
          <div className="flex items-center gap-2.5">
            <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-slate-100 text-slate-600">
              <CurrentIcon className="h-4 w-4 text-indigo-600" />
            </div>
            <div className="flex items-center gap-2 text-sm">
              <span className="text-slate-400">Router</span>
              <span className="text-slate-300">/</span>
              <span className="font-semibold text-slate-800">{currentNav.label}</span>
            </div>
          </div>

          {/* Right: Security Status & Logout Button */}
          <div className="flex items-center gap-3">
            <div className="hidden sm:flex items-center gap-1.5 rounded-md border border-slate-200 bg-slate-50 px-2.5 py-1 text-xs font-medium text-slate-600">
              <LuShieldCheck className="h-3.5 w-3.5 text-emerald-600" />
              <span>Master Key Auth</span>
            </div>

            {/* Logout Action in Navbar */}
            <button
              onClick={logout}
              className="inline-flex items-center gap-2 rounded-lg border border-slate-200 bg-white px-3.5 py-1.5 text-xs font-semibold text-slate-700 shadow-xs transition-all hover:border-rose-200 hover:bg-rose-50 hover:text-rose-600 active:scale-98"
              title="Sign out of admin dashboard"
            >
              <LuLogOut className="h-3.5 w-3.5" />
              <span>Log out</span>
            </button>
          </div>
        </header>

        {/* Page Body */}
        <main className="flex-1 overflow-x-auto p-8 lg:p-10">{children}</main>
      </div>
    </div>
  );
}