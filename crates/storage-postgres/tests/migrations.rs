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
    ])
    .fetch_one(&pool)
    .await
    .expect("query migrated tables");

    assert_eq!(table_count, 7);
}
