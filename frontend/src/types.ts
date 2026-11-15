export interface User {
  id: string;
  username: string;
}

export interface AuthState {
  setup_required: boolean;
}

export interface Health {
  status: string;
  version: string;
  uptime_seconds: number;
  database: string;
  protocol_version: number;
}

export interface Agent {
  id: string;
  name: string;
  connected: boolean;
  enrolled: boolean;
  last_seen_at: string | null;
  version: string | null;
}

export interface ProvisionResponse {
  agent: Agent;
  enrollment_token: string;
  expires_at: string;
  install_command: string;
}

export type TargetSpec =
  | { kind: "http"; url: string; method: "GET" | "HEAD"; expected_status: number | null }
  | { kind: "tcp"; host: string; port: number }
  | { kind: "icmp"; host: string };

export interface Target {
  id: string;
  name: string;
  spec: TargetSpec;
  interval_secs: number;
  timeout_ms: number;
  enabled: boolean;
  agent_ids: string[];
  created_at: string;
  updated_at: string;
}

export type CheckMode = "http" | "tcp" | "icmp" | "tcp_fallback";

export interface CheckResult {
  result_id: string;
  target_id: string;
  agent_id: string;
  checked_at: string;
  checked_at_unix: number;
  ok: boolean;
  latency_ms: number;
  status_code: number | null;
  mode: CheckMode;
  error_kind: string | null;
  error_message: string | null;
}

export interface Summary {
  total_checks: number;
  successful_checks: number;
  uptime_percent: number | null;
  average_latency_ms: number | null;
  p95_latency_ms: number | null;
  from: string;
  to: string;
}

export interface ApiErrorBody {
  error?: { code?: string; message?: string };
}
