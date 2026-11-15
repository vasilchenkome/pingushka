import type { Agent, CheckResult, Summary, Target, TargetSpec } from "./types";

export type Health = "ok" | "warn" | "fail" | "idle";

export interface AgentCheck {
  agentId: string;
  last: CheckResult | null;
  checks: number;
  successful: number;
  uptime: number | null;
  averageLatency: number | null;
}

export interface SparkPoint {
  value: number;
  failed: boolean;
}

export interface TargetMetrics {
  results: CheckResult[];
  total: number;
  successful: number;
  uptime: number | null;
  averageLatency: number | null;
  p95: number | null;
  maxLatency: number | null;
  health: Health;
  perAgent: Record<string, AgentCheck>;
  spark: SparkPoint[];
  lastCheckedAt: string | null;
}

export function sortResults(results: CheckResult[]): CheckResult[] {
  return [...results].sort((left, right) => left.checked_at_unix - right.checked_at_unix);
}

export function buildMetrics(
  target: Target,
  results: CheckResult[],
  summary?: Summary | null,
): TargetMetrics {
  const ordered = sortResults(results);
  const perAgent: Record<string, AgentCheck> = {};
  for (const agentId of target.agent_ids) {
    perAgent[agentId] = {
      agentId,
      last: null,
      checks: 0,
      successful: 0,
      uptime: null,
      averageLatency: null,
    };
  }

  let latencySum = 0;
  let latencyCount = 0;
  for (const result of ordered) {
    const entry = perAgent[result.agent_id] ?? {
      agentId: result.agent_id,
      last: null,
      checks: 0,
      successful: 0,
      uptime: null,
      averageLatency: null,
    };
    entry.checks += 1;
    if (result.ok) entry.successful += 1;
    latencySum += result.latency_ms;
    latencyCount += 1;
    entry.last = result;
    perAgent[result.agent_id] = entry;
  }

  for (const entry of Object.values(perAgent)) {
    entry.uptime = entry.checks === 0 ? null : (entry.successful * 100) / entry.checks;
    const latencies = ordered
      .filter((result) => result.agent_id === entry.agentId)
      .map((result) => result.latency_ms);
    entry.averageLatency = latencies.length === 0 ? null : average(latencies);
  }

  const latencies = ordered.map((result) => result.latency_ms).sort((left, right) => left - right);
  const total = ordered.length;
  const successful = ordered.filter((result) => result.ok).length;
  const latestByAgent = Object.values(perAgent)
    .map((entry) => entry.last)
    .filter((result): result is CheckResult => result !== null);
  const uptime = summary?.uptime_percent ?? (total === 0 ? null : (successful * 100) / total);
  const p95 =
    summary?.p95_latency_ms ?? (latencies.length === 0 ? null : percentile(latencies, 0.95));
  const maxLatency = latencies.length === 0 ? null : latencies[latencies.length - 1];
  const totalChecks = summary?.total_checks ?? total;

  let health: Health = "ok";
  if (totalChecks === 0) {
    health = "idle";
  } else if (latestByAgent.some((result) => !result.ok)) {
    health = "fail";
  } else if ((uptime !== null && uptime < 99.5) || (p95 !== null && p95 > 600)) {
    health = "warn";
  }

  return {
    results: ordered,
    total: totalChecks,
    successful,
    uptime,
    averageLatency: summary?.average_latency_ms ?? (latencyCount === 0 ? null : latencySum / latencyCount),
    p95,
    maxLatency,
    health,
    perAgent,
    spark: buildSpark(ordered),
    lastCheckedAt: ordered.length === 0 ? null : ordered[ordered.length - 1].checked_at,
  };
}

function average(values: number[]): number {
  return values.reduce((sum, value) => sum + value, 0) / values.length;
}

function percentile(sorted: number[], fraction: number): number {
  const index = Math.min(sorted.length - 1, Math.max(0, Math.ceil(sorted.length * fraction) - 1));
  return sorted[index];
}

function buildSpark(results: CheckResult[]): SparkPoint[] {
  const window = results.slice(-40);
  if (window.length === 0) return [];
  const max = Math.max(...window.map((result) => result.latency_ms), 1);
  return window.map((result) => ({
    value: Math.max(0.08, result.latency_ms / max),
    failed: !result.ok,
  }));
}

export function orderedAgentIds(target: Target, agents: Agent[]): string[] {
  const known = agents.filter((agent) => target.agent_ids.includes(agent.id)).map((agent) => agent.id);
  const unknown = target.agent_ids.filter((id) => !known.includes(id));
  return [...known, ...unknown];
}

export function agentChecks(target: Target, metrics: TargetMetrics, agents: Agent[]): AgentCheck[] {
  return orderedAgentIds(target, agents)
    .map((agentId) => {
      const known = agents.find((agent) => agent.id === agentId);
      const entry = metrics.perAgent[agentId];
      if (!entry) return null;
      return { ...entry, agentId: known?.id ?? agentId };
    })
    .filter((entry): entry is AgentCheck => entry !== null);
}

export function agentName(agents: Agent[], id: string): string {
  return agents.find((agent) => agent.id === id)?.name ?? id.slice(0, 8);
}

export function specLabel(spec: TargetSpec): string {
  switch (spec.kind) {
    case "http":
      return `HTTP ${spec.method}${spec.expected_status ? ` · ожидается ${spec.expected_status}` : ""}`;
    case "tcp":
      return `TCP · порт ${spec.port}`;
    case "icmp":
      return "ICMP · 4 пакета";
  }
}

export function specTarget(spec: TargetSpec): string {
  switch (spec.kind) {
    case "http":
      return spec.url;
    case "tcp":
      return `${spec.host}:${spec.port}`;
    case "icmp":
      return spec.host;
  }
}

export function formatNumber(value: number | null, digits = 0): string {
  if (value === null || Number.isNaN(value)) return "—";
  return new Intl.NumberFormat("ru-RU", {
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  }).format(value);
}

export function formatUptime(value: number | null): string {
  return value === null ? "—" : `${formatNumber(value, 2)} %`;
}

export function formatClock(iso: string | null): string {
  if (!iso) return "—";
  return new Date(iso).toLocaleTimeString("ru-RU", {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

export function relativeTime(iso: string | null): string {
  if (!iso) return "нет сигнала";
  const seconds = Math.max(0, Math.round((Date.now() - new Date(iso).getTime()) / 1000));
  if (seconds < 5) return "только что";
  if (seconds < 60) return `${seconds} с назад`;
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes} мин назад`;
  const hours = Math.round(minutes / 60);
  if (hours < 48) return `${hours} ч назад`;
  return `${Math.round(hours / 24)} дн назад`;
}

export function pluralAgents(count: number): string {
  const mod10 = count % 10;
  const mod100 = count % 100;
  if (mod10 === 1 && mod100 !== 11) return "агент";
  if (mod10 >= 2 && mod10 <= 4 && (mod100 < 12 || mod100 > 14)) return "агента";
  return "агентов";
}
