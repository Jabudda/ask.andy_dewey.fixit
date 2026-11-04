import type { ScoreBreakdown } from "../lib/types";
import { Card } from "./ui";

const REC: Record<string, [string, string]> = {
  apply: ["✓ Recommended — apply", "good"],
  consider: ["! Worth considering", "warn"],
  skip: ["✕ Not recommended", "bad"],
};

function List({ title, items, tone }: { title: string; items: string[]; tone: string }) {
  if (!items.length) return null;
  return (
    <details className="small" open={tone !== "muted"}>
      <summary><strong>{title}</strong> ({items.length})</summary>
      <ul className={`score-list tone-text-${tone}`}>{items.map((x, i) => <li key={i}>{x}</li>)}</ul>
    </details>
  );
}

/** Hiring-manager assessment: total, weighted criteria, and the evidence behind them. */
export function ScoreCard({ b }: { b: ScoreBreakdown | null }) {
  if (!b) return <Card title="Compatibility"><p className="muted">Not scored yet.</p></Card>;
  const [label, tone] = REC[b.recommendation] ?? REC.skip;
  return (
    <Card title="Compatibility & hiring-manager assessment">
      <div className="score-head">
        <div className={`score-num tone-text-${tone}`}>{b.total}<span>/100</span></div>
        <span className={`badge tone-${tone}`}>{label}</span>
      </div>
      <div className="criteria">
        {b.criteria.map((c) => (
          <div key={c.key} className="criterion" title={c.detail}>
            <div className="criterion-head"><span>{c.label}</span><span className="muted small">weight {c.weight} · {Math.round(c.score * 100)}%</span></div>
            <div className="hbar-track"><span className="hbar-fill" style={{ width: `${Math.max(2, c.score * 100)}%` }} /></div>
            <div className="muted small">{c.detail}</div>
          </div>
        ))}
      </div>
      <List title="Requirements met" items={b.satisfied} tone="good" />
      <List title="Partially met" items={b.partial} tone="warn" />
      <List title="Gaps" items={b.gaps} tone="bad" />
      <List title="Could not be checked — your judgment" items={b.unverified} tone="muted" />
      <p className="muted small">Scored only against your verified profile and master resume. {b.assumptions.join(" ")} Adjust the weights in Preferences.</p>
    </Card>
  );
}
