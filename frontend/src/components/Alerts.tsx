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
} from "../types";
import type { ViewName } from "./Targets";
import { Dialog, StatusDot } from "./ui";

interface AlertsScreenProps {
  targets: Target[];
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

function defaultsFor(target: Target, channels: AlertChannel[]): AlertRule {
  return {
    id: "",
    target_id: target.id,
    target_name: target.name,
    target_kind: target.spec.kind,
    target_ref:
      target.spec.kind === "http" ? target.spec.url : `${target.spec.host}${target.spec.kind === "tcp" ? `:${target.spec.port}` : ""}`,
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

export function AlertsScreen({ targets, user, onNavigate, onLogout }: AlertsScreenProps) {
  const [tab, setTab] = useState<Tab>("rules");
  const [channels, setChannels] = useState<AlertChannel[]>([]);
  const [rules, setRules] = useState<AlertRule[]>([]);
  const [events, setEvents] = useState<AlertEvent[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [ruleForm, setRuleForm] = useState<{ target: Target; rule: AlertRule } | null>(null);
  const [channelForm, setChannelForm] = useState<{ channel: AlertChannel | null } | null>(null);

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
    () => new Map(rules.map((rule) => [rule.target_id, rule])),
    [rules],
  );
  const channelName = (id: string) => channels.find((channel) => channel.id === id)?.name ?? "канал";
  const unconfigured = targets.filter((target) => !rulesByTarget.has(target.id));

  const openRule = (target: Target) => {
    const rule = rulesByTarget.get(target.id) ?? defaultsFor(target, channels);
    setRuleForm({ target, rule });
  };

  const removeRule = async (rule: AlertRule) => {
    if (!window.confirm(`Удалить правило для «${rule.target_name}»?`)) return;
    setBusyId(rule.id);
    try {
      await api.deleteAlertRule(rule.target_id);
      setToast("Правило удалено.");
      await load(true);
    } catch (cause) {
      setToast(errorText(cause));
    } finally {
      setBusyId(null);
    }
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

  const removeChannel = async (channel: AlertChannel) => {
    if (!window.confirm(`Удалить канал «${channel.name}»? Правила, где он используется, потеряют этот канал.`)) return;
    setBusyId(channel.id);
    try {
      await api.deleteAlertChannel(channel.id);
      setToast("Канал удалён.");
      await load(true);
    } catch (cause) {
      setToast(errorText(cause));
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

  return (
    <div className="screen alerts-screen">
      <header className="appbar">
        <div className="m-brand">
          <span className="mark" />
        </div>
        <div className="title-row">
          <h1>Оповещения</h1>
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
              <button type="button" className="btn btn--ghost" onClick={() => {
                const target = unconfigured[0] ?? targets[0];
                if (target) openRule(target);
              }}>
                Настроить цель
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
                      <b>{rule.target_name}</b>
                      <span className="rule-ref">{rule.target_ref}</span>
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
                    <button type="button" className="btn btn--ghost" onClick={() => {
                      const target = targets.find((entry) => entry.id === rule.target_id);
                      if (target) openRule(target);
                    }}>
                      Изменить
                    </button>
                    <button type="button" className="link-button danger" disabled={busyId === rule.id} onClick={() => void removeRule(rule)}>
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
                    <button type="button" className="btn btn--ghost" disabled={busyId === channel.id} onClick={() => void testChannel(channel)}>
                      Тест
                    </button>
                    <button type="button" className="btn btn--ghost" onClick={() => setChannelForm({ channel })}>
                      Изменить
                    </button>
                    <button type="button" className="link-button danger" disabled={busyId === channel.id} onClick={() => void removeChannel(channel)}>
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
                    <span>{event.target_name}</span>
                    <span>{event.channel_name}</span>
                    <span className={`delivery delivery--${event.status}`} title={event.last_error ?? undefined}>
                      {event.status === "sent" ? "отправлено" : event.status === "failed" ? "ошибка" : event.status === "sending" ? "отправка" : "в очереди"}
                      {event.attempts > 0 ? ` · ${event.attempts}` : ""}
                    </span>
                    <span className="muted">{relativeTime(event.created_at)}</span>
                    <span>
                      {event.status === "failed" ? (
                        <button type="button" className="link-button" disabled={busyId === event.id} onClick={() => void retry(event)}>
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
          rule={ruleForm.rule}
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
  rule,
  channels,
  onClose,
  onSaved,
  onError,
}: {
  target: Target;
  rule: AlertRule;
  channels: AlertChannel[];
  onClose: () => void;
  onSaved: () => Promise<void>;
  onError: (message: string) => void;
}) {
  const [enabled, setEnabled] = useState(rule.enabled);
  const [mode, setMode] = useState(rule.mode);
  const [failure, setFailure] = useState(rule.failure_threshold);
  const [recovery, setRecovery] = useState(rule.recovery_threshold);
  const [cooldown, setCooldown] = useState(Math.round(rule.cooldown_secs / 60));
  const [selected, setSelected] = useState<string[]>(rule.channel_ids);
  const [busy, setBusy] = useState(false);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setBusy(true);
    try {
      await api.saveAlertRule({
        target_id: target.id,
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

  return (
    <Dialog
      title={`Правило · ${target.name}`}
      subtitle={target.spec.kind === "http" ? target.spec.url : target.spec.host}
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
        <label className="check">
          <input type="checkbox" checked={enabled} onChange={(event) => setEnabled(event.target.checked)} />
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
          <input value={name} placeholder={KIND_LABEL[kind]} onChange={(event) => setName(event.target.value)} />
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
