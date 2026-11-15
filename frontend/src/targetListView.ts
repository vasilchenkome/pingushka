import type { Target } from "./types";
import type { TargetMetrics } from "./metrics";

export type SortKey = "status" | "name" | "uptime" | "p95" | "checked" | "group";
export type SortDirection = "asc" | "desc";

export interface TargetGroupView {
  key: string;
  name: string;
  targets: Target[];
  failed: number;
  health: "fail" | "warn" | "ok" | "offline";
}

const HEALTH_RANK: Record<string, number> = { fail: 0, warn: 1, ok: 2, offline: 3 };

export function targetHealth(metrics: TargetMetrics | undefined): string {
  if (!metrics || metrics.health === "idle") return "offline";
  return metrics.health;
}

export function filterByTags(targets: Target[], tags: string[]): Target[] {
  if (tags.length === 0) return targets;
  return targets.filter((target) => tags.every((tag) => target.tags.includes(tag)));
}

export function sortTargets(
  targets: Target[],
  metrics: Record<string, TargetMetrics>,
  key: SortKey,
  direction: SortDirection,
): Target[] {
  const sign = direction === "asc" ? 1 : -1;
  return [...targets].sort((left, right) => {
    const leftMetrics = metrics[left.id];
    const rightMetrics = metrics[right.id];
    let result = 0;
    if (key === "status") {
      result =
        (HEALTH_RANK[targetHealth(leftMetrics)] ?? 3) -
        (HEALTH_RANK[targetHealth(rightMetrics)] ?? 3);
    } else if (key === "uptime") {
      result = (leftMetrics?.uptime ?? -1) - (rightMetrics?.uptime ?? -1);
    } else if (key === "p95") {
      result = (leftMetrics?.p95 ?? Number.MAX_SAFE_INTEGER) - (rightMetrics?.p95 ?? Number.MAX_SAFE_INTEGER);
    } else if (key === "checked") {
      result =
        Date.parse(leftMetrics?.lastCheckedAt ?? "") - Date.parse(rightMetrics?.lastCheckedAt ?? "");
      if (Number.isNaN(result)) result = 0;
    } else if (key === "group") {
      result = (left.group?.name ?? "\uffff").localeCompare(right.group?.name ?? "\uffff", "ru");
    }
    if (result === 0) result = left.name.localeCompare(right.name, "ru");
    return result * sign;
  });
}

export function groupTargets(
  targets: Target[],
  metrics: Record<string, TargetMetrics>,
  direction: SortDirection,
): TargetGroupView[] {
  const buckets = new Map<string, TargetGroupView>();
  for (const target of targets) {
    const key = target.group?.id ?? "";
    const name = target.group?.name ?? "Без группы";
    const bucket = buckets.get(key) ?? {
      key,
      name,
      targets: [],
      failed: 0,
      health: "ok" as const,
    };
    bucket.targets.push(target);
    if (targetHealth(metrics[target.id]) === "fail") bucket.failed += 1;
    buckets.set(key, bucket);
  }
  return [...buckets.values()]
    .map((bucket) => {
      const ranks = bucket.targets.map((target) => HEALTH_RANK[targetHealth(metrics[target.id])] ?? 3);
      const worst = Math.min(...ranks);
      const health =
        worst === 0 ? "fail" : worst === 1 ? "warn" : worst === 3 ? "offline" : "ok";
      return { ...bucket, health } as TargetGroupView;
    })
    .sort((left, right) => {
      if (left.key === "" && right.key !== "") return 1;
      if (right.key === "" && left.key !== "") return -1;
      const result = left.name.localeCompare(right.name, "ru");
      return direction === "asc" ? result : -result;
    });
}
