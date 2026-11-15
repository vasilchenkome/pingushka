use pingushka_storage_postgres::PgStore;
use sqlx::PgPool;

#[sqlx::test(migrations = false)]
async fn migrations_are_repeatable_and_create_core_tables(pool: PgPool) {
    let store = PgStore::new(pool.clone());

    store.migrate().await.expect("first migration run");
    store.migrate().await.expect("repeat migration run");

    let table_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema = current_schema() AND table_name = ANY($1)",
    )
    .bind(vec![
        "users",
        "sessions",
        "agents",
        "agent_enrollments",
        "targets",
        "target_agents",
        "check_results",
        "target_groups",
        "target_tags",
        "alert_rules",
        "alert_subject_states",
    ])
    .fetch_one(&pool)
    .await
    .expect("query migrated tables");

    assert_eq!(table_count, 11);
}

#[sqlx::test(migrations = false)]
async fn upgrade_from_v020_backfills_groups_from_target_prefixes(pool: PgPool) {
    sqlx::raw_sql(include_str!("../migrations/0001_core.sql"))
        .execute(&pool)
        .await
        .expect("apply core schema");
    sqlx::raw_sql(include_str!("../migrations/0002_alerts.sql"))
        .execute(&pool)
        .await
        .expect("apply alerts schema");
    sqlx::query(
        "INSERT INTO targets (id, name, kind, spec_json, interval_secs, timeout_ms, created_at, updated_at) VALUES (gen_random_uuid(), '[Neva] Main', 'http', '{}'::jsonb, 30, 5000, now(), now())",
    )
    .execute(&pool)
    .await
    .expect("insert legacy target");
    sqlx::raw_sql(include_str!(
        "../migrations/0003_groups_tags_alert_scopes.sql"
    ))
    .execute(&pool)
    .await
    .expect("apply groups migration");

    let name: String = sqlx::query_scalar("SELECT name FROM target_groups")
        .fetch_one(&pool)
        .await
        .expect("backfilled group");
    assert_eq!(name, "Neva");
    let linked: bool = sqlx::query_scalar(
        "SELECT g.id = t.group_id FROM targets t JOIN target_groups g ON lower(g.name) = lower('Neva') WHERE t.name = '[Neva] Main'",
    )
    .fetch_one(&pool)
    .await
    .expect("linked legacy target");
    assert!(linked);
}
