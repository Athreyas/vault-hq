// Tiny hand-drawn SVG marks for widgets: a sparkline, a meter and day bars. Colour comes
// from CSS (currentColor and tokens), so they follow the theme.

const W = 120;
const H = 28;
const PAD = 2;

/** A line over `values`, oldest first, scaled to [0, max] (or to the data if no max). */
export function Spark({ values, max, label, unit = "" }: { values: number[]; max?: number; label: string; unit?: string }) {
  if (values.length === 0) return <span className="w-spark is-empty">–</span>;
  const top = max ?? Math.max(...values, 1);
  const lo = max === undefined ? Math.min(...values) : 0;
  const span = top - lo || 1;
  const x = (i: number) => PAD + (i * (W - PAD * 2)) / Math.max(values.length - 1, 1);
  const y = (v: number) => H - PAD - ((Math.min(v, top) - lo) / span) * (H - PAD * 2);
  const d = values.map((v, i) => `${i ? "L" : "M"}${x(i).toFixed(1)},${y(v).toFixed(1)}`).join(" ");
  const last = values[values.length - 1]!;
  return (
    <svg className="w-spark" viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" role="img" aria-label={`${label}: latest ${Math.round(last)}${unit}`}>
      <path className="w-spark-area" d={`${d} L${x(values.length - 1).toFixed(1)},${H - PAD} L${PAD},${H - PAD} Z`} />
      <path className="w-spark-line" d={d} vectorEffect="non-scaling-stroke" />
    </svg>
  );
}

/** A horizontal bar, 0–100. `tone` tints it when something is worth a glance. */
export function Meter({ pct, label, tone }: { pct: number; label: string; tone?: "warn" | "bad" }) {
  const v = Math.max(0, Math.min(100, pct));
  return (
    <span className={`w-meter${tone ? ` is-${tone}` : ""}`} role="meter" aria-valuenow={Math.round(v)} aria-valuemin={0} aria-valuemax={100} aria-label={label}>
      <span style={{ width: `${v}%` }} />
    </span>
  );
}

/** One thin bar per value (done per day). */
export function Bars({ values, label }: { values: number[]; label: string }) {
  const top = Math.max(...values, 1);
  const bw = (W - PAD * 2) / Math.max(values.length, 1);
  return (
    <svg className="w-bars" viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" role="img" aria-label={`${label}: ${values.reduce((a, b) => a + b, 0)} in ${values.length} days`}>
      {values.map((v, i) =>
        v > 0 ? <rect key={i} x={PAD + i * bw + bw * 0.15} width={bw * 0.7} y={H - PAD - (v / top) * (H - PAD * 2)} height={(v / top) * (H - PAD * 2)} /> : null,
      )}
      <line x1={PAD} x2={W - PAD} y1={H - PAD + 0.5} y2={H - PAD + 0.5} />
    </svg>
  );
}
