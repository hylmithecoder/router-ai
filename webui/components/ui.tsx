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
    <div className={`rounded-xl border border-zinc-800 bg-zinc-900/60 ${className}`}>
      {title && (
        <div className="border-b border-zinc-800 px-4 py-3">
          <h2 className="text-sm font-semibold text-zinc-200">{title}</h2>
        </div>
      )}
      <div className="p-4">{children}</div>
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
    <div className="rounded-xl border border-zinc-800 bg-zinc-900/60 p-4">
      <p className="text-xs uppercase tracking-wider text-zinc-500">{label}</p>
      <p className="mt-1 font-mono text-2xl font-semibold text-zinc-100">{value}</p>
      {sub && <p className="mt-1 text-xs text-zinc-500">{sub}</p>}
    </div>
  );
}

export function Badge({
  tone,
  children,
}: {
  tone: "green" | "red" | "amber" | "zinc";
  children: React.ReactNode;
}) {
  const styles = {
    green: "bg-emerald-500/15 text-emerald-400 border-emerald-500/30",
    red: "bg-red-500/15 text-red-400 border-red-500/30",
    amber: "bg-amber-500/15 text-amber-400 border-amber-500/30",
    zinc: "bg-zinc-500/15 text-zinc-400 border-zinc-500/30",
  }[tone];
  return (
    <span
      className={`inline-flex items-center rounded-full border px-2 py-0.5 text-xs font-medium ${styles}`}
    >
      {children}
    </span>
  );
}

export function Spinner() {
  return (
    <div className="flex items-center justify-center py-16 text-sm text-zinc-500">
      <div className="h-5 w-5 animate-spin rounded-full border-2 border-zinc-700 border-t-emerald-500" />
    </div>
  );
}

export function ErrorBox({ message }: { message: string }) {
  return (
    <div className="rounded-xl border border-red-500/30 bg-red-500/10 p-4 text-sm text-red-400">
      {message}
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
    pct >= 90 ? "bg-red-500" : pct >= 70 ? "bg-amber-500" : "bg-emerald-500";
  return (
    <div>
      <div className="flex items-baseline justify-between">
        <p className="text-xs uppercase tracking-wider text-zinc-500">
          Daily quota
        </p>
        <p className="font-mono text-sm text-zinc-300">
          {new Intl.NumberFormat("en-US").format(used)}{" "}
          <span className="text-zinc-600">/ {new Intl.NumberFormat("en-US").format(quota)} tokens</span>
        </p>
      </div>
      <div className="mt-3 h-2.5 w-full overflow-hidden rounded-full bg-zinc-800">
        <div className={`h-full rounded-full ${tone}`} style={{ width: `${pct}%` }} />
      </div>
      <p className="mt-2 text-xs text-zinc-500">
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
    <div className="flex h-40 items-end gap-2">
      {data.map((d) => (
        <div key={d.day} className="group flex flex-1 flex-col items-center gap-1">
          <div className="relative flex w-full flex-1 items-end">
            <div
              className="w-full rounded-t-md bg-emerald-500/80 transition-colors group-hover:bg-emerald-400"
              style={{ height: `${Math.max(2, (d.tokens / max) * 100)}%` }}
              title={`${d.day}: ${new Intl.NumberFormat("en-US").format(d.tokens)} tokens`}
            />
          </div>
          <span className="text-[10px] text-zinc-600">{d.day.slice(5)}</span>
        </div>
      ))}
    </div>
  );
}