import type { Agent, Target } from "../types";
import {
  agentName,
  formatNumber,
  formatUptime,
  orderedAgentIds,
  specLabel,
  type SparkPoint,
  type TargetMetrics,
} from "../metrics";
import { TargetDetail } from "./TargetDetail";
import { StatusDot } from "./ui";

export type StatusFilter = "all" | "fail" | "slow";
export type ViewName = "targets" | "agents" | "alerts";

interface TargetsProps {
  agents: Agent[];
  targets: Target[];
  metrics: Record<string, TargetMetrics>;
  selectedId: string | null;
  onSelect: (id: string | null) => void;
  onAdd: () => void;
  onEdit: (target: Target) => void;
  onDelete: (target: Target) => void;
  onCheck: (target: Target) => void;
  onToggleAgent: (agentId: string) => void;
  onNavigate: (view: ViewName) => void;
  onLogout: () => void;
  selectedAgentIds: string[];
  query: string;
  onQuery: (value: string) => void;
  status: StatusFilter;
  onStatus: (value: StatusFilter) => void;
  agentFilter: string;
  onAgentFilter: (value: string) => void;
  user: string;
  loading: boolean;
  wide: boolean;
  phone: boolean;
  busyTargetId: string | null;
}

function Spark({ points }: { points: SparkPoint[] }) {
  return (
    <span className="spark" aria-hidden="true">
      {points.length === 0 ? <i style={{ height: "8%" }} /> : null}
      {points.map((point, index) => (
        <i
          key={index}
          className={point.failed ? "f" : undefined}
          style={{ height: `${Math.round(point.value * 100)}%` }}
        />
      ))}
    </span>
  );
}

function AgentRail({
  target,
  metrics,
  agents,
}: {
  target: Target;
  metrics: TargetMetrics;
  agents: Agent[];
}) {
  const rows = orderedAgentIds(target, agents).map((id) => ({
    id,
    agent: agents.find((agent) => agent.id === id),
    check: metrics.perAgent[id],
  }));
  const responded = rows.filter((row) => row.check?.last).length;
  return (
    <span className="agents" title={rows.map((row) => agentName(agents, row.id)).join(" · ")}>
      <span className="arail" aria-hidden="true">
        {rows.map((row) => {
          let className = "";
          if (!row.agent?.connected) className = "off";
          if (!row.check?.last) className = row.agent?.connected ? "n" : "off";
          if (row.check?.last && !row.check.last.ok) className = "f";
          return <i key={row.id} className={className} />;
        })}
      </span>
      <span className="arail-meta">
        <b>
          {responded}/{rows.length}
        </b>
        <span>макс {formatNumber(metrics.maxLatency)}</span>
      </span>
    </span>
  );
}

export function TargetsScreen(props: TargetsProps) {
  const {
    agents,
    targets,
    metrics,
    selectedId,
    onSelect,
    onAdd,
    onEdit,
    onDelete,
    onCheck,
    onToggleAgent,
    onNavigate,
    onLogout,
    selectedAgentIds,
    query,
    onQuery,
    status,
    onStatus,
    agentFilter,
    onAgentFilter,
    user,
    loading,
    wide,
    phone,
    busyTargetId,
  } = props;

  const selected = selectedId ? targets.find((target) => target.id === selectedId) ?? null : null;
  const selectedMetrics = selected ? metrics[selected.id] : undefined;

  return (
    <div className="screen">
      <header className="appbar">
        <div className="m-brand">
          <span className="mark" />
        </div>
        <div className="title-row">
          <h1>Цели</h1>
          <span className="count">{targets.length}</span>
          <button type="button" className="add" aria-label="Добавить цель" onClick={onAdd}>
            <span aria-hidden="true">+</span>
          </button>
        </div>
        <div className="appbar-tools">
          <label className="search">
            <svg width="13" height="13" viewBox="0 0 16 16" aria-hidden="true">
              <circle cx="7" cy="7" r="4.5" fill="none" stroke="currentColor" />
              <path d="M10.5 10.5 L14 14" stroke="currentColor" />
            </svg>
            <input
              type="search"
              placeholder="Имя или хост"
              aria-label="Поиск цели"
              value={query}
              onChange={(event) => onQuery(event.target.value)}
            />
          </label>
          <div className="seg" role="group" aria-label="Фильтр состояния">
            <button
              type="button"
              aria-pressed={status === "all"}
              onClick={() => onStatus("all")}
            >
              Все
            </button>
            <button
              type="button"
              aria-pressed={status === "fail"}
              onClick={() => onStatus("fail")}
            >
              Сбои
            </button>
            <button
              type="button"
              aria-pressed={status === "slow"}
              onClick={() => onStatus("slow")}
            >
              Медленные
            </button>
          </div>
          <button type="button" className="user" onClick={onLogout} title="Выйти">
            <i>{user.slice(0, 1).toLowerCase()}</i>
            {user}
          </button>
        </div>
      </header>

      <nav className="tabs" aria-label="Разделы">
        <button type="button" aria-current="page" onClick={() => onNavigate("targets")}>
          Цели <span className="n">{targets.length}</span>
        </button>
        <button type="button" onClick={() => onNavigate("agents")}>
          Агенты <span className="n">{agents.length}</span>
        </button>
        <button type="button" onClick={() => onNavigate("alerts")}>
          Оповещения
        </button>
      </nav>

      <div className="toolbar">
        <div className="agent-filter" role="group" aria-label="Фильтр по агенту">
          <button
            type="button"
            aria-pressed={agentFilter === "all"}
            onClick={() => onAgentFilter("all")}
          >
            все {agents.length}
          </button>
          {agents.map((agent) => (
            <button
              key={agent.id}
              type="button"
              aria-pressed={agentFilter === agent.id}
              onClick={() => onAgentFilter(agent.id)}
            >
              {agent.name}
            </button>
          ))}
        </div>
        <select
          className="agent-select"
          aria-label="Фильтр по агенту"
          value={agentFilter}
          onChange={(event) => onAgentFilter(event.target.value)}
        >
          <option value="all">все агенты · {agents.length}</option>
          {agents.map((agent) => (
            <option key={agent.id} value={agent.id}>
              {agent.name}
            </option>
          ))}
        </select>
        <span className="toolbar-note">проверки по расписанию · история 30 дней</span>
      </div>
      <div className="mobile-controls">
        <label className="search">
          <svg width="13" height="13" viewBox="0 0 16 16" aria-hidden="true">
            <circle cx="7" cy="7" r="4.5" fill="none" stroke="currentColor" />
            <path d="M10.5 10.5 L14 14" stroke="currentColor" />
          </svg>
          <input
            type="search"
            placeholder="Имя или хост"
            aria-label="Поиск цели"
            value={query}
            onChange={(event) => onQuery(event.target.value)}
          />
        </label>
        <div className="seg" role="group" aria-label="Фильтр состояния">
          <button type="button" aria-pressed={status === "all"} onClick={() => onStatus("all")}>
            Все
          </button>
          <button type="button" aria-pressed={status === "fail"} onClick={() => onStatus("fail")}>
            Сбои
          </button>
          <button type="button" aria-pressed={status === "slow"} onClick={() => onStatus("slow")}>
            Медленные
          </button>
        </div>
        <select
          className="agent-select"
          aria-label="Фильтр по агенту"
          value={agentFilter}
          onChange={(event) => onAgentFilter(event.target.value)}
        >
          <option value="all">все агенты · {agents.length}</option>
          {agents.map((agent) => (
            <option key={agent.id} value={agent.id}>
              {agent.name}
            </option>
          ))}
        </select>
      </div>

      <div className="workspace">
        <div className="listcol">
          <div className="cols">
            <span>Цель</span>
            <span>{agents.length} агентов</span>
            <span>24 часа</span>
            <span className="r">Аптайм 24 ч</span>
            <span className="r">p95, мс</span>
          </div>
          <div className="rows">
            {loading && targets.length === 0 ? (
              <p className="list-note">Загружаем цели и историю…</p>
            ) : null}
            {!loading && targets.length === 0 ? (
              <div className="empty-state">
                <h3>Пока нет целей</h3>
                <p>Добавьте первый HTTP, TCP или ICMP-эндпоинт и назначьте его агентам.</p>
                <button type="button" className="btn" onClick={onAdd}>
                  Создать цель
                </button>
              </div>
            ) : null}
            {targets.map((target) => {
              const targetMetrics = metrics[target.id];
              if (!targetMetrics) return null;
              const health = targetMetrics.health === "idle" ? "offline" : targetMetrics.health;
              return (
                <div key={target.id} className="row-wrap">
                  <button
                    type="button"
                    className="row"
                    aria-current={selectedId === target.id ? "true" : undefined}
                    onClick={() => onSelect(target.id)}
                  >
                    <span className="who">
                      <StatusDot health={health} />
                      <span>
                        <span className="name">{target.name}</span>
                        <span className="sub">
                          {specLabel(target.spec)} · {target.interval_secs} с
                        </span>
                      </span>
                    </span>
                    <AgentRail target={target} metrics={targetMetrics} agents={agents} />
                    <Spark points={targetMetrics.spark} />
                    <span className="metric metric--uptime">
                      {formatUptime(targetMetrics.uptime)}
                      <small>за 24 ч</small>
                    </span>
                    <span className="metric metric--p95">
                      {formatNumber(targetMetrics.p95)}
                      <small>мс</small>
                    </span>
                  </button>
                  {!wide && !phone && selectedId === target.id && selectedMetrics ? (
                    <div className="row-expand">
                      <TargetDetail
                        target={target}
                        metrics={selectedMetrics}
                        agents={agents}
                        selectedAgentIds={selectedAgentIds}
                        onToggleAgent={onToggleAgent}
                        onCheck={() => onCheck(target)}
                        onEdit={() => onEdit(target)}
                        onDelete={() => onDelete(target)}
                        busy={busyTargetId === target.id}
                      />
                    </div>
                  ) : null}
                </div>
              );
            })}
          </div>
        </div>

        {wide ? (
          <aside className="detail">
            {selected && selectedMetrics ? (
              <TargetDetail
                target={selected}
                metrics={selectedMetrics}
                agents={agents}
                selectedAgentIds={selectedAgentIds}
                onToggleAgent={onToggleAgent}
                onCheck={() => onCheck(selected)}
                onEdit={() => onEdit(selected)}
                onDelete={() => onDelete(selected)}
                busy={busyTargetId === selected.id}
              />
            ) : (
              <div className="detail-placeholder">
                <p>Выберите цель, чтобы увидеть историю и разбивку по агентам.</p>
              </div>
            )}
          </aside>
        ) : null}
      </div>
    </div>
  );
}
