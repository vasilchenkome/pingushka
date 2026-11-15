import type { CheckResult } from "../types";

export interface Series {
  id: string;
  name: string;
  results: CheckResult[];
  dashed?: boolean;
}

interface ChartProps {
  series: Series[];
  to: number;
  from: number;
}

const WIDTH = 344;
const HEIGHT = 190;
const LEFT = 4;
const RIGHT = 40;
const TOP = 8;
const BOTTOM = 24;

export function HistoryChart({ series, from, to }: ChartProps) {
  const points = series.flatMap((entry) => entry.results);
  const width = WIDTH - LEFT - RIGHT;
  const height = HEIGHT - TOP - BOTTOM;
  const span = Math.max(1, to - from);
  const rawMax = Math.max(200, ...points.map((point) => point.latency_ms));
  const max = Math.ceil(rawMax / 200) * 200;

  const x = (seconds: number) => LEFT + ((seconds - from) / span) * width;
  const y = (value: number) => TOP + height - (Math.min(value, max) / max) * height;

  const ticks = [0, 0.25, 0.5, 0.75, 1];
  const failures = points.filter((point) => !point.ok);

  return (
    <svg
      className="chart"
      viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
      role="img"
      aria-label="История отклика выбранных агентов"
    >
      {ticks.map((fraction) => {
        const value = Math.round((max * fraction) / 10) * 10;
        const py = y(value);
        return (
          <g key={fraction}>
            <line x1={LEFT} x2={LEFT + width} y1={py} y2={py} stroke="var(--rule)" strokeWidth="1" />
            <text
              x={LEFT + width + 6}
              y={py + 3.5}
              fill="var(--slate-light)"
              fontSize="9"
              fontFamily="var(--font-mono)"
            >
              {value}
            </text>
          </g>
        );
      })}
      {ticks.map((fraction) => {
        const px = LEFT + fraction * width;
        const hour = new Date((from + span * fraction) * 1000).toLocaleTimeString("ru-RU", {
          hour: "2-digit",
          minute: "2-digit",
        });
        return (
          <g key={`x-${fraction}`}>
            <line
              x1={px}
              x2={px}
              y1={TOP}
              y2={TOP + height}
              stroke="var(--rule)"
              strokeWidth="1"
              strokeDasharray="2 3"
            />
            <text
              x={px}
              y={HEIGHT - 6}
              fill="var(--slate-light)"
              fontSize="9"
              fontFamily="var(--font-mono)"
              textAnchor={fraction === 0 ? "start" : fraction === 1 ? "end" : "middle"}
            >
              {hour}
            </text>
          </g>
        );
      })}
      {series.map((entry) => {
        const ordered = [...entry.results].sort(
          (left, right) => left.checked_at_unix - right.checked_at_unix,
        );
        if (ordered.length === 0) return null;
        const path = ordered
          .map((point, index) => {
            const command = index === 0 ? "M" : "L";
            return `${command}${x(point.checked_at_unix).toFixed(1)} ${y(point.latency_ms).toFixed(1)}`;
          })
          .join(" ");
        return (
          <path
            key={entry.id}
            d={path}
            fill="none"
            stroke={entry.dashed ? "var(--slate)" : "var(--signal)"}
            strokeWidth={entry.dashed ? 1.25 : 1.6}
            strokeDasharray={entry.dashed ? "3 3" : undefined}
          />
        );
      })}
      {failures.slice(0, 60).map((point) => (
        <rect
          key={point.result_id}
          x={x(point.checked_at_unix) - 1}
          y={TOP + height + 2}
          width="3"
          height="7"
          fill="var(--alert)"
        />
      ))}
    </svg>
  );
}
