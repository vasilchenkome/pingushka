# Pingushka

Pingushka — открытая распределённая система мониторинга на базе центрального Hub и лёгких Linux-агентов (проб). Агенты подключаются к Hub по WebSocket и выполняют проверки из своих сетевых точек.

В планируемый MVP входят проверки HTTP(S), TCP-портов и ICMP; история на PostgreSQL; веб-интерфейс с авторизацией; отдельные одноразовые команды регистрации агентов; развёртывание через Docker Compose с автоматическим HTTPS от Caddy.

**Статус: ранняя разработка.** Сейчас в репозитории есть только каркас workspace. Hub, агенты, веб-интерфейс, установщик и Docker-образ приложения ещё не реализованы и не выпущены.

## Поддерживаемые платформы агентов

- Linux x86_64
- Linux ARM64

Агенты для Windows и macOS не входят в MVP.

## Локальная разработка

Требуются stable Rust toolchain и Docker Desktop с Docker Compose.

1. Скопируйте `.env.example` в `.env` и замените `POSTGRES_PASSWORD` на длинный случайный локальный пароль.
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

## Лицензия

MIT. См. [LICENSE](LICENSE).
