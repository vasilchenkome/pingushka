import { useEffect, useState } from "react";
import type { Agent, CheckResult, Summary, Target } from "../types";
import { api } from "../api";
import {
  agentChecks,
  agentName,
  formatClock,
  formatNumber,
  formatUptime,
  orderedAgentIds,
  relativeTime,
  specLabel,
  specTarget,
  type TargetMetrics,
} from "../metrics";
import { HistoryChart } from "./Chart";
import { StatusDot } from "./ui";

interface DetailProps {
  target: Target;
  metrics: TargetMetrics;
  agents: Agent[];
  selectedAgentIds: string[];
  onToggleAgent: (agentId: string) => void;
  onCheck: () => void;
  onEdit: () => void;
  onDelete: () => void;
  onClose?: () => void;
  busy?: boolean;
}

const HEALTH_LABEL = {
  ok: "работает",
  warn: "медленно",
  fail: "сбой",
  idle: "нет данных",
} as const;

export function TargetDetail({
  target,
  metrics,
  agents,
  selectedAgentIds,
  onToggleAgent,
  onCheck,
  onEdit,
  onDelete,
  onClose,
  busy,
}: DetailProps) {
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [summary, setSummary] = useState<Summary | null>(null);
  const [agentSummaries, setAgentSummaries] = useState<Record<string, Summary>>({});
  const [chartResults, setChartResults] = useState<Record<string, CheckResult[]>>({});

  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      const chartIds =
        selectedAgentIds.length > 0 ? selectedAgentIds : target.agent_ids.slice(0, 2);
      try {
        const [series, perAgent] = await Promise.all([
          Promise.all(
            chartIds.map(async (id) => {
              try {
                return [id, await api.history(target.id, 2000, id)] as const;
              } catch {
                return [id, [] as CheckResult[]] as const;
              }
            }),
          ),
          Promise.all(
            target.agent_ids.map(async (id) => {
              try {
                return [id, await api.summary(target.id, id)] as const;
              } catch {
                return null;
              }
            }),
          ),
        ]);
        const totals = await api.summary(target.id).catch(() => null);
        if (cancelled) return;
        setChartResults(Object.fromEntries(series));
        setAgentSummaries(
          Object.fromEntries(perAgent.filter((entry) => entry !== null) as [string, Summary][]),
        );
        setSummary(totals);
      } catch {
        // Панель остаётся с данными из общего обновления, если детальные запросы не удались.
      }
    };
    void load();
    return () => {
      cancelled = true;
    };
  }, [target.id, target.updated_at, selectedAgentIds.join(","), metrics.lastCheckedAt]);

  const rows = agentChecks(target, metrics, agents);
  const checksByAgent = rows.map((row) => ({
    ...row,
    name: agentName(agents, row.agentId),
    connected: agents.find((agent) => agent.id === row.agentId)?.connected ?? false,
    uptime: agentSummaries[row.agentId]?.uptime_percent ?? row.uptime,
    average:
      agentSummaries[row.agentId]?.average_latency_ms ??
      (row.last ? row.last.latency_ms : null),
  }));
  const selected = orderedAgentIds(target, agents).filter((id) =>
    selectedAgentIds.includes(id),
  );
  const series = selected.map((id) => ({
    id,
    name: agentName(agents, id),
    results:
      chartResults[id] ?? metrics.results.filter((result) => result.agent_id === id),
    dashed: selected.indexOf(id) > 0,
  }));
  const windowEnd = Math.floor(Date.now() / 1000);
  const windowStart = windowEnd - 24 * 3600;
  const uptime = summary?.uptime_percent ?? metrics.uptime;
  const p95 = summary?.p95_latency_ms ?? metrics.p95;
  const checksTotal = summary?.total_checks ?? metrics.total;

  return (
    <div className="detail-inner">
      <header className="detail-head">
        <div className="detail-title">
          <h2>{target.name}</h2>
          <p className="sub">
            {specLabel(target.spec)} · {specTarget(target.spec)} · каждые {target.interval_secs} с ·
            таймаут {formatNumber(target.timeout_ms / 1000, 1)} с
          </p>
        </div>
        <div className="detail-head-actions">
          <span className={`state state--${metrics.health}`}>
            <StatusDot health={metrics.health} />
            {HEALTH_LABEL[metrics.health]}
          </span>
          {onClose ? (
            <button type="button" className="icon-button" onClick={onClose} aria-label="Назад">
              ←
            </button>
          ) : null}
        </div>
      </header>

      <dl className="stats">
        <div className="stat">
          <dt>аптайм</dt>
          <dd>{formatUptime(uptime)}</dd>
        </div>
        <div className="stat">
          <dt>p95</dt>
          <dd>{formatNumber(p95)} мс</dd>
        </div>
        <div className="stat">
          <dt>проверок за 24 ч</dt>
          <dd>{formatNumber(checksTotal)}</dd>
        </div>
      </dl>

      <section className="chart-block">
        <div className="chart-head">
          <h3>История отклика, 24 часа</h3>
          <span className="chart-note">
            {selected.length} из {target.agent_ids.length}
          </span>
        </div>
        {metrics.total === 0 ? (
          <p className="empty-note">
            Пока нет проверок. Агент пришлёт данные после первого цикла опроса.
          </p>
        ) : (
          <HistoryChart series={series} from={windowStart} to={windowEnd} />
        )}
        <div className="chart-legend">
          {series.map((entry, index) => (
            <span key={entry.id}>
              <i className={index === 0 ? "line" : "line line--dashed"} />
              {entry.name}
            </span>
          ))}
          <span>
            <i className="fail" /> сбой
          </span>
          <span className="unit">шкала, мс</span>
        </div>
        <div className="agent-picks" role="group" aria-label="Агенты на графике">
          {orderedAgentIds(target, agents).map((id) => (
            <button
              key={id}
              type="button"
              className={selected.includes(id) ? "pick pick--on" : "pick"}
              aria-pressed={selected.includes(id)}
              onClick={() => onToggleAgent(id)}
            >
              {agentName(agents, id)}
            </button>
          ))}
        </div>
      </section>

      <section className="checks">
        <h3>Агенты на цели</h3>
        <table>
          <thead>
            <tr>
              <th>агент</th>
              <th className="r">аптайм</th>
              <th className="r">мс</th>
              <th className="r">сигнал</th>
            </tr>
          </thead>
          <tbody>
            {checksByAgent.length === 0 ? (
              <tr>
                <td colSpan={4} className="empty-cell">
                  Цель ещё не назначена агентам.
                </td>
              </tr>
            ) : (
              checksByAgent.map((row) => (
                <tr key={row.agentId}>
                  <td>
                    <i className={row.connected ? "dot dot--ok" : "dot dot--offline"} aria-hidden="true" />
                    <span className={row.last && !row.last.ok ? "bad" : undefined}>{row.name}</span>
                    {!row.connected ? <span className="offline-tag">offline</span> : null}
                  </td>
                  <td className="r">{row.uptime === null ? "—" : formatNumber(row.uptime, 2)}</td>
                  <td className="r">{row.average === null ? "—" : formatNumber(row.average)}</td>
                  <td className="r">{row.last ? formatClock(row.last.checked_at) : "—"}</td>
                </tr>
              ))
            )}
          </tbody>
        </table>
        <p className="detail-note">
          Последний сигнал: {relativeTime(metrics.lastCheckedAt)}. История хранится 30 дней.
        </p>
      </section>

      <footer className="detail-actions">
        <button type="button" className="btn" onClick={onCheck} disabled={busy}>
          {busy ? "Запускаем…" : "Проверить сейчас"}
        </button>
        <button type="button" className="btn btn--ghost" onClick={onEdit}>
          Изменить
        </button>
        {confirmDelete ? (
          <>
            <button type="button" className="btn btn--danger" onClick={onDelete}>
              Удалить цель
            </button>
            <button type="button" className="link-button" onClick={() => setConfirmDelete(false)}>
              Отмена
            </button>
          </>
        ) : (
          <button type="button" className="link-button" onClick={() => setConfirmDelete(true)}>
            Удалить
          </button>
        )}
      </footer>
    </div>
  );
}
