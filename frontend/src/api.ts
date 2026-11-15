import type {
  Agent,
  AlertChannel,
  AlertChannelInput,
  AlertEvent,
  AlertRule,
  AlertRuleInput,
  ApiErrorBody,
  AuthState,
  BulkTargetInput,
  CheckResult,
  Health,
  ProvisionResponse,
  Summary,
  Target,
  TargetGroup,
  TargetSpec,
  User,
} from "./types";

export class ApiError extends Error {
  readonly status: number;
  readonly code: string;

  constructor(status: number, code: string, message: string) {
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.code = code;
  }
}

type RequestOptions = {
  method?: string;
  body?: unknown;
  signal?: AbortSignal;
};

async function request<T>(path: string, options: RequestOptions = {}): Promise<T> {
  const response = await fetch(path, {
    method: options.method ?? "GET",
    credentials: "include",
    signal: options.signal,
    headers: options.body === undefined ? undefined : { "Content-Type": "application/json" },
    body: options.body === undefined ? undefined : JSON.stringify(options.body),
  });

  if (!response.ok) {
    let code = "request_failed";
    let message = `Запрос завершился с кодом ${response.status}`;
    try {
      const body = (await response.json()) as ApiErrorBody;
      code = body.error?.code ?? code;
      message = body.error?.message ?? message;
    } catch {
      // Тело не JSON — оставляем сообщение по коду ответа.
    }
    throw new ApiError(response.status, code, message);
  }

  if (response.status === 204) return undefined as T;
  return (await response.json()) as T;
}

export const api = {
  health: () => request<Health>("/api/health"),
  authState: () => request<AuthState>("/api/auth/state"),
  me: () => request<User>("/api/auth/me"),
  setup: (username: string, password: string) =>
    request<User>("/api/setup", { method: "POST", body: { username, password } }),
  login: (username: string, password: string) =>
    request<{ user: User; expires_at: string }>("/api/auth/login", {
      method: "POST",
      body: { username, password },
    }),
  logout: () => request<void>("/api/auth/logout", { method: "POST" }),

  agents: () => request<Agent[]>("/api/agents"),
  createAgent: (name: string) =>
    request<ProvisionResponse>("/api/agents", { method: "POST", body: { name } }),
  rotateAgent: (id: string) =>
    request<ProvisionResponse>(`/api/agents/${id}/token`, { method: "POST" }),
  deleteAgent: (id: string) => request<void>(`/api/agents/${id}`, { method: "DELETE" }),

  targets: () => request<Target[]>("/api/targets"),
  targetGroups: () => request<TargetGroup[]>("/api/target-groups"),
  createTargetGroup: (name: string) =>
    request<TargetGroup>("/api/target-groups", { method: "POST", body: { name } }),
  renameTargetGroup: (id: string, name: string) =>
    request<TargetGroup>(`/api/target-groups/${id}`, { method: "PATCH", body: { name } }),
  deleteTargetGroup: (id: string) =>
    request<void>(`/api/target-groups/${id}`, { method: "DELETE" }),
  bulkUpdateTargets: (payload: BulkTargetInput) =>
    request<{ updated: number; group_id: string | null }>("/api/targets/bulk", {
      method: "PATCH",
      body: payload,
    }),
  createTarget: (payload: {
    name: string;
    spec: TargetSpec;
    interval_secs: number;
    timeout_ms: number;
    enabled: boolean;
    agent_ids: string[];
    group_id: string | null;
    tags: string[];
  }) => request<Target>("/api/targets", { method: "POST", body: payload }),
  updateTarget: (
    id: string,
    payload: Partial<{
      name: string;
      spec: TargetSpec;
      interval_secs: number;
      timeout_ms: number;
      enabled: boolean;
      agent_ids: string[];
      group_id: string | null;
      tags: string[];
    }>,
  ) => request<Target>(`/api/targets/${id}`, { method: "PATCH", body: payload }),
  deleteTarget: (id: string) => request<void>(`/api/targets/${id}`, { method: "DELETE" }),
  checkTarget: (id: string) =>
    request<{ triggered: number }>(`/api/targets/${id}/check`, { method: "POST" }),
  history: (id: string, limit = 600, agentId?: string, signal?: AbortSignal) =>
    request<CheckResult[]>(
      `/api/targets/${id}/results?limit=${limit}${agentId ? `&agent_id=${agentId}` : ""}`,
      { signal },
    ),
  summary: (targetId: string, agentId?: string, period: "24h" | "7d" = "24h") =>
    request<Summary>(
      `/api/summary?period=${period}&target_id=${targetId}${agentId ? `&agent_id=${agentId}` : ""}`,
    ),

  alertChannels: () => request<AlertChannel[]>("/api/alerts/channels"),
  createAlertChannel: (payload: AlertChannelInput) =>
    request<AlertChannel>("/api/alerts/channels", { method: "POST", body: payload }),
  updateAlertChannel: (id: string, payload: AlertChannelInput) =>
    request<AlertChannel>(`/api/alerts/channels/${id}`, { method: "PUT", body: payload }),
  deleteAlertChannel: (id: string) =>
    request<void>(`/api/alerts/channels/${id}`, { method: "DELETE" }),
  testAlertChannel: (id: string) =>
    request<{ ok: boolean; message: string }>(`/api/alerts/channels/${id}/test`, {
      method: "POST",
    }),
  alertRules: () => request<AlertRule[]>("/api/alerts/rules"),
  saveAlertRule: (payload: AlertRuleInput) =>
    request<AlertRule>("/api/alerts/rules", { method: "PUT", body: payload }),
  deleteAlertRule: (targetId: string) =>
    request<void>(`/api/alerts/rules/${targetId}`, { method: "DELETE" }),
  deleteAlertRuleById: (id: string) =>
    request<void>(`/api/alerts/rules/id/${id}`, { method: "DELETE" }),
  alertEvents: (limit = 100) =>
    request<AlertEvent[]>(`/api/alerts/events?limit=${limit}`),
  retryAlertEvent: (id: string) =>
    request<{ retried: boolean }>(`/api/alerts/events/${id}/retry`, { method: "POST" }),
};

export function isUnauthorized(error: unknown): boolean {
  return error instanceof ApiError && error.status === 401;
}

export function errorText(error: unknown): string {
  if (error instanceof ApiError) return error.message;
  if (error instanceof Error) return error.message;
  return "Не удалось выполнить запрос.";
}
