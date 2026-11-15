import { useState, type FormEvent } from "react";
import { ApiError, errorText } from "../api";

function BrandPanel({ compact = false }: { compact?: boolean }) {
  return (
    <aside className={compact ? "brand brand--compact" : "brand"}>
      <div className="brandline">
        <span className="mark" /> pingushka
      </div>
      {!compact ? (
        <>
          <div className="trace-wrap">
            <svg className="trace" viewBox="0 0 440 176" role="img" aria-label="Сигнал агентов за сутки">
              <g stroke="#2c3a3b" strokeWidth="1">
                <line x1="0" y1="24" x2="440" y2="24" />
                <line x1="0" y1="72" x2="440" y2="72" />
                <line x1="0" y1="120" x2="440" y2="120" />
                <line x1="0" y1="168" x2="440" y2="168" />
                <line x1="73" y1="0" x2="73" y2="176" />
                <line x1="147" y1="0" x2="147" y2="176" />
                <line x1="220" y1="0" x2="220" y2="176" />
                <line x1="293" y1="0" x2="293" y2="176" />
                <line x1="367" y1="0" x2="367" y2="176" />
              </g>
              <path
                d="M0 96 H40 L44 96 L48 88 L52 96 H96 L100 96 L104 92 L108 96 H152 L156 96 L160 90 L164 96 H208 L212 96 L216 94 L220 96 H264 L268 96 L272 86 L276 96 H320 L324 96 L328 93 L332 96 H376 L380 96 L384 90 L388 96 H440"
                fill="none"
                stroke="#7fbdb4"
                strokeWidth="1.75"
              />
              <path
                d="M0 112 H60 L64 112 L68 106 L72 112 H132 L136 112 L140 109 L144 112 H204 L208 112 L212 104 L216 112 H276 L280 112 L284 110 L288 112 H348 L352 112 L356 105 L360 112 H420 L424 112 H440"
                fill="none"
                stroke="#8d9b97"
                strokeWidth="1.25"
                strokeDasharray="3 3"
              />
              <rect x="212" y="160" width="4" height="8" fill="#a9462f" />
              <rect x="216" y="160" width="4" height="8" fill="#a9462f" />
            </svg>
            <div className="trace-note">
              <span>msk-1 · 24 ч</span>
              <span>01:40 — 4 мин без ответа</span>
              <span>fra-2</span>
            </div>
          </div>
          <p className="brand-copy">
            Hub принимает сигналы агентов и хранит историю проверок. Агент сам подключается наружу —
            входящие порты и белый IP не нужны.
          </p>
          <div className="brand-meta">
            <span>hub 0.1.0</span>
            <span>протокол 1</span>
            <span>агенты в разных точках</span>
          </div>
        </>
      ) : null}
    </aside>
  );
}

function authError(error: unknown): string {
  if (error instanceof ApiError) {
    if (error.status === 401) return "Неверный логин или пароль.";
    if (error.status === 429) return "Слишком много попыток входа. Повторите через 15 минут.";
    if (error.status === 409) return "Hub уже настроен. Войдите существующим администратором.";
    if (error.status === 422) return error.message;
  }
  return errorText(error);
}

interface LoginProps {
  setupRequired: boolean;
  notice?: string | null;
  onLogin: (username: string, password: string) => Promise<void>;
  onGoSetup: () => void;
}

export function LoginScreen({ setupRequired, notice, onLogin, onGoSetup }: LoginProps) {
  const [username, setUsername] = useState("demo");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await onLogin(username, password);
    } catch (cause) {
      setError(authError(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="auth-screen">
      <BrandPanel />
      <main className="auth-wrap">
        <form className="auth" onSubmit={submit}>
          <p className="auth-host">{window.location.host}</p>
          <h1>Вход</h1>
          <p className="auth-sub">Доступ только для администратора Hub.</p>
          {notice ? <p className="auth-notice">{notice}</p> : null}
          <div className="field">
            <label htmlFor="login-name">Логин</label>
            <input
              id="login-name"
              name="username"
              autoComplete="username"
              value={username}
              onChange={(event) => setUsername(event.target.value)}
              required
            />
          </div>
          <div className="field">
            <label htmlFor="login-pass">Пароль</label>
            <input
              id="login-pass"
              name="password"
              type="password"
              autoComplete="current-password"
              value={password}
              onChange={(event) => setPassword(event.target.value)}
              aria-invalid={error ? "true" : undefined}
              aria-describedby={error ? "login-error" : undefined}
              required
            />
            {error ? (
              <p className="field-error" id="login-error" role="alert">
                <span aria-hidden="true">!</span> {error}
              </p>
            ) : null}
          </div>
          <button className="btn btn--block" type="submit" disabled={busy}>
            {busy ? "Проверяем…" : "Войти"}
          </button>
          {setupRequired ? (
            <p className="auth-alt">
              Hub ещё не настроен?{" "}
              <button type="button" className="link-button" onClick={onGoSetup}>
                Создать администратора
              </button>
            </p>
          ) : null}
          <p className="auth-note">
            Сессия — httpOnly cookie на 7 дней. Пароль проверяется argon2id, попытки ограничены
            пятью за 15 минут.
          </p>
        </form>
      </main>
    </div>
  );
}

interface SetupProps {
  onSetup: (username: string, password: string) => Promise<void>;
  onGoLogin: () => void;
  initialUsername?: string;
}

export function SetupScreen({ onSetup, onGoLogin, initialUsername = "" }: SetupProps) {
  const [username, setUsername] = useState(initialUsername);
  const [password, setPassword] = useState("");
  const [confirmation, setConfirmation] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (password !== confirmation) {
      setError("Пароли не совпадают.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await onSetup(username, password);
    } catch (cause) {
      setError(authError(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="setup-screen">
      <header className="setup-bar">
        <div className="brandline">
          <span className="mark" /> pingushka
        </div>
      </header>
      <main className="setup-body">
        <form className="setup-form" onSubmit={submit}>
          <h1>Первый запуск</h1>
          <p className="lead">
            Создайте единственного администратора Hub. Эта форма исчезнет после создания; новых
            пользователей в MVP нет.
          </p>
          <div className="field">
            <label htmlFor="setup-name">Имя администратора</label>
            <input
              id="setup-name"
              autoComplete="username"
              value={username}
              onChange={(event) => setUsername(event.target.value)}
              required
            />
          </div>
          <div className="field">
            <label htmlFor="setup-pass">Пароль</label>
            <input
              id="setup-pass"
              type="password"
              autoComplete="new-password"
              value={password}
              onChange={(event) => setPassword(event.target.value)}
              required
            />
            <p className="field-hint">Не короче 12 символов. Хранится как argon2id-хэш.</p>
          </div>
          <div className="field">
            <label htmlFor="setup-pass2">Повторите пароль</label>
            <input
              id="setup-pass2"
              type="password"
              autoComplete="new-password"
              value={confirmation}
              onChange={(event) => setConfirmation(event.target.value)}
              required
            />
          </div>
          {error ? (
            <p className="field-error" role="alert">
              <span aria-hidden="true">!</span> {error}
            </p>
          ) : null}
          <button className="btn btn--block" type="submit" disabled={busy}>
            {busy ? "Создаём…" : "Создать администратора"}
          </button>
          <p className="auth-alt">
            Hub уже настроен?{" "}
            <button type="button" className="link-button" onClick={onGoLogin}>
              Войти
            </button>
          </p>
        </form>
      </main>
    </div>
  );
}
