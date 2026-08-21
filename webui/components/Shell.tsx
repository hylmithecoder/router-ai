import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import { clearMasterKey } from "@/lib/api";

const NAV = [
  { href: "/dashboard", label: "Overview" },
  { href: "/usage", label: "Usage log" },
  { href: "/keys", label: "API keys" },
  { href: "/providers", label: "Providers" },
];

export default function Shell({ children }: { children: React.ReactNode }) {
  const pathname = usePathname();
  const router = useRouter();

  function logout() {
    clearMasterKey();
    router.push("/login");
  }

  return (
    <div className="flex min-h-screen bg-zinc-950 text-zinc-100">
      <aside className="flex w-56 flex-col border-r border-zinc-800 p-4">
        <div className="mb-8 flex items-center gap-2 px-2">
          <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-emerald-500 font-mono text-sm font-bold text-zinc-950">
            AI
          </div>
          <div>
            <p className="text-sm font-semibold leading-tight">AI Router</p>
            <p className="text-xs text-zinc-500">dashboard</p>
          </div>
        </div>

        <nav className="flex flex-col gap-1">
          {NAV.map((item) => {
            const active = pathname === item.href;
            return (
              <Link
                key={item.href}
                href={item.href}
                className={`rounded-md px-3 py-2 text-sm transition-colors ${active
                    ? "bg-zinc-800 font-medium text-white"
                    : "text-zinc-400 hover:bg-zinc-900 hover:text-zinc-200"
                  }`}
              >
                {item.label}
              </Link>
            );
          })}
        </nav>

        <button
          onClick={logout}
          className="mt-auto rounded-md px-3 py-2 text-left text-sm text-zinc-500 transition-colors hover:bg-zinc-900 hover:text-zinc-200"
        >
          Log out
        </button>
      </aside>

      <main className="flex-1 overflow-x-auto p-8">{children}</main>
    </div>
  );
}