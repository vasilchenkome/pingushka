import { useEffect, useMemo, useRef, useState } from "react";
import type { Agent, BulkGroupMode, BulkTagMode, Target, TargetGroup } from "../types";
import { api, errorText } from "../api";
import {
  agentName,
  formatNumber,
  formatUptime,
  orderedAgentIds,
  specLabel,
  type SparkPoint,
  type TargetMetrics,
} from "../metrics";
import {
  filterByTags,
  groupTargets,
  sortTargets,
  type SortDirection,
  type SortKey,
} from "../targetListView";
import { TagInput, tagTone } from "./TagInput";
import { TargetDetail } from "./TargetDetail";
import { TargetGroupsDialog } from "./TargetGroups";
import { StatusDot } from "./ui";

export type StatusFilter = "all" | "fail" | "slow";
export type ViewName = "targets" | "agents" | "alerts";

function targetsWord(count: number): string {
  const mod100 = count % 100;
  const mod10 = count % 10;
  if (mod100 >= 11 && mod100 <= 14) return "целей";
  if (mod10 === 1) return "цель";
  if (mod10 >= 2 && mod10 <= 4) return "цели";
  return "целей";
}

interface TargetsProps {
  agents: Agent[];
  targets: Target[];
  groups: TargetGroup[];
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
  onChanged: () => void;
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
    groups,
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
    onChanged,
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

  const [sortKey, setSortKey] = useState<SortKey>(
    () => (localStorage.getItem("pingushka.sort") as SortKey | null) ?? "status",
  );
  const [direction, setDirection] = useState<SortDirection>(
    () => (localStorage.getItem("pingushka.direction") as SortDirection | null) ?? "asc",
  );
  const [grouped, setGrouped] = useState(
    () => localStorage.getItem("pingushka.grouped") !== "false",
  );
  const [collapsed, setCollapsed] = useState<string[]>(() => {
    try {
      return JSON.parse(localStorage.getItem("pingushka.collapsed") ?? "[]") as string[];
    } catch {
      return [];
    }
  });
  const [tagFilter, setTagFilter] = useState<string[]>([]);
  const [tagPanel, setTagPanel] = useState(false);
  const tagFilterButtonRef = useRef<HTMLButtonElement>(null);
  const tagFilterRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!tagPanel) return;
    const closeOnOutsidePress = (event: PointerEvent) => {
      const target = event.target as Node | null;
      if (target && !tagFilterRef.current?.contains(target)) setTagPanel(false);
    };
    document.addEventListener("pointerdown", closeOnOutsidePress);
    return () => document.removeEventListener("pointerdown", closeOnOutsidePress);
  }, [tagPanel]);
  const [groupManager, setGroupManager] = useState(false);
  const [bulk, setBulk] = useState(false);
  const [chosen, setChosen] = useState<string[]>([]);
  const [bulkAction, setBulkAction] = useState<"group" | "tags" | null>(null);
  const [bulkGroup, setBulkGroup] = useState("");
  const [bulkGroupName, setBulkGroupName] = useState("");
  const [bulkTagMode, setBulkTagMode] = useState<"add" | "remove" | "replace">("add");
  const [bulkTags, setBulkTags] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const allTags = useMemo(
    () => [...new Set(targets.flatMap((target) => target.tags))].sort(),
    [targets],
  );
  const visible = useMemo(
    () => sortTargets(filterByTags(targets, tagFilter), metrics, sortKey, direction),
    [targets, tagFilter, metrics, sortKey, direction],
  );
  const views = useMemo(
    () => (grouped ? groupTargets(visible, metrics, direction) : [{ key: "", name: "", targets: visible, failed: 0, health: "ok" as const }]),
    [grouped, visible, metrics, direction],
  );

  const setSort = (next: SortKey) => {
    setSortKey(next);
    localStorage.setItem("pingushka.sort", next);
  };
  const flipDirection = () => {
    const next = direction === "asc" ? "desc" : "asc";
    setDirection(next);
    localStorage.setItem("pingushka.direction", next);
  };
  const toggleGrouped = () => {
    setGrouped((current) => {
      localStorage.setItem("pingushka.grouped", String(!current));
      return !current;
    });
  };
  const toggleCollapsed = (key: string) => {
    setCollapsed((current) => {
      const next = current.includes(key)
        ? current.filter((entry) => entry !== key)
        : [...current, key];
      localStorage.setItem("pingushka.collapsed", JSON.stringify(next));
      return next;
    });
  };
  const toggleChosen = (id: string) => {
    setChosen((current) =>
      current.includes(id) ? current.filter((entry) => entry !== id) : [...current, id],
    );
  };
  const toggleTagFilter = (tag: string) => {
    setTagFilter((current) =>
      current.includes(tag) ? current.filter((entry) => entry !== tag) : [...current, tag],
    );
  };
  const applyGroup = async () => {
    let group: BulkGroupMode;
    if (bulkGroup === "__clear") group = { mode: "clear" };
    else if (bulkGroup === "__new") group = { mode: "create", name: bulkGroupName.trim() };
    else if (bulkGroup) group = { mode: "set", group_id: bulkGroup };
    else group = { mode: "keep" };
    await applyBulk(group, { mode: "keep" });
  };
  const applyTags = async () => {
    await applyBulk({ mode: "keep" }, { mode: bulkTagMode, values: bulkTags });
  };
  const applyBulk = async (group: BulkGroupMode, tags: BulkTagMode) => {
    setBusy(true);
    setError(null);
    try {
      await api.bulkUpdateTargets({ target_ids: chosen, group, tags });
      setBulkAction(null);
      setChosen([]);
      setBulk(false);
      setBulkGroup("");
      setBulkGroupName("");
      setBulkTags([]);
      onChanged();
    } catch (cause) {
      setError(errorText(cause));
    } finally {
      setBusy(false);
    }
  };

  const selected = selectedId ? targets.find((target) => target.id === selectedId) ?? null : null;
  const selectedMetrics = selected ? metrics[selected.id] : undefined;

  return (
    <div className="screen">
      <header className="appbar">
        <div className="m-brand">
          <span className="mark" />
        </div>
        <div className="title-row">
          <h1 tabIndex={-1} data-dialog-fallback-focus>Цели</h1>
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

      <div className="target-tools">
        <div className="sort-control">
          <label htmlFor="target-sort">Сортировка</label>
          <select
            id="target-sort"
            value={sortKey}
            onChange={(event) => setSort(event.target.value as SortKey)}
          >
            <option value="status">Сначала сбои</option>
            <option value="name">Имя</option>
            <option value="uptime">Аптайм</option>
            <option value="p95">p95</option>
            <option value="checked">Последняя проверка</option>
            <option value="group">Группа</option>
          </select>
          <button type="button" className="btn btn--ghost btn--small" aria-label={direction === "asc" ? "По возрастанию" : "По убыванию"} onClick={flipDirection}>
            {direction === "asc" ? "↑" : "↓"}
          </button>
        </div>
        <button
          type="button"
          className={`btn btn--ghost btn--small${grouped ? " is-on" : ""}`}
          aria-pressed={grouped}
          onClick={toggleGrouped}
        >
          Группировка
        </button>
        <button
          type="button"
          className="btn btn--ghost btn--small"
          onClick={() => setGroupManager(true)}
        >
          Группы…
        </button>
        <div
          className="tag-filter"
          ref={tagFilterRef}
          onKeyDown={(event) => {
            if (event.key === "Escape" && tagPanel) {
              event.preventDefault();
              setTagPanel(false);
              tagFilterButtonRef.current?.focus();
            }
          }}
        >
          <button
            ref={tagFilterButtonRef}
            type="button"
            className={`btn btn--ghost btn--small${tagFilter.length > 0 ? " is-on" : ""}`}
            aria-expanded={tagPanel}
            aria-controls={tagPanel ? "tag-filter-options" : undefined}
            onClick={() => setTagPanel((open) => !open)}
          >
            Теги{tagFilter.length > 0 ? ` · ${tagFilter.length}` : ""}
          </button>
          {tagPanel ? (
            <div
              id="tag-filter-options"
              className="tag-filter__panel"
              role="group"
              aria-label="Фильтр по тегам"
            >
              {allTags.length === 0 ? <p className="field-hint">Тегов пока нет.</p> : null}
              {allTags.map((tag) => (
                <button
                  key={tag}
                  type="button"
                  className={`tag tag--target tag--${tagTone(tag)}${tagFilter.includes(tag) ? " tag--on" : ""}`}
                  aria-pressed={tagFilter.includes(tag)}
                  onClick={() => toggleTagFilter(tag)}
                >
                  {tag}
                </button>
              ))}
              {tagFilter.length > 0 ? (
                <button
                  type="button"
                  className="btn btn--ghost btn--small"
                  onClick={() => setTagFilter([])}
                >
                  Сбросить
                </button>
              ) : null}
            </div>
          ) : null}
        </div>
        <button
          type="button"
          className={`btn btn--ghost btn--small${bulk ? " is-on" : ""}`}
          onClick={() => {
            setBulk((current) => !current);
            setChosen([]);
            setBulkAction(null);
          }}
        >
          {bulk ? "Отмена выбора" : "Выбрать"}
        </button>
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

      {bulk ? (
        <div className="bulk-bar">
          <strong>Выбрано {chosen.length}</strong>
          <button
            type="button"
            className="btn btn--ghost btn--small"
            disabled={chosen.length === 0}
            onClick={() => setBulkAction(bulkAction === "group" ? null : "group")}
          >
            Группа
          </button>
          <button
            type="button"
            className="btn btn--ghost btn--small"
            disabled={chosen.length === 0}
            onClick={() => setBulkAction(bulkAction === "tags" ? null : "tags")}
          >
            Теги
          </button>
          <button
            type="button"
            className="btn btn--ghost btn--small"
            onClick={() => setChosen([])}
          >
            Снять выбор
          </button>
          {error ? <span className="bulk-bar__error">{error}</span> : null}
          {bulkAction === "group" ? (
            <div className="bulk-form">
              <select aria-label="Назначить группу выбранным целям" value={bulkGroup} onChange={(event) => setBulkGroup(event.target.value)}>
                <option value="">Не менять</option>
                <option value="__clear">Без группы</option>
                {groups.map((group) => (
                  <option key={group.id} value={group.id}>
                    {group.name}
                  </option>
                ))}
                <option value="__new">Новая группа…</option>
              </select>
              {bulkGroup === "__new" ? (
                <input
                  value={bulkGroupName}
                  onChange={(event) => setBulkGroupName(event.target.value)}
                  placeholder="Название группы"
                  aria-label="Название новой группы"
                />
              ) : null}
              <button
                type="button"
                className="btn btn--small"
                disabled={busy || (bulkGroup === "__new" && bulkGroupName.trim() === "")}
                onClick={() => void applyGroup()}
              >
                Применить
              </button>
            </div>
          ) : null}
          {bulkAction === "tags" ? (
            <div className="bulk-form">
              <select
                aria-label="Операция с тегами выбранных целей"
                value={bulkTagMode}
                onChange={(event) =>
                  setBulkTagMode(event.target.value as "add" | "remove" | "replace")
                }
              >
                <option value="add">Добавить</option>
                <option value="remove">Удалить</option>
                <option value="replace">Заменить</option>
              </select>
              <TagInput
                id="bulk-tags"
                ariaLabel="Теги для выбранных целей"
                value={bulkTags}
                onChange={setBulkTags}
                suggestions={allTags}
              />
              <button
                type="button"
                className="btn btn--small"
                disabled={busy || bulkTags.length === 0}
                onClick={() => void applyTags()}
              >
                Применить
              </button>
            </div>
          ) : null}
        </div>
      ) : null}

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
            {views.map((view) => (
              <div key={view.key || "all"} className="target-group">
                {grouped ? (
                  <button
                    type="button"
                    className="target-group__head"
                    aria-expanded={!collapsed.includes(view.key)}
                    onClick={() => toggleCollapsed(view.key)}
                  >
                    <span aria-hidden="true">
                      {collapsed.includes(view.key) ? "▸" : "▾"}
                    </span>
                    <strong>{view.name}</strong>
                    <span>{view.targets.length} {targetsWord(view.targets.length)}</span>
                    {view.failed > 0 ? <em>есть сбой · {view.failed}</em> : null}
                  </button>
                ) : null}
                {!collapsed.includes(view.key) || !grouped
                  ? view.targets.map((target) => {
                      const targetMetrics = metrics[target.id];
                      if (!targetMetrics) return null;
                      const health =
                        targetMetrics.health === "idle" ? "offline" : targetMetrics.health;
                      return (
                        <div key={target.id} className="row-wrap">
                          <div
                            className={`row row--target${bulk ? " row--bulk" : ""}`}
                            aria-current={selectedId === target.id ? "true" : undefined}
                          >
                            {bulk ? (
                              <label className="row-check">
                                <input
                                  type="checkbox"
                                  checked={chosen.includes(target.id)}
                                  onChange={() => toggleChosen(target.id)}
                                  aria-label={`Выбрать ${target.name}`}
                                />
                              </label>
                            ) : null}
                            <button
                              type="button"
                              className="row__hit"
                              onClick={() =>
                                bulk ? toggleChosen(target.id) : onSelect(target.id)
                              }
                            >
                              <span className="who">
                                <StatusDot health={health} />
                                <span>
                                  <span className="name">{target.name}</span>
                                  <span className="sub">
                                    {specLabel(target.spec)} · {target.interval_secs} с
                                  </span>
                                  {target.tags.length > 0 ? (
                                    <span className="row-tags">
                                      {target.tags.map((tag) => (
                                        <span
                                          key={tag}
                                          className={`tag tag--target tag--${tagTone(tag)}`}
                                        >
                                          {tag}
                                        </span>
                                      ))}
                                    </span>
                                  ) : null}
                                </span>
                              </span>
                              <AgentRail
                                target={target}
                                metrics={targetMetrics}
                                agents={agents}
                              />
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
                          </div>
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
                    })
                  : null}
              </div>
            ))}
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

      {groupManager ? (
        <TargetGroupsDialog
          groups={groups}
          targets={targets}
          onClose={() => setGroupManager(false)}
          onChanged={async () => {
            await onChanged();
          }}
          onError={(message) => setError(message)}
          error={error}
        />
      ) : null}
    </div>
  );
}
