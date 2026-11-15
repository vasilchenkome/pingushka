import { useCallback, useEffect, useMemo, useState } from "react";
import { api, errorText, isUnauthorized } from "./api";
import { buildMetrics, orderedAgentIds, pluralAgents, type TargetMetrics } from "./metrics";
import type {
  Agent,
  CheckResult,
  ProvisionResponse,
  Summary,
  Target,
  TargetGroup,
  User,
} from "./types";
import { LoginScreen, SetupScreen } from "./components/Auth";
import { AgentsScreen, InstallDialog } from "./components/Agents";
import { AlertsScreen } from "./components/Alerts";
import { TargetsScreen, type StatusFilter, type ViewName } from "./components/Targets";
import { TargetDetail } from "./components/TargetDetail";
import { TargetForm } from "./components/TargetForm";
import { useMediaQuery } from "./hooks";

type Phase = "loading" | "setup" | "login" | "app";

export function App() {
  const [phase, setPhase] = useState<Phase>("loading");
  const [setupRequired, setSetupRequired] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [user, setUser] = useState<User | null>(null);
  const [hubVersion, setHubVersion] = useState("—");
  const [protocolVersion, setProtocolVersion] = useState<number | null>(null);

  const [agents, setAgents] = useState<Agent[]>([]);
  const [targets, setTargets] = useState<Target[]>([]);
  const [groups, setGroups] = useState<TargetGroup[]>([]);
  const [histories, setHistories] = useState<Record<string, CheckResult[]>>({});
  const [summaries, setSummaries] = useState<Record<string, Summary>>({});
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);

  const [view, setView] = useState<ViewName>("targets");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [status, setStatus] = useState<StatusFilter>("all");
  const [agentFilter, setAgentFilter] = useState("all");
  const [chartAgents, setChartAgents] = useState<string[]>([]);

  const [formTarget, setFormTarget] = useState<Target | null | undefined>(undefined);
  const [provision, setProvision] = useState<ProvisionResponse | null>(null);
  const [busyTargetId, setBusyTargetId] = useState<string | null>(null);
  const [toast, setToast] = useState<string | null>(null);

  const wide = useMediaQuery("(min-width: 1180px)");
  const phone = useMediaQuery("(max-width: 639px)");

  const handleApiError = useCallback((error: unknown) => {
    if (isUnauthorized(error)) {
      setPhase("login");
      setUser(null);
      return true;
    }
    return false;
  }, []);

  useEffect(() => {
    let cancelled = false;
    const boot = async () => {
      try {
        const health = await api.health();
        if (cancelled) return;
        setHubVersion(health.version);
        setProtocolVersion(health.protocol_version);
      } catch {
        // Health is best-effort; the auth flow below still works.
      }
      try {
        const state = await api.authState();
        if (cancelled) return;
        setSetupRequired(state.setup_required);
        if (state.setup_required) {
          setPhase("setup");
          return;
        }
        try {
          const me = await api.me();
          if (cancelled) return;
          setUser(me);
          setPhase("app");
        } catch (error) {
          if (cancelled) return;
          setPhase(isUnauthorized(error) ? "login" : "login");
        }
      } catch (error) {
        if (!cancelled) {
          setPhase("login");
          setNotice(errorText(error));
        }
      }
    };
    void boot();
    return () => {
      cancelled = true;
    };
  }, []);

  const refresh = useCallback(
    async (silent = false) => {
      if (!silent) setLoading(true);
      setLoadError(null);
      try {
        const [nextAgents, nextTargets, nextGroups] = await Promise.all([
          api.agents(),
          api.targets(),
          api.targetGroups(),
        ]);
        const [entries, summaryEntries] = await Promise.all([
          Promise.all(
            nextTargets.map(async (target) => {
              try {
                const results = await api.history(target.id, 600);
                return [target.id, results] as const;
              } catch (error) {
                if (isUnauthorized(error)) throw error;
                return [target.id, [] as CheckResult[]] as const;
              }
            }),
          ),
          Promise.all(
            nextTargets.map(async (target) => {
              try {
                return [target.id, await api.summary(target.id)] as const;
              } catch (error) {
                if (isUnauthorized(error)) throw error;
                return null;
              }
            }),
          ),
        ]);
        setAgents(nextAgents);
        setTargets(nextTargets);
        setGroups(nextGroups);
        setHistories(Object.fromEntries(entries));
        setSummaries(
          Object.fromEntries(
            summaryEntries.filter((entry) => entry !== null) as [string, Summary][],
          ),
        );
      } catch (error) {
        if (!handleApiError(error)) setLoadError(errorText(error));
      } finally {
        setLoading(false);
      }
    },
    [handleApiError],
  );

  useEffect(() => {
    if (phase !== "app") return;
    void refresh();
    const timer = window.setInterval(() => void refresh(true), 20_000);
    return () => window.clearInterval(timer);
  }, [phase, refresh]);

  const metrics = useMemo(() => {
    const result: Record<string, TargetMetrics> = {};
    for (const target of targets) {
      result[target.id] = buildMetrics(target, histories[target.id] ?? [], summaries[target.id]);
    }
    return result;
  }, [targets, histories, summaries]);

  const filteredTargets = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return targets.filter((target) => {
      if (needle) {
        const haystack = `${target.name} ${target.spec.kind === "http" ? target.spec.url : target.spec.host}`.toLowerCase();
        if (!haystack.includes(needle)) return false;
      }
      if (agentFilter !== "all" && !target.agent_ids.includes(agentFilter)) return false;
      const targetMetrics = metrics[target.id];
      if (!targetMetrics) return status === "all";
      if (status === "fail") return targetMetrics.health === "fail";
      if (status === "slow") {
        return targetMetrics.health === "warn" || (targetMetrics.p95 ?? 0) > 600;
      }
      return true;
    });
  }, [targets, metrics, query, status, agentFilter]);

  const selected = selectedId ? targets.find((target) => target.id === selectedId) ?? null : null;
  const selectedMetrics = selected ? metrics[selected.id] : undefined;

  useEffect(() => {
    if (!wide || selectedId || targets.length === 0) return;
    setSelectedId(targets[0].id);
  }, [wide, selectedId, targets]);

  useEffect(() => {
    if (!selected) {
      setChartAgents([]);
      return;
    }
    const ordered = orderedAgentIds(selected, agents);
    const chosen = [...ordered]
      .sort(
        (left, right) =>
          Number(agents.find((agent) => agent.id === right)?.connected ?? false) -
          Number(agents.find((agent) => agent.id === left)?.connected ?? false),
      )
      .slice(0, 2);
    setChartAgents(chosen);
  }, [selected?.id, agents.length]);

  useEffect(() => {
    if (!toast) return;
    const timer = window.setTimeout(() => setToast(null), 3200);
    return () => window.clearTimeout(timer);
  }, [toast]);

  if (phase === "loading") {
    return (
      <div className="boot">
        <span className="mark" />
        <span>pingushka</span>
      </div>
    );
  }

  if (phase === "setup") {
    return (
      <SetupScreen
        onSetup={async (username, password) => {
          await api.setup(username, password);
          setSetupRequired(false);
          setNotice("Администратор создан. Войдите с этим логином.");
          setPhase("login");
        }}
        onGoLogin={() => setPhase("login")}
      />
    );
  }

  if (phase === "login" || !user) {
    return (
      <LoginScreen
        setupRequired={setupRequired}
        notice={notice}
        hubVersion={hubVersion}
        protocolVersion={protocolVersion}
        onLogin={async (username, password) => {
          const result = await api.login(username, password);
          setUser(result.user);
          setNotice(null);
          setPhase("app");
        }}
        onGoSetup={() => setPhase("setup")}
      />
    );
  }

  const online = agents.filter((agent) => agent.connected).length;

  const logout = async () => {
    try {
      await api.logout();
    } finally {
      setUser(null);
      setPhase("login");
      setTargets([]);
      setAgents([]);
      setHistories({});
      setSummaries({});
      setSelectedId(null);
    }
  };

  const removeTarget = async (target: Target) => {
    try {
      await api.deleteTarget(target.id);
      setToast("Цель удалена.");
      if (selectedId === target.id) setSelectedId(null);
      await refresh(true);
    } catch (error) {
      if (!handleApiError(error)) setToast(errorText(error));
    }
  };

  const checkTarget = async (target: Target) => {
    setBusyTargetId(target.id);
    try {
      const result = await api.checkTarget(target.id);
      setToast(
        result.triggered > 0
          ? `Команда отправлена ${result.triggered} ${pluralAgents(result.triggered)}.`
          : "Нет подключённых агентов с этой целью.",
      );
      window.setTimeout(() => void refresh(true), 1800);
    } catch (error) {
      if (!handleApiError(error)) setToast(errorText(error));
    } finally {
      setBusyTargetId(null);
    }
  };

  const toggleChartAgent = (agentId: string) => {
    setChartAgents((current) => {
      if (current.includes(agentId)) {
        return current.length === 1 ? current : current.filter((id) => id !== agentId);
      }
      return [...current, agentId].slice(-3);
    });
  };

  const closePhoneDetail = () => setSelectedId(null);

  return (
    <>
      <div className={phone ? "app app--phone" : "app"}>
        <aside className="spine">
          <div className="brandline">
            <span className="mark" /> pingushka
          </div>
          <nav aria-label="Разделы">
            <button
              type="button"
              aria-current={view === "targets" ? "page" : undefined}
              onClick={() => {
                setView("targets");
                if (phone) setSelectedId(null);
              }}
            >
              Цели <span className="n">{targets.length}</span>
            </button>
            <button
              type="button"
              aria-current={view === "agents" ? "page" : undefined}
              onClick={() => {
                setView("agents");
                if (phone) setSelectedId(null);
              }}
            >
              Агенты <span className="n">{agents.length}</span>
            </button>
            <button
              type="button"
              aria-current={view === "alerts" ? "page" : undefined}
              onClick={() => {
                setView("alerts");
                if (phone) setSelectedId(null);
              }}
            >
              Оповещения
            </button>
          </nav>
          <div className="spine-foot">
            <div className="online">
              <i className="dot" /> {online} из {agents.length} онлайн
            </div>
            <div className="mono">hub {hubVersion} · protocol {protocolVersion ?? "—"}</div>
            <button type="button" className="link-button" onClick={logout}>
              Выйти
            </button>
          </div>
        </aside>

        <div className="app-main">
          {loadError ? (
            <div className="banner" role="alert">
              {loadError}
              <button type="button" className="link-button" onClick={() => void refresh()}>
                Повторить
              </button>
            </div>
          ) : null}

          {phone && selected && selectedMetrics ? (
            <div className="phone-detail">
              <TargetDetail
                target={selected}
                metrics={selectedMetrics}
                agents={agents}
                selectedAgentIds={chartAgents}
                onToggleAgent={toggleChartAgent}
                onCheck={() => void checkTarget(selected)}
                onEdit={() => setFormTarget(selected)}
                onDelete={() => void removeTarget(selected)}
                onClose={closePhoneDetail}
                busy={busyTargetId === selected.id}
              />
            </div>
          ) : view === "targets" ? (
            <TargetsScreen
              agents={agents}
              targets={filteredTargets}
              groups={groups}
              metrics={metrics}
              selectedId={selectedId}
              onSelect={(id) => {
                setSelectedId(id);
                if (id) setView("targets");
              }}
              onAdd={() => setFormTarget(null)}
              onEdit={(target) => setFormTarget(target)}
              onDelete={(target) => void removeTarget(target)}
              onCheck={(target) => void checkTarget(target)}
              onToggleAgent={toggleChartAgent}
              onNavigate={(next) => {
                setView(next);
                if (phone) setSelectedId(null);
              }}
              onLogout={() => void logout()}
              onChanged={() => void refresh(true)}
              selectedAgentIds={chartAgents}
              query={query}
              onQuery={setQuery}
              status={status}
              onStatus={setStatus}
              agentFilter={agentFilter}
              onAgentFilter={setAgentFilter}
              user={user.username}
              loading={loading}
              wide={wide}
              phone={phone}
              busyTargetId={busyTargetId}
            />
          ) : view === "agents" ? (
            <AgentsScreen
              agents={agents}
              targets={targets}
              user={user.username}
              onNavigate={setView}
              onLogout={() => void logout()}
              onCreate={async (name) => {
                const created = await api.createAgent(name);
                await refresh(true);
                return created;
              }}
              onRotate={async (agent) => {
                const rotated = await api.rotateAgent(agent.id);
                await refresh(true);
                return rotated;
              }}
              onDelete={async (agent) => {
                await api.deleteAgent(agent.id);
                await refresh(true);
                setToast("Агент удалён.");
              }}
              onProvision={setProvision}
            />
          ) : (
            <AlertsScreen
              targets={targets}
              groups={groups}
              user={user.username}
              onNavigate={(next) => {
                setView(next);
                if (phone) setSelectedId(null);
              }}
              onLogout={() => void logout()}
            />
          )}
        </div>
      </div>

      {formTarget !== undefined ? (
        <TargetForm
          target={formTarget}
          agents={agents}
          groups={groups}
          tagSuggestions={[...new Set(targets.flatMap((target) => target.tags))].sort()}
          onClose={() => setFormTarget(undefined)}
          onSaved={(saved, created) => {
            setFormTarget(undefined);
            setSelectedId(saved.id);
            setToast(created ? "Цель создана." : "Изменения сохранены.");
            void refresh(true);
          }}
        />
      ) : null}

      {provision ? <InstallDialog provision={provision} onClose={() => setProvision(null)} /> : null}

      {toast ? (
        <div className="toast" role="status">
          {toast}
        </div>
      ) : null}
    </>
  );
}
