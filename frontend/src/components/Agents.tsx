import { useState, type FormEvent } from "react";
import type { Agent, ProvisionResponse, Target } from "../types";
import { relativeTime } from "../metrics";
import { CopyButton, Dialog, StatusDot } from "./ui";
import type { ViewName } from "./Targets";

interface AgentsProps {
  agents: Agent[];
  targets: Target[];
  user: string;
  onNavigate: (view: ViewName) => void;
  onLogout: () => void;
  onCreate: (name: string) => Promise<ProvisionResponse>;
  onRotate: (agent: Agent) => Promise<ProvisionResponse>;
  onDelete: (agent: Agent) => Promise<void>;
  onProvision: (provision: ProvisionResponse) => void;
}

export function InstallDialog({
  provision,
  onClose,
}: {
  provision: ProvisionResponse;
  onClose: () => void;
}) {
  return (
    <Dialog
      title={`Установка агента ${provision.agent.name}`}
      subtitle={`токен действует до ${new Date(provision.expires_at).toLocaleTimeString("ru-RU", {
        hour: "2-digit",
        minute: "2-digit",
      })} · показывается один раз`}
      onClose={onClose}
      footer={
        <>
          <button type="button" className="btn btn--ghost" onClick={onClose}>
            Закрыть
          </button>
          <CopyButton text={provision.install_command} label="Скопировать команду" className="btn" />
        </>
      }
    >
      <p className="dialog-lead">
        Выполните на Linux-хосте под root. Установщик скачает бинарь для текущей архитектуры,
        проверит SHA-256 и заведёт systemd-службу.
      </p>
      <div className="cmd">
        <CopyButton text={provision.install_command} />
        <pre>{provision.install_command}</pre>
      </div>
      <p className="dialog-note">
        Постоянный credential сохранится в <b>/etc/pingushka/agent.toml</b> с правами 0600 и больше
        нигде не появится. Если команда потеряна — выпустите новый токен кнопкой «Ротация».
      </p>
    </Dialog>
  );
}

export function AgentsScreen({
  agents,
  targets,
  user,
  onNavigate,
  onLogout,
  onCreate,
  onRotate,
  onDelete,
  onProvision,
}: AgentsProps) {
  const [creating, setCreating] = useState(false);
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [toDelete, setToDelete] = useState<Agent | null>(null);
  const [rotatingId, setRotatingId] = useState<string | null>(null);

  const online = agents.filter((agent) => agent.connected).length;

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const provision = await onCreate(name.trim());
      setCreating(false);
      setName("");
      onProvision(provision);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Не удалось создать агента.");
    } finally {
      setBusy(false);
    }
  };

  const rotate = async (agent: Agent) => {
    setRotatingId(agent.id);
    try {
      const provision = await onRotate(agent);
      onProvision(provision);
    } finally {
      setRotatingId(null);
    }
  };

  const remove = async () => {
    if (!toDelete) return;
    setBusy(true);
    try {
      await onDelete(toDelete);
      setToDelete(null);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="screen">
      <header className="appbar">
        <div className="m-brand">
          <span className="mark" />
        </div>
        <div className="title-row">
          <h1 tabIndex={-1} data-dialog-fallback-focus>Агенты</h1>
          <span className="count">{agents.length}</span>
          <button
            type="button"
            className="add"
            aria-label="Добавить агента"
            onClick={() => setCreating(true)}
          >
            <span aria-hidden="true">+</span>
          </button>
        </div>
        <div className="appbar-tools">
          <button type="button" className="user" onClick={onLogout} title="Выйти">
            <i>{user.slice(0, 1).toLowerCase()}</i>
            {user}
          </button>
        </div>
      </header>

      <nav className="tabs" aria-label="Разделы">
        <button type="button" onClick={() => onNavigate("targets")}>
          Цели <span className="n">{targets.length}</span>
        </button>
        <button type="button" aria-current="page" onClick={() => onNavigate("agents")}>
          Агенты <span className="n">{agents.length}</span>
        </button>
        <button type="button" onClick={() => onNavigate("alerts")}>
          Оповещения
        </button>
      </nav>

      <div className="agent-note">
        <b>Установка по одной команде.</b> Токен одноразовый, действует 30 минут; Hub хранит только
        SHA-256 от постоянного credential. Сейчас онлайн {online} из {agents.length}.
      </div>

      <div className="workspace workspace--agents">
        <div className="listcol">
          <div className="cols cols--agents">
            <span>Агент</span>
            <span>Состояние</span>
            <span>Версия</span>
            <span>Цели</span>
            <span className="r">Последний сигнал</span>
            <span className="r">Действия</span>
          </div>
          <div className="rows">
            {agents.length === 0 ? (
              <div className="empty-state">
                <h3>Агентов пока нет</h3>
                <p>Создайте агента и установите его на Linux-хосте одной командой.</p>
                <button type="button" className="btn" onClick={() => setCreating(true)}>
                  Добавить агента
                </button>
              </div>
            ) : null}
            {agents.map((agent) => {
              const assigned = targets.filter((target) => target.agent_ids.includes(agent.id)).length;
              return (
                <div key={agent.id} className="row row--agent">
                  <span className="who">
                    <StatusDot health={agent.connected ? "ok" : "offline"} />
                    <span>
                      <span className="name">{agent.name}</span>
                      <span className="sub">
                        {agent.enrolled ? "credential выдан" : "ждёт установки"} ·{" "}
                        {agent.connected ? "ws установлен" : "нет соединения"}
                      </span>
                    </span>
                  </span>
                  <span className="agents">
                    <span>{agent.connected ? "online" : "offline"}</span>
                  </span>
                  <span className="ver">{agent.version ?? "—"}</span>
                  <span className="ver">{assigned}</span>
                  <span className="cred">{relativeTime(agent.last_seen_at)}</span>
                  <span className="row-actions">
                    <button
                      type="button"
                      className="link-button"
                      onClick={() => rotate(agent)}
                      disabled={rotatingId === agent.id}
                    >
                      {rotatingId === agent.id ? "Ротация…" : "Ротация"}
                    </button>
                    <button type="button" className="link-button danger" onClick={() => setToDelete(agent)}>
                      Удалить
                    </button>
                  </span>
                </div>
              );
            })}
          </div>
        </div>
      </div>

      {creating ? (
        <Dialog
          title="Новый агент"
          subtitle="имя видно только в Hub"
          onClose={() => setCreating(false)}
          footer={
            <>
              <button type="button" className="btn btn--ghost" onClick={() => setCreating(false)}>
                Отмена
              </button>
              <button type="submit" form="agent-form" className="btn" disabled={busy || !name.trim()}>
                {busy ? "Создаём…" : "Создать"}
              </button>
            </>
          }
        >
          <form id="agent-form" onSubmit={submit}>
            <div className="field">
              <label htmlFor="agent-name">Имя агента</label>
              <input
                id="agent-name"
                value={name}
                onChange={(event) => setName(event.target.value)}
                placeholder="msk-1"
                data-dialog-autofocus
                required
              />
              <p className="field-hint">Латинские буквы, цифры, дефис. Например, fra-2.</p>
            </div>
            {error ? (
              <p className="field-error" role="alert">
                <span aria-hidden="true">!</span> {error}
              </p>
            ) : null}
          </form>
        </Dialog>
      ) : null}

      {toDelete ? (
        <Dialog
          title={`Удалить агента ${toDelete.name}?`}
          subtitle="действие необратимо"
          alert
          descriptionId="agent-delete-description"
          onClose={() => setToDelete(null)}
          footer={
            <>
              <button type="button" className="btn btn--ghost" data-dialog-autofocus onClick={() => setToDelete(null)}>
                Отмена
              </button>
              <button type="button" className="btn btn--danger" aria-label={`Подтвердить удаление агента ${toDelete.name}`} onClick={remove} disabled={busy}>
                {busy ? "Удаляем…" : "Удалить агента"}
              </button>
            </>
          }
        >
          <p id="agent-delete-description" className="dialog-lead">
            Агент будет снят со всех целей, его соединение закроется. История проверок останется в базе.
          </p>
        </Dialog>
      ) : null}
    </div>
  );
}
