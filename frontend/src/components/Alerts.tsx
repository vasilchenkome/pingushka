import { useEffect, useMemo, useState, type FormEvent } from "react";
import { api, errorText } from "../api";
import { relativeTime } from "../metrics";
import type {
  AlertChannel,
  AlertChannelInput,
  AlertChannelKind,
  AlertEvent,
  AlertRule,
  Target,
  TargetGroup,
} from "../types";
import type { ViewName } from "./Targets";
import { Dialog, StatusDot } from "./ui";

interface AlertsScreenProps {
  targets: Target[];
  groups: TargetGroup[];
  user: string;
  onNavigate: (view: ViewName) => void;
  onLogout: () => void;
}

type Tab = "rules" | "channels" | "journal";

const KIND_LABEL: Record<AlertChannelKind, string> = {
  telegram: "Telegram",
  webhook: "Webhook",
  email: "Email",
};

const STATUS_LABEL: Record<AlertRule["state"]["status"], string> = {
  ok: "норма",
  pending: "ожидание",
  firing: "сбой",
};

function defaultConfig(kind: AlertChannelKind): Record<string, unknown> {
  if (kind === "telegram") return { bot_token: "", chat_id: "" };
  if (kind === "webhook") return { url: "", secret: "" };
  return { host: "", port: 587, from: "", to: "", username: "", password: "", tls: "starttls" };
}

function emptyRule(channels: AlertChannel[]): AlertRule {
  return {
    id: "",
    scope: "target",
    target_id: null,
    group_id: null,
    target_name: null,
    target_kind: null,
    target_ref: null,
    group_name: null,
    enabled: true,
    mode: "all",
    failure_threshold: 3,
    recovery_threshold: 1,
    cooldown_secs: 900,
    channel_ids: channels.filter((channel) => channel.enabled).map((channel) => channel.id),
    state: {
      status: "ok",
      consecutive_failures: 0,
      consecutive_successes: 0,
      last_result_at: null,
      last_notified_at: null,
      opened_at: null,
    },
  };
}

export function AlertsScreen({ targets, groups, user, onNavigate, onLogout }: AlertsScreenProps) {
  const [tab, setTab] = useState<Tab>("rules");
  const [channels, setChannels] = useState<AlertChannel[]>([]);
  const [rules, setRules] = useState<AlertRule[]>([]);
  const [events, setEvents] = useState<AlertEvent[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [ruleForm, setRuleForm] = useState<{
    target: Target | null;
    group: TargetGroup | null;
    rule: AlertRule | null;
  } | null>(null);
  const [channelForm, setChannelForm] = useState<{ channel: AlertChannel | null } | null>(null);
  const [pendingDelete, setPendingDelete] = useState<
    { kind: "rule"; rule: AlertRule } | { kind: "channel"; channel: AlertChannel } | null
  >(null);
  const [confirmError, setConfirmError] = useState<string | null>(null);

  const load = async (silent = false) => {
    if (!silent) setLoading(true);
    try {
      const [nextChannels, nextRules, nextEvents] = await Promise.all([
        api.alertChannels(),
        api.alertRules(),
        api.alertEvents(100),
      ]);
      setChannels(nextChannels);
      setRules(nextRules);
      setEvents(nextEvents);
      setError(null);
    } catch (cause) {
      setError(errorText(cause));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void load();
    const timer = window.setInterval(() => void load(true), 15_000);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    if (!toast) return;
    const timer = window.setTimeout(() => setToast(null), 3200);
    return () => window.clearTimeout(timer);
  }, [toast]);

  const rulesByTarget = useMemo(
    () =>
      new Map(
        rules
          .filter((rule) => rule.target_id)
          .map((rule) => [rule.target_id as string, rule]),
      ),
    [rules],
  );
  const rulesByGroup = useMemo(
    () =>
      new Map(
        rules.filter((rule) => rule.group_id).map((rule) => [rule.group_id as string, rule]),
      ),
    [rules],
  );
  const channelName = (id: string) => channels.find((channel) => channel.id === id)?.name ?? "канал";
  const unconfigured = targets.filter((target) => !rulesByTarget.has(target.id));

  const openRule = (target: Target) => {
    setRuleForm({ target, group: null, rule: rulesByTarget.get(target.id) ?? null });
  };
  const openGroupRule = (group: TargetGroup) => {
    setRuleForm({ target: null, group, rule: rulesByGroup.get(group.id) ?? null });
  };
  const openNewRule = () => setRuleForm({ target: null, group: null, rule: null });

  const requestRuleDelete = (rule: AlertRule) => {
    if (busyId !== null) return;
    setConfirmError(null);
    setPendingDelete({ kind: "rule", rule });
  };

  const testChannel = async (channel: AlertChannel) => {
    setBusyId(channel.id);
    try {
      const result = await api.testAlertChannel(channel.id);
      setToast(result.ok ? `Тест отправлен: ${channel.name}.` : result.message);
    } catch (cause) {
      setToast(errorText(cause));
    } finally {
      setBusyId(null);
    }
  };

  const requestChannelDelete = (channel: AlertChannel) => {
    if (busyId !== null) return;
    setConfirmError(null);
    setPendingDelete({ kind: "channel", channel });
  };

  const confirmDelete = async () => {
    if (!pendingDelete) return;
    const id = pendingDelete.kind === "rule" ? pendingDelete.rule.id : pendingDelete.channel.id;
    setBusyId(id);
    setConfirmError(null);
    try {
      if (pendingDelete.kind === "rule") {
        await api.deleteAlertRuleById(pendingDelete.rule.id);
        setToast("Правило удалено.");
      } else {
        await api.deleteAlertChannel(pendingDelete.channel.id);
        setToast("Канал удалён.");
      }
      await load(true);
      setPendingDelete(null);
    } catch (cause) {
      setConfirmError(errorText(cause));
    } finally {
      setBusyId(null);
    }
  };

  const retry = async (event: AlertEvent) => {
    setBusyId(event.id);
    try {
      await api.retryAlertEvent(event.id);
      setToast("Событие возвращено в очередь.");
      await load(true);
    } catch (cause) {
      setToast(errorText(cause));
    } finally {
      setBusyId(null);
    }
  };

  const closeConfirmation = () => {
    if (busyId !== null) return;
    setPendingDelete(null);
    setConfirmError(null);
  };

  return (
    <div className="screen alerts-screen">
      <header className="appbar">
        <div className="m-brand">
          <span className="mark" />
        </div>
        <div className="title-row">
          <h1 tabIndex={-1} data-dialog-fallback-focus>Оповещения</h1>
          <span className="count">{rules.length}</span>
          <button
            type="button"
            className="add"
            aria-label={tab === "channels" ? "Добавить канал" : "Добавить правило"}
            onClick={() => {
              if (tab === "channels") {
                setChannelForm({ channel: null });
                return;
              }
              if (tab === "journal") {
                setTab("rules");
              }
              const target = unconfigured[0] ?? targets[0];
              if (target) openRule(target);
            }}
          >
            <span aria-hidden="true">+</span>
          </button>
        </div>
        <div className="appbar-tools">
          <span className="toolbar-note">
            один down и один recovered на инцидент · cooldown защищает от флаппинга
          </span>
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
        <button type="button" onClick={() => onNavigate("agents")}>
          Агенты
        </button>
        <button type="button" aria-current="page" onClick={() => onNavigate("alerts")}>
          Оповещения <span className="n">{rules.length}</span>
        </button>
      </nav>

      <div className="alert-tabs" role="tablist" aria-label="Раздел оповещений">
        <button type="button" role="tab" aria-selected={tab === "rules"} onClick={() => setTab("rules")}>
          Правила <span className="n">{rules.length}</span>
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={tab === "channels"}
          onClick={() => setTab("channels")}
        >
          Каналы <span className="n">{channels.length}</span>
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={tab === "journal"}
          onClick={() => setTab("journal")}
        >
          Журнал <span className="n">{events.length}</span>
        </button>
      </div>

      {error ? (
        <div className="banner" role="alert">
          {error}
          <button type="button" className="link-button" onClick={() => void load()}>
            Повторить
          </button>
        </div>
      ) : null}

      <div className="alert-body">
        {tab === "rules" ? (
          <section className="alert-panel" aria-label="Правила оповещений">
            <div className="panel-head">
              <div>
                <h2>Правила</h2>
                <p>Сбой фиксируется, когда условие выполняется нужное число проверок подряд.</p>
              </div>
              <button type="button" className="btn btn--ghost" onClick={openNewRule}>
                Создать правило
              </button>
            </div>
            {loading ? <div className="empty">Загрузка…</div> : null}
            {!loading && rules.length === 0 ? (
              <div className="empty">
                <b>Правил пока нет</b>
                <span>Выберите цель и канал — первый алерт уйдёт после третьего сбоя подряд.</span>
                <button type="button" className="btn" disabled={targets.length === 0} onClick={() => {
                  if (targets[0]) openRule(targets[0]);
                }}>
                  Создать правило
                </button>
              </div>
            ) : null}
            <div className="rule-list">
              {rules.map((rule) => (
                <article key={rule.id} className={`rule-card rule-card--${rule.state.status}`}>
                  <div className="rule-main">
                    <div className="rule-title">
                      <StatusDot health={rule.state.status === "firing" ? "fail" : rule.state.status === "pending" ? "warn" : "ok"} />
                      <b>{rule.target_name ?? rule.group_name}</b>
                            <span className="rule-ref">
                              {rule.scope === "group"
                                ? `группа · ${rule.group_name ?? ""}`
                                : rule.target_ref}
                            </span>
                      {!rule.enabled ? <span className="tag">выключено</span> : null}
                    </div>
                    <div className="rule-meta">
                      <span>{STATUS_LABEL[rule.state.status]}</span>
                      <span>·</span>
                      <span>{rule.mode === "all" ? "все агенты" : "любой агент"}</span>
                      <span>·</span>
                      <span>{rule.failure_threshold} сбоя подряд</span>
                      <span>·</span>
                      <span>cooldown {Math.round(rule.cooldown_secs / 60)} мин</span>
                      {rule.state.last_result_at ? (
                        <>
                          <span>·</span>
                          <span>проверка {relativeTime(rule.state.last_result_at)}</span>
                        </>
                      ) : null}
                    </div>
                    <div className="rule-channels">
                      {rule.channel_ids.length === 0 ? (
                        <span className="muted">каналы не выбраны</span>
                      ) : (
                        rule.channel_ids.map((id) => (
                          <span key={id} className="tag tag--signal">
                            {channelName(id)}
                          </span>
                        ))
                      )}
                    </div>
                  </div>
                  <div className="rule-actions">
                    <button type="button" className="btn btn--ghost" aria-label={`Изменить правило для ${rule.target_name ?? rule.group_name ?? "цель"}`} onClick={() => {
                      if (rule.scope === "group") {
                        const group = groups.find((entry) => entry.id === rule.group_id);
                        if (group) openGroupRule(group);
                      } else {
                        const target = targets.find((entry) => entry.id === rule.target_id);
                        if (target) openRule(target);
                      }
                    }}>
                      Изменить
                    </button>
                    <button type="button" className="link-button danger" aria-label={`Удалить правило для ${rule.target_name ?? rule.group_name ?? "цели"}`} disabled={busyId !== null} onClick={() => requestRuleDelete(rule)}>
                      Удалить
                    </button>
                  </div>
                </article>
              ))}
            </div>
          </section>
        ) : null}

        {tab === "channels" ? (
          <section className="alert-panel" aria-label="Каналы доставки">
            <div className="panel-head">
              <div>
                <h2>Каналы</h2>
                <p>Секреты шифруются ключом ALERT_SECRET_KEY и не возвращаются в API.</p>
              </div>
              <button type="button" className="btn btn--ghost" onClick={() => setChannelForm({ channel: null })}>
                Добавить канал
              </button>
            </div>
            {channels.length === 0 ? (
              <div className="empty">
                <b>Каналов пока нет</b>
                <span>Telegram, webhook или SMTP-письмо. Начните с одного — правило можно привязать позже.</span>
                <button type="button" className="btn" onClick={() => setChannelForm({ channel: null })}>
                  Добавить канал
                </button>
              </div>
            ) : null}
            <div className="channel-grid">
              {channels.map((channel) => (
                <article key={channel.id} className="channel-card">
                  <div className="channel-head">
                    <span className={`channel-kind channel-kind--${channel.kind}`}>{KIND_LABEL[channel.kind]}</span>
                    <StatusDot health={channel.enabled ? "ok" : "idle"} />
                  </div>
                  <b>{channel.name}</b>
                  <span className="muted">{channel.enabled ? "включён" : "выключен"} · {relativeTime(channel.updated_at)}</span>
                  <div className="channel-actions">
                    <button type="button" className="btn btn--ghost" aria-label={`Отправить тест в канал ${channel.name}`} disabled={busyId === channel.id} onClick={() => void testChannel(channel)}>
                      Тест
                    </button>
                    <button type="button" className="btn btn--ghost" aria-label={`Изменить канал ${channel.name}`} onClick={() => setChannelForm({ channel })}>
                      Изменить
                    </button>
                    <button type="button" className="link-button danger" aria-label={`Удалить канал ${channel.name}`} disabled={busyId !== null} onClick={() => requestChannelDelete(channel)}>
                      Удалить
                    </button>
                  </div>
                </article>
              ))}
            </div>
          </section>
        ) : null}

        {tab === "journal" ? (
          <section className="alert-panel" aria-label="Журнал доставки">
            <div className="panel-head">
              <div>
                <h2>Журнал</h2>
                <p>Последние 100 событий. Неудачные можно вернуть в очередь вручную.</p>
              </div>
              <button type="button" className="btn btn--ghost" onClick={() => void load(true)}>
                Обновить
              </button>
            </div>
            {events.length === 0 ? (
              <div className="empty">
                <b>Событий пока нет</b>
                <span>Здесь появятся down и recovered после срабатывания правил.</span>
              </div>
            ) : (
              <div className="journal">
                <div className="journal-row journal-row--head">
                  <span>Событие</span>
                  <span>Цель</span>
                  <span>Канал</span>
                  <span>Статус</span>
                  <span>Когда</span>
                  <span />
                </div>
                {events.map((event) => (
                  <div key={event.id} className="journal-row">
                    <span className={event.event === "down" ? "event event--down" : "event event--up"}>
                      {event.event === "down" ? "DOWN" : "RECOVERED"}
                    </span>
                    <span>{event.target_name ?? event.group_name ?? "—"}</span>
                    <span>{event.channel_name}</span>
                    <span className={`delivery delivery--${event.status}`} title={event.last_error ?? undefined}>
                      {event.status === "sent" ? "отправлено" : event.status === "failed" ? "ошибка" : event.status === "sending" ? "отправка" : "в очереди"}
                      {event.attempts > 0 ? ` · ${event.attempts}` : ""}
                    </span>
                    <span className="muted">{relativeTime(event.created_at)}</span>
                    <span>
                      {event.status === "failed" ? (
                        <button type="button" className="link-button" aria-label={`Повторить доставку для ${event.target_name ?? event.group_name ?? "цели"} через ${event.channel_name}`} disabled={busyId === event.id} onClick={() => void retry(event)}>
                          Повторить
                        </button>
                      ) : null}
                    </span>
                  </div>
                ))}
              </div>
            )}
          </section>
        ) : null}
      </div>

      {ruleForm ? (
        <RuleDialog
          target={ruleForm.target}
          group={ruleForm.group}
          targets={targets}
          groups={groups}
          rule={ruleForm.rule ?? emptyRule(channels)}
          editing={ruleForm.rule !== null}
          channels={channels}
          onClose={() => setRuleForm(null)}
          onSaved={async () => {
            setRuleForm(null);
            setToast("Правило сохранено.");
            await load(true);
          }}
          onError={(message) => setToast(message)}
        />
      ) : null}

      {channelForm ? (
        <ChannelDialog
          channel={channelForm.channel}
          onClose={() => setChannelForm(null)}
          onSaved={async () => {
            setChannelForm(null);
            setToast("Канал сохранён.");
            await load(true);
          }}
          onError={(message) => setToast(message)}
        />
      ) : null}

      {pendingDelete ? (
        <Dialog
          title={
            pendingDelete.kind === "rule"
              ? `Удалить правило для «${pendingDelete.rule.target_name ?? pendingDelete.rule.group_name ?? "правило"}»?`
              : `Удалить канал «${pendingDelete.channel.name}»?`
          }
          subtitle="Действие нельзя отменить"
          alert
          descriptionId="delete-confirm-description"
          onClose={closeConfirmation}
          footer={
            <>
              <button
                type="button"
                className="btn btn--ghost"
                data-dialog-autofocus
                onClick={closeConfirmation}
                disabled={busyId !== null}
              >
                Отмена
              </button>
              <button
                type="button"
                className="btn btn--danger"
                onClick={() => void confirmDelete()}
                disabled={busyId !== null}
              >
                {busyId !== null
                  ? "Удаляем…"
                  : pendingDelete.kind === "rule"
                    ? "Удалить правило"
                    : "Удалить канал"}
              </button>
            </>
          }
        >
          <p id="delete-confirm-description" className="dialog-lead">
            {pendingDelete.kind === "rule"
              ? "Уведомления по этому правилу будут остановлены."
              : `Правила, где используется канал «${pendingDelete.channel.name}», потеряют этот канал.`}
          </p>
          {confirmError ? <p className="form-error" role="alert">{confirmError}</p> : null}
        </Dialog>
      ) : null}

      {toast ? (
        <div className="toast" role="status">
          {toast}
        </div>
      ) : null}
    </div>
  );
}

function RuleDialog({
  target,
  group,
  targets,
  groups,
  rule,
  editing,
  channels,
  onClose,
  onSaved,
  onError,
}: {
  target: Target | null;
  group: TargetGroup | null;
  targets: Target[];
  groups: TargetGroup[];
  rule: AlertRule;
  editing: boolean;
  channels: AlertChannel[];
  onClose: () => void;
  onSaved: () => Promise<void>;
  onError: (message: string) => void;
}) {
  const [scope, setScope] = useState<"target" | "group">(
    group ? "group" : rule.scope,
  );
  const [targetId, setTargetId] = useState(target?.id ?? rule.target_id ?? "");
  const [groupId, setGroupId] = useState(group?.id ?? rule.group_id ?? "");
  const [enabled, setEnabled] = useState(rule.enabled);
  const [mode, setMode] = useState(rule.mode);
  const [failure, setFailure] = useState(rule.failure_threshold);
  const [recovery, setRecovery] = useState(rule.recovery_threshold);
  const [cooldown, setCooldown] = useState(Math.round(rule.cooldown_secs / 60));
  const [selected, setSelected] = useState<string[]>(rule.channel_ids);
  const [busy, setBusy] = useState(false);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (scope === "target" && targetId === "") {
      onError("Выберите цель для правила.");
      return;
    }
    if (scope === "group" && groupId === "") {
      onError("Выберите группу для правила.");
      return;
    }
    setBusy(true);
    try {
      await api.saveAlertRule({
        ...(scope === "target" ? { target_id: targetId } : { group_id: groupId }),
        enabled,
        mode,
        failure_threshold: failure,
        recovery_threshold: recovery,
        cooldown_secs: cooldown * 60,
        channel_ids: selected,
      });
      await onSaved();
    } catch (cause) {
      onError(errorText(cause));
    } finally {
      setBusy(false);
    }
  };

  const title =
    scope === "group"
      ? `Правило · ${groups.find((entry) => entry.id === groupId)?.name ?? "группа"}`
      : `Правило · ${targets.find((entry) => entry.id === targetId)?.name ?? "цель"}`;

  return (
    <Dialog
      title={title}
      subtitle="правило применяется к цели или ко всей группе"
      onClose={onClose}
      footer={
        <>
          <button type="button" className="btn btn--ghost" onClick={onClose}>
            Отмена
          </button>
          <button type="submit" form="rule-form" className="btn" disabled={busy}>
            {busy ? "Сохранение…" : "Сохранить"}
          </button>
        </>
      }
    >
      <form id="rule-form" className="form" onSubmit={submit}>
        {!editing ? (
          <div className="field">
            <label htmlFor="rule-scope">Область правила</label>
            <select
              id="rule-scope"
              data-dialog-autofocus={!editing ? true : undefined}
              value={scope}
              onChange={(event) => setScope(event.target.value as "target" | "group")}
            >
              <option value="target">Цель</option>
              <option value="group">Группа</option>
            </select>
          </div>
        ) : null}
        {scope === "target" ? (
          <div className="field">
            <label htmlFor="rule-target">Цель</label>
            <select
              id="rule-target"
              value={targetId}
              onChange={(event) => setTargetId(event.target.value)}
              disabled={editing}
            >
              <option value="">Выберите цель</option>
              {targets.map((entry) => (
                <option key={entry.id} value={entry.id}>
                  {entry.name}
                </option>
              ))}
            </select>
          </div>
        ) : (
          <div className="field">
            <label htmlFor="rule-group">Группа</label>
            <select
              id="rule-group"
              value={groupId}
              onChange={(event) => setGroupId(event.target.value)}
              disabled={editing}
            >
              <option value="">Выберите группу</option>
              {groups.map((entry) => (
                <option key={entry.id} value={entry.id}>
                  {entry.name}
                </option>
              ))}
            </select>
            <p className="field-hint">
              Правило группы работает как шаблон для целей без своего правила и как отдельный
              групповой инцидент.
            </p>
          </div>
        )}
        <label className="check">
          <input type="checkbox" checked={enabled} data-dialog-autofocus={editing ? true : undefined} onChange={(event) => setEnabled(event.target.checked)} />
          Правило включено
        </label>
        <div className="field">
          <span className="label">Когда считать сбоем</span>
          <div className="seg">
            <button type="button" aria-pressed={mode === "all"} onClick={() => setMode("all")}>
              все агенты
            </button>
            <button type="button" aria-pressed={mode === "any"} onClick={() => setMode("any")}>
              любой агент
            </button>
          </div>
        </div>
        <div className="form-row">
          <label className="field">
            <span className="label">Сбоев подряд</span>
            <input type="number" min={1} max={100} value={failure} onChange={(event) => setFailure(Number(event.target.value))} />
          </label>
          <label className="field">
            <span className="label">Успехов для recovered</span>
            <input type="number" min={1} max={100} value={recovery} onChange={(event) => setRecovery(Number(event.target.value))} />
          </label>
          <label className="field">
            <span className="label">Cooldown, минут</span>
            <input type="number" min={0} max={1440} value={cooldown} onChange={(event) => setCooldown(Number(event.target.value))} />
          </label>
        </div>
        <div className="field">
          <span className="label">Каналы доставки</span>
          {channels.length === 0 ? (
            <p className="muted">Сначала добавьте канал во вкладке «Каналы».</p>
          ) : (
            <div className="check-list">
              {channels.map((channel) => (
                <label key={channel.id} className="check">
                  <input
                    type="checkbox"
                    checked={selected.includes(channel.id)}
                    onChange={(event) => {
                      setSelected((current) =>
                        event.target.checked
                          ? [...current, channel.id]
                          : current.filter((id) => id !== channel.id),
                      );
                    }}
                  />
                  <span>{channel.name}</span>
                  <span className={`tag tag--${channel.kind}`}>{KIND_LABEL[channel.kind]}</span>
                </label>
              ))}
            </div>
          )}
        </div>
        <p className="dialog-note">
          down уйдёт после {failure} неудачных оценок подряд, recovered — после {recovery} успешной.
          Повторный сбой в течение {cooldown} {cooldown === 1 ? "минуты" : "минут"} ждёт в очереди
          и не создаёт второй down.
        </p>
      </form>
    </Dialog>
  );
}

function ChannelDialog({
  channel,
  onClose,
  onSaved,
  onError,
}: {
  channel: AlertChannel | null;
  onClose: () => void;
  onSaved: () => Promise<void>;
  onError: (message: string) => void;
}) {
  const [name, setName] = useState(channel?.name ?? "");
  const [kind, setKind] = useState<AlertChannelKind>(channel?.kind ?? "telegram");
  const [enabled, setEnabled] = useState(channel?.enabled ?? true);
  const [config, setConfig] = useState<Record<string, unknown>>(
    channel ? defaultConfig(channel.kind) : defaultConfig("telegram"),
  );
  const [busy, setBusy] = useState(false);

  const set = (key: string, value: unknown) => setConfig((current) => ({ ...current, [key]: value }));

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setBusy(true);
    const payload: AlertChannelInput = {
      name: name.trim() || KIND_LABEL[kind],
      kind,
      config,
      enabled,
    };
    try {
      if (channel) await api.updateAlertChannel(channel.id, payload);
      else await api.createAlertChannel(payload);
      await onSaved();
    } catch (cause) {
      onError(errorText(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      title={channel ? `Канал · ${channel.name}` : "Новый канал"}
      subtitle="Секреты сохраняются только в зашифрованном виде"
      onClose={onClose}
      footer={
        <>
          <button type="button" className="btn btn--ghost" onClick={onClose}>
            Отмена
          </button>
          <button type="submit" form="channel-form" className="btn" disabled={busy}>
            {busy ? "Сохранение…" : "Сохранить"}
          </button>
        </>
      }
    >
      {channel ? (
        <p className="dialog-note">
          Сохранённые секреты не возвращаются в браузер. Заполните поля заново, чтобы обновить канал.
        </p>
      ) : null}
      <form id="channel-form" className="form" onSubmit={submit}>
        <div className="field">
          <span className="label">Тип</span>
          <div className="seg">
            {(["telegram", "webhook", "email"] as AlertChannelKind[]).map((value) => (
              <button
                key={value}
                type="button"
                aria-pressed={kind === value}
                onClick={() => {
                  setKind(value);
                  setConfig(defaultConfig(value));
                }}
              >
                {KIND_LABEL[value]}
              </button>
            ))}
          </div>
        </div>
        <label className="field">
          <span className="label">Название</span>
          <input data-dialog-autofocus value={name} placeholder={KIND_LABEL[kind]} onChange={(event) => setName(event.target.value)} />
        </label>
        {kind === "telegram" ? (
          <>
            <label className="field">
              <span className="label">Bot token</span>
              <input type="password" value={String(config.bot_token ?? "")} onChange={(event) => set("bot_token", event.target.value)} autoComplete="off" />
            </label>
            <label className="field">
              <span className="label">Chat ID</span>
              <input value={String(config.chat_id ?? "")} placeholder="-1001234567890" onChange={(event) => set("chat_id", event.target.value)} />
            </label>
          </>
        ) : null}
        {kind === "webhook" ? (
          <>
            <label className="field">
              <span className="label">URL</span>
              <input value={String(config.url ?? "")} placeholder="https://example.com/hook" onChange={(event) => set("url", event.target.value)} />
            </label>
            <label className="field">
              <span className="label">HMAC secret, необязательно</span>
              <input type="password" value={String(config.secret ?? "")} onChange={(event) => set("secret", event.target.value)} autoComplete="off" />
            </label>
            <p className="muted">Подпись уйдёт в X-Pingushka-Signature: sha256=…</p>
          </>
        ) : null}
        {kind === "email" ? (
          <>
            <div className="form-row">
              <label className="field">
                <span className="label">SMTP host</span>
                <input value={String(config.host ?? "")} onChange={(event) => set("host", event.target.value)} />
              </label>
              <label className="field">
                <span className="label">Port</span>
                <input type="number" value={Number(config.port ?? 587)} onChange={(event) => set("port", Number(event.target.value))} />
              </label>
            </div>
            <div className="field">
              <span className="label">TLS</span>
              <div className="seg">
                {["starttls", "implicit", "none"].map((value) => (
                  <button key={value} type="button" aria-pressed={config.tls === value} onClick={() => set("tls", value)}>
                    {value}
                  </button>
                ))}
              </div>
            </div>
            <div className="form-row">
              <label className="field">
                <span className="label">From</span>
                <input value={String(config.from ?? "")} onChange={(event) => set("from", event.target.value)} />
              </label>
              <label className="field">
                <span className="label">To</span>
                <input value={String(config.to ?? "")} onChange={(event) => set("to", event.target.value)} />
              </label>
            </div>
            <div className="form-row">
              <label className="field">
                <span className="label">Username</span>
                <input value={String(config.username ?? "")} onChange={(event) => set("username", event.target.value)} autoComplete="off" />
              </label>
              <label className="field">
                <span className="label">Password</span>
                <input type="password" value={String(config.password ?? "")} onChange={(event) => set("password", event.target.value)} autoComplete="new-password" />
              </label>
            </div>
          </>
        ) : null}
        <label className="check">
          <input type="checkbox" checked={enabled} onChange={(event) => setEnabled(event.target.checked)} />
          Канал включён
        </label>
      </form>
    </Dialog>
  );
}
