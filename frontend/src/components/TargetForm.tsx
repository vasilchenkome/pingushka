import { useState, type FormEvent } from "react";
import type { Agent, Target, TargetSpec } from "../types";
import { api, errorText } from "../api";
import { Dialog } from "./ui";

interface TargetFormProps {
  target: Target | null;
  agents: Agent[];
  onClose: () => void;
  onSaved: (target: Target, created: boolean) => void;
}

type Kind = TargetSpec["kind"];

export function TargetForm({ target, agents, onClose, onSaved }: TargetFormProps) {
  const initialKind: Kind = target?.spec.kind ?? "http";
  const [kind, setKind] = useState<Kind>(initialKind);
  const [name, setName] = useState(target?.name ?? "");
  const [url, setUrl] = useState(target?.spec.kind === "http" ? target.spec.url : "https://");
  const [method, setMethod] = useState<"GET" | "HEAD">(
    target?.spec.kind === "http" ? target.spec.method : "GET",
  );
  const [expected, setExpected] = useState(
    target?.spec.kind === "http" && target.spec.expected_status
      ? String(target.spec.expected_status)
      : "",
  );
  const [host, setHost] = useState(
    target?.spec.kind === "tcp" || target?.spec.kind === "icmp" ? target.spec.host : "",
  );
  const [port, setPort] = useState(
    target?.spec.kind === "tcp" ? String(target.spec.port) : "443",
  );
  const [interval, setInterval] = useState(String(target?.interval_secs ?? 60));
  const [timeout, setTimeout] = useState(String(target?.timeout_ms ?? 5000));
  const [enabled, setEnabled] = useState(target?.enabled ?? true);
  const [assigned, setAssigned] = useState<string[]>(target?.agent_ids ?? []);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const toggleAgent = (id: string) => {
    setAssigned((current) =>
      current.includes(id) ? current.filter((entry) => entry !== id) : [...current, id],
    );
  };

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setBusy(true);
    setError(null);

    let spec: TargetSpec;
    if (kind === "http") {
      spec = {
        kind: "http",
        url: url.trim(),
        method,
        expected_status: expected.trim() === "" ? null : Number(expected),
      };
    } else if (kind === "tcp") {
      spec = { kind: "tcp", host: host.trim(), port: Number(port) };
    } else {
      spec = { kind: "icmp", host: host.trim() };
    }

    const payload = {
      name: name.trim(),
      spec,
      interval_secs: Number(interval),
      timeout_ms: Number(timeout),
      enabled,
      agent_ids: assigned,
    };

    try {
      const saved = target
        ? await api.updateTarget(target.id, payload)
        : await api.createTarget(payload);
      onSaved(saved, target === null);
    } catch (cause) {
      setError(errorText(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      title={target ? `Изменить ${target.name}` : "Новая цель"}
      subtitle="проверка выполняется назначенными агентами по расписанию"
      onClose={onClose}
      wide
      footer={
        <>
          <button type="button" className="btn btn--ghost" onClick={onClose}>
            Отмена
          </button>
          <button type="submit" form="target-form" className="btn" disabled={busy}>
            {busy ? "Сохраняем…" : target ? "Сохранить" : "Создать цель"}
          </button>
        </>
      }
    >
      <form id="target-form" className="form-grid" onSubmit={submit}>
        <div className="field field--wide">
          <label htmlFor="target-name">Название</label>
          <input
            id="target-name"
            value={name}
            onChange={(event) => setName(event.target.value)}
            placeholder="api.example.com"
            autoFocus
            required
          />
        </div>

        <div className="field">
          <label htmlFor="target-kind">Тип проверки</label>
          <select
            id="target-kind"
            value={kind}
            onChange={(event) => setKind(event.target.value as Kind)}
          >
            <option value="http">HTTP(S)</option>
            <option value="tcp">TCP-порт</option>
            <option value="icmp">ICMP</option>
          </select>
        </div>

        {kind === "http" ? (
          <>
            <div className="field field--wide">
              <label htmlFor="target-url">URL</label>
              <input
                id="target-url"
                value={url}
                onChange={(event) => setUrl(event.target.value)}
                placeholder="https://example.com/health"
                required
              />
            </div>
            <div className="field">
              <label htmlFor="target-method">Метод</label>
              <select
                id="target-method"
                value={method}
                onChange={(event) => setMethod(event.target.value as "GET" | "HEAD")}
              >
                <option value="GET">GET</option>
                <option value="HEAD">HEAD</option>
              </select>
            </div>
            <div className="field">
              <label htmlFor="target-status">Ожидаемый код</label>
              <input
                id="target-status"
                inputMode="numeric"
                value={expected}
                onChange={(event) => setExpected(event.target.value.replace(/[^0-9]/g, ""))}
                placeholder="200"
              />
              <p className="field-hint">Пусто — любой код 2xx–3xx считается успехом.</p>
            </div>
          </>
        ) : (
          <>
            <div className="field">
              <label htmlFor="target-host">Хост</label>
              <input
                id="target-host"
                value={host}
                onChange={(event) => setHost(event.target.value)}
                placeholder="db-01.internal"
                required
              />
            </div>
            {kind === "tcp" ? (
              <div className="field">
                <label htmlFor="target-port">Порт</label>
                <input
                  id="target-port"
                  inputMode="numeric"
                  value={port}
                  onChange={(event) => setPort(event.target.value.replace(/[^0-9]/g, ""))}
                  required
                />
              </div>
            ) : null}
          </>
        )}

        <div className="field">
          <label htmlFor="target-interval">Интервал, секунды</label>
          <input
            id="target-interval"
            inputMode="numeric"
            value={interval}
            onChange={(event) => setInterval(event.target.value.replace(/[^0-9]/g, ""))}
            required
          />
          <p className="field-hint">От 5 до 3600.</p>
        </div>
        <div className="field">
          <label htmlFor="target-timeout">Таймаут, мс</label>
          <input
            id="target-timeout"
            inputMode="numeric"
            value={timeout}
            onChange={(event) => setTimeout(event.target.value.replace(/[^0-9]/g, ""))}
            required
          />
          <p className="field-hint">От 100 до 30 000.</p>
        </div>

        <div className="field field--wide">
          <span className="label">Агенты</span>
          <div className="agent-choices">
            {agents.length === 0 ? (
              <p className="field-hint">Сначала создайте хотя бы одного агента.</p>
            ) : null}
            {agents.map((agent) => (
              <label key={agent.id} className="choice">
                <input
                  type="checkbox"
                  checked={assigned.includes(agent.id)}
                  onChange={() => toggleAgent(agent.id)}
                />
                <span>
                  {agent.name}
                  {!agent.connected ? <em>offline</em> : null}
                </span>
              </label>
            ))}
          </div>
          {agents.length > 0 && assigned.length === 0 ? (
            <p className="field-hint">Без агентов цель не будет проверяться.</p>
          ) : null}
        </div>

        <label className="choice choice--wide">
          <input
            type="checkbox"
            checked={enabled}
            onChange={(event) => setEnabled(event.target.checked)}
          />
          <span>Цель включена</span>
        </label>

        {error ? (
          <p className="field-error field--wide" role="alert">
            <span aria-hidden="true">!</span> {error}
          </p>
        ) : null}
      </form>
    </Dialog>
  );
}
