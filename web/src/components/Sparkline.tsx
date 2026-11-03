import { useState } from "react";

// One metric over time: a single 2px line in text ink, a marker on the latest value,
// and a crosshair + tooltip on hover. Days without a value break the line.
export interface Point {
  date: string;
  value: number | null;
}

const W = 168;
const H = 44;
const PAD = 5;

export default function Sparkline({ label, points, unit = "" }: { label: string; points: Point[]; unit?: string }) {
  const [hover, setHover] = useState<number | null>(null);
  const vals = points.map((p) => p.value).filter((v): v is number => v !== null);
  const latest = [...points].reverse().find((p) => p.value !== null) ?? null;
  const min = Math.min(...vals);
  const max = Math.max(...vals);
  const span = max - min || 1;
  const x = (i: number) => PAD + (i * (W - PAD * 2)) / Math.max(points.length - 1, 1);
  const y = (v: number) => H - PAD - ((v - min) / span) * (H - PAD * 2);

  let d = "";
  points.forEach((p, i) => {
    if (p.value === null) return;
    const prev = points[i - 1];
    d += `${i === 0 || !prev || prev.value === null ? "M" : "L"}${x(i).toFixed(1)},${y(p.value).toFixed(1)} `;
  });
  const fmt = (v: number) => `${Number.isInteger(v) ? v.toLocaleString("en-US") : v.toFixed(1)}${unit}`;
  const shown = hover !== null ? points[hover] : latest;
  const lastIndex = latest ? points.indexOf(latest) : -1;

  return (
    <figure className="spark">
      <figcaption>
        <span className="spark-label">{label}</span>
        <span className="spark-value">{shown?.value != null ? fmt(shown.value) : "–"}</span>
        <span className="spark-date">{hover !== null && shown ? shown.date.slice(5) : "latest"}</span>
      </figcaption>
      {vals.length > 0 ? (
        <svg
          viewBox={`0 0 ${W} ${H}`}
          role="img"
          aria-label={`${label}: ${points.map((p) => `${p.date} ${p.value ?? "no data"}`).join(", ")}`}
          onMouseLeave={() => setHover(null)}
          onMouseMove={(e) => {
            const box = e.currentTarget.getBoundingClientRect();
            const px = ((e.clientX - box.left) / box.width) * W;
            const i = Math.round((px - PAD) / ((W - PAD * 2) / Math.max(points.length - 1, 1)));
            setHover(Math.max(0, Math.min(points.length - 1, i)));
          }}
        >
          <path className="spark-line" d={d} />
          {hover !== null && <line className="spark-cross" x1={x(hover)} x2={x(hover)} y1={0} y2={H} />}
          {hover !== null && points[hover]?.value != null && <circle className="spark-dot" cx={x(hover)} cy={y(points[hover]!.value!)} r={4} />}
          {hover === null && lastIndex >= 0 && latest?.value != null && <circle className="spark-dot" cx={x(lastIndex)} cy={y(latest.value)} r={4} />}
        </svg>
      ) : (
        <p className="spark-empty">No data yet</p>
      )}
    </figure>
  );
}
