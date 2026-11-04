import { useState } from "react";
import type { LabelCount, WeeklyActivity } from "../lib/types";
import { shortDate } from "../lib/format";

const SERIES = [
  { key: "discovered", label: "Discovered", color: "var(--series-1)" },
  { key: "submitted", label: "Submitted", color: "var(--series-2)" },
  { key: "interviews", label: "Interviews", color: "var(--series-3)" },
] as const;

type Tip = { x: number; y: number; html: string[] } | null;

/** Grouped weekly bars (3 series, one shared y-axis) with per-week hover tooltip and table fallback. */
export function ActivityChart({ data }: { data: WeeklyActivity[] }) {
  const [tip, setTip] = useState<Tip>(null);
  const [showTable, setShowTable] = useState(false);
  const W = 640, H = 220, padL = 32, padB = 26, padT = 10;
  const max = Math.max(1, ...data.flatMap((d) => [d.discovered, d.submitted, d.interviews]));
  const nice = max <= 4 ? 4 : Math.ceil(max / 4) * 4;
  const plotW = W - padL - 8;
  const plotH = H - padB - padT;
  const groupW = plotW / Math.max(1, data.length);
  const barW = Math.max(3, Math.min(12, (groupW - 8) / 3 - 2));
  const y = (v: number) => padT + plotH - (v / nice) * plotH;
  const ticks = [0, nice / 4, nice / 2, (3 * nice) / 4, nice];

  return (
    <div className="chart">
      <div className="legend">
        {SERIES.map((s) => (
          <span key={s.key}><i style={{ background: s.color }} />{s.label}</span>
        ))}
        <button className="link small" onClick={() => setShowTable((v) => !v)}>{showTable ? "Show chart" : "Show table"}</button>
      </div>
      {showTable ? (
        <table className="table compact">
          <thead><tr><th>Week of</th>{SERIES.map((s) => <th key={s.key} className="num">{s.label}</th>)}</tr></thead>
          <tbody>
            {data.map((d) => (
              <tr key={d.weekStart}><td>{shortDate(d.weekStart)}</td><td className="num">{d.discovered}</td><td className="num">{d.submitted}</td><td className="num">{d.interviews}</td></tr>
            ))}
          </tbody>
        </table>
      ) : (
        <div className="chart-wrap" onMouseLeave={() => setTip(null)}>
          <svg viewBox={`0 0 ${W} ${H}`} role="img" aria-label="Weekly job-search activity, last 12 weeks">
            {ticks.map((t) => (
              <g key={t}>
                <line x1={padL} x2={W - 8} y1={y(t)} y2={y(t)} className="grid" />
                <text x={padL - 6} y={y(t) + 4} className="axis" textAnchor="end">{t}</text>
              </g>
            ))}
            {data.map((d, i) => {
              const gx = padL + i * groupW;
              const vals = [d.discovered, d.submitted, d.interviews];
              const start = gx + (groupW - (barW * 3 + 4)) / 2;
              return (
                <g key={d.weekStart}>
                  {vals.map((v, k) => {
                    const h = (v / nice) * plotH;
                    if (v === 0) return null;
                    const x = start + k * (barW + 2);
                    const r = Math.min(4, barW / 2, h);
                    // Rounded top only, anchored on the baseline.
                    const top = y(v);
                    const base = padT + plotH;
                    return (
                      <path key={k} fill={SERIES[k].color}
                        d={`M${x},${base} V${top + r} Q${x},${top} ${x + r},${top} H${x + barW - r} Q${x + barW},${top} ${x + barW},${top + r} V${base} Z`} />
                    );
                  })}
                  {i % 2 === 0 && (
                    <text x={gx + groupW / 2} y={H - 8} className="axis" textAnchor="middle">
                      {new Date(d.weekStart + "T00:00").toLocaleDateString("en-US", { month: "short", day: "numeric" })}
                    </text>
                  )}
                  <rect x={gx} y={padT} width={groupW} height={plotH} fill="transparent"
                    onMouseMove={(e) => {
                      const box = (e.currentTarget.ownerSVGElement as SVGSVGElement).getBoundingClientRect();
                      setTip({
                        x: ((gx + groupW / 2) / W) * box.width,
                        y: 8,
                        html: [`Week of ${shortDate(d.weekStart)}`, ...SERIES.map((s, k) => `${s.label}: ${vals[k]}`)],
                      });
                    }} />
                </g>
              );
            })}
            <line x1={padL} x2={W - 8} y1={padT + plotH} y2={padT + plotH} className="baseline" />
          </svg>
          {tip && (
            <div className="tooltip" style={{ left: tip.x, top: tip.y }}>
              {tip.html.map((l, i) => <div key={i} className={i === 0 ? "tip-title" : ""}>{l}</div>)}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

/** Single-series horizontal bars (magnitude) with value labels at the end of each bar. */
export function HBarChart({ data, emptyNote }: { data: LabelCount[]; emptyNote?: string }) {
  const max = Math.max(1, ...data.map((d) => d.count));
  const total = data.reduce((a, d) => a + d.count, 0);
  return (
    <div className="hbars">
      {data.map((d) => (
        <div className="hbar-row" key={d.label} title={`${d.label}: ${d.count}`}>
          <span className="hbar-label">{d.label}</span>
          <span className="hbar-track">
            {d.count > 0 && <span className="hbar-fill" style={{ width: `${(d.count / max) * 100}%` }} />}
          </span>
          <span className="hbar-value">{d.count}</span>
        </div>
      ))}
      {emptyNote && total > 0 && <p className="muted small">{emptyNote}</p>}
    </div>
  );
}
