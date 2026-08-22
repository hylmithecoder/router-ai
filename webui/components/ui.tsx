import { LuCircleAlert } from "react-icons/lu";

export function Card({
  title,
  children,
  className = "",
}: {
  title?: string;
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <div className={`rounded-xl border border-slate-200/90 bg-white shadow-xs ${className}`}>
      {title && (
        <div className="border-b border-slate-100 px-5 py-3.5">
          <h2 className="text-sm font-semibold tracking-tight text-slate-800">{title}</h2>
        </div>
      )}
      <div className="p-5">{children}</div>
    </div>
  );
}

export function Stat({
  label,
  value,
  sub,
}: {
  label: string;
  value: string;
  sub?: string;
}) {
  return (
    <div className="rounded-xl border border-slate-200/90 bg-white p-5 shadow-xs transition-shadow hover:shadow-sm">
      <p className="text-xs font-semibold uppercase tracking-wider text-slate-400">{label}</p>
      <p className="mt-1.5 font-mono text-2xl font-bold tracking-tight text-slate-900">{value}</p>
      {sub && <p className="mt-1.5 text-xs text-slate-500">{sub}</p>}
    </div>
  );
}

export function Badge({
  tone,
  children,
}: {
  tone: "green" | "red" | "amber" | "zinc" | "blue";
  children: React.ReactNode;
}) {
  const styles = {
    green: "bg-emerald-50 text-emerald-700 border-emerald-200",
    red: "bg-rose-50 text-rose-700 border-rose-200",
    amber: "bg-amber-50 text-amber-800 border-amber-200",
    zinc: "bg-slate-100 text-slate-700 border-slate-200",
    blue: "bg-indigo-50 text-indigo-700 border-indigo-200",
  }[tone];
  return (
    <span
      className={`inline-flex items-center rounded-md border px-2.5 py-0.5 text-xs font-medium ${styles}`}
    >
      {children}
    </span>
  );
}

export function Spinner() {
  return (
    <div className="flex items-center justify-center py-16 text-sm text-slate-400">
      <div className="h-6 w-6 animate-spin rounded-full border-2 border-slate-200 border-t-indigo-600" />
    </div>
  );
}

export function ErrorBox({ message }: { message: string }) {
  return (
    <div className="rounded-xl border border-rose-200 bg-rose-50/80 p-4 text-sm text-rose-700 shadow-xs">
      <div className="flex items-center gap-2.5">
        <LuCircleAlert className="h-4 w-4 shrink-0 text-rose-600" />
        <span className="font-medium">{message}</span>
      </div>
    </div>
  );
}

export function QuotaGauge({
  used,
  quota,
}: {
  used: number;
  quota: number;
}) {
  const pct = quota > 0 ? Math.min(100, Math.round((used / quota) * 100)) : 0;
  const tone =
    pct >= 90 ? "bg-rose-500" : pct >= 70 ? "bg-amber-500" : "bg-indigo-600";
  return (
    <div>
      <div className="flex items-baseline justify-between">
        <p className="text-xs font-semibold uppercase tracking-wider text-slate-400">
          Daily quota
        </p>
        <p className="font-mono text-sm font-semibold text-slate-800">
          {new Intl.NumberFormat("en-US").format(used)}{" "}
          <span className="font-normal text-slate-400">/ {new Intl.NumberFormat("en-US").format(quota)} tokens</span>
        </p>
      </div>
      <div className="mt-3 h-2.5 w-full overflow-hidden rounded-full bg-slate-100 ring-1 ring-slate-200/80">
        <div className={`h-full rounded-full transition-all duration-500 ${tone}`} style={{ width: `${pct}%` }} />
      </div>
      <p className="mt-2 text-xs font-medium text-slate-500">
        {quota > 0 ? `${pct}% used today` : "no quota configured (unlimited)"}
      </p>
    </div>
  );
}

export function BarChart({
  data,
}: {
  data: { day: string; tokens: number }[];
}) {
  const max = Math.max(1, ...data.map((d) => d.tokens));
  return (
    <div className="flex h-40 items-end gap-2 pt-2">
      {data.map((d) => (
        <div key={d.day} className="group flex flex-1 flex-col items-center gap-1.5">
          <div className="relative flex w-full flex-1 items-end">
            <div
              className="w-full rounded-t-md bg-indigo-500/80 transition-all group-hover:bg-indigo-600 shadow-xs"
              style={{ height: `${Math.max(4, (d.tokens / max) * 100)}%` }}
              title={`${d.day}: ${new Intl.NumberFormat("en-US").format(d.tokens)} tokens`}
            />
          </div>
          <span className="font-mono text-[11px] font-medium text-slate-500">{d.day.slice(5)}</span>
        </div>
      ))}
    </div>
  );
}