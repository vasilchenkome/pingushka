# Pingushka

Pingushka — открытая распределённая система мониторинга на базе центрального Hub и лёгких Linux-агентов (проб). Агенты подключаются к Hub по WebSocket и выполняют проверки из своих сетевых точек.

В планируемый MVP входят проверки HTTP(S), TCP-портов и ICMP; история на PostgreSQL; веб-интерфейс с авторизацией; отдельные одноразовые команды регистрации агентов; развёртывание через Docker Compose с автоматическим HTTPS от Caddy.

**Статус: ранняя разработка.** Уже готовы workspace, доменная модель, схема и репозитории PostgreSQL, батч-запись результатов и криптографические адаптеры. Hub отдаёт защищённый REST API (первичная настройка, сессии, агенты с одноразовой регистрацией, цели, история и сводка), принимает результаты по WebSocket с подтверждениями и чистит устаревшие данные. Linux-агент реализован: планировщик с явными таймаутами, пробы HTTP/TCP/ICMP с явным `tcp_fallback`, ограниченный буфер результатов на диске, отправка батчами с `ack`, reconnect с backoff и джиттером, корректное завершение. `install.sh` ставит агент и systemd-службу. Веб-интерфейс и Docker-образ приложения ещё не реализованы и не выпущены.

## Поддерживаемые платформы агентов

- Linux x86_64
- Linux ARM64

Агенты для Windows и macOS не входят в MVP.

## Установка агента

Каждый агент регистрируется своим одноразовым токеном. Создайте агента в Hub и выполните сгенерированную команду на Linux-хосте:

```sh
curl -fsSL https://raw.githubusercontent.com/vasilchenkome/pingushka/main/install.sh \
  | sudo sh -s -- --hub https://monitor.example.com --enrollment-token <токен>
```

Установщик скачивает релизный бинарь для x86_64 или ARM64, проверяет SHA-256, обменивает enrollment-токен на постоянный credential, пишет `/etc/pingushka/agent.toml` с правами `0600`, ставит systemd-службу `pingushka-agent.service`, выдаёт `CAP_NET_RAW` для ICMP и запускает службу. Повторный запуск команды ротирует credential и перезапускает службу.

Буфер результатов хранится в `/var/lib/pingushka/buffer.jsonl`. Логи: `journalctl -u pingushka-agent -f`.

## Локальная разработка

Требуются stable Rust toolchain и Docker Desktop с Docker Compose.

1. Скопируйте `.env.example` в `.env`. Установите длинный случайный буквенно-цифровой `POSTGRES_PASSWORD` и укажите то же значение в `DATABASE_URL`.
2. Запустите локальный PostgreSQL:

   ```sh
   docker compose up -d postgres
   ```

3. Соберите и проверьте Rust workspace:

   ```sh
   cargo build --workspace
   cargo test --workspace
   cargo clippy --workspace --all-targets --all-features -- -D warnings
   ```

Порт локальной базы привязан только к `127.0.0.1`. Не публикуйте `.env`: файл исключён из Git. Команда `docker compose down -v` удалит локальный том базы и все данные в нём.

Hub читает переменные `DATABASE_URL`, `BIND_ADDR`, `PUBLIC_URL`, `COOKIE_SECURE`, `SESSION_TTL_HOURS`, `ENROLLMENT_TTL_MINUTES`, `RETENTION_DAYS`, `MANUAL_CHECK_COOLDOWN_SECS` и `PRUNE_INTERVAL_MINUTES`.

## Лицензия

MIT. См. [LICENSE](LICENSE).
