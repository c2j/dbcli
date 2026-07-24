// Oracle regression test suite
// Run: POLARDB_ORACLE_TEST_URL=oracle://system:testpass@127.0.0.1:1521/FREEPDB1 cargo test --features "oracle,integration" --test regress_oracle

#[cfg(all(feature = "integration", feature = "oracle"))]
mod common;

#[cfg(all(feature = "integration", feature = "oracle"))]
mod tests {
    use polar_mysql::backend::oracle::OracleFactory;
    use serde_json::Value;

    fn oracle_url() -> String {
        std::env::var("POLARDB_ORACLE_TEST_URL")
            .unwrap_or_else(|_| "oracle://system:testpass@127.0.0.1:1521/FREEPDB1".to_string())
    }

    async fn connect() -> Box<dyn polar_mysql::backend::DbConn + Send> {
        let pool = crate::common::connect_pool(OracleFactory, &oracle_url()).await;
        pool.acquire().await.expect("acquire")
    }

    const TABLE: &str = "REGRESS_TEST";

    async fn ensure_table(mut conn: &mut dyn polar_mysql::backend::DbConn) {
        let _ = conn
            .query_drop(&format!(
                "BEGIN EXECUTE IMMEDIATE 'DROP TABLE {}'; EXCEPTION WHEN OTHERS THEN NULL; END;",
                TABLE
            ))
            .await;
        conn.query_drop(&format!(
            "CREATE TABLE {} (id NUMBER PRIMARY KEY, name VARCHAR2(100), amount NUMBER(10,2))",
            TABLE
        ))
        .await
        .expect("create table");
        conn.query_drop(&format!("INSERT INTO {} VALUES (1, 'hello', 99.99)", TABLE))
            .await
            .expect("insert");
    }

    async fn drop_table(mut conn: Box<dyn polar_mysql::backend::DbConn>) {
        let _ = conn.query_drop(&format!("DROP TABLE {}", TABLE)).await;
    }

    #[tokio::test]
    async fn oracle_database_info() {
        let mut conn = connect().await;
        let sql = conn.dialect().database_info().to_string();
        let result = polar_mysql::backend::DbConn::query(&mut *conn, &sql)
            .await
            .expect("database_info");
        assert!(result.row_count >= 1);
        crate::common::assert_columns(
            &result.columns,
            &[
                "version",
                "database",
                "current_user",
                "hostname",
                "port",
                "os",
                "charset",
                "collation",
                "version_comment",
            ],
        );
    }

    #[tokio::test]
    async fn oracle_list_tables() {
        let mut conn = connect().await;
        ensure_table(&mut *conn).await;
        let sql = conn.dialect().list_tables().to_string();
        let result = polar_mysql::backend::DbConn::query(&mut *conn, &sql)
            .await
            .expect("list_tables");
        assert!(result.row_count >= 1);
        crate::common::assert_columns(
            &result.columns,
            &[
                "schema_name",
                "table_name",
                "table_type",
                "engine",
                "row_count",
                "total_size",
                "comment",
            ],
        );
        drop_table(conn).await;
    }

    #[tokio::test]
    async fn oracle_table_columns() {
        let mut conn = connect().await;
        ensure_table(&mut *conn).await;
        let sql = conn.dialect().table_columns().to_string();
        let result = polar_mysql::backend::DbConn::exec(
            &mut *conn,
            &sql,
            &[Value::String("SYSTEM".into()), Value::String(TABLE.into())],
        )
        .await
        .expect("table_columns");
        assert!(result.row_count >= 1);
        crate::common::assert_columns(
            &result.columns,
            &[
                "column_name",
                "data_type",
                "nullable",
                "default_value",
                "ordinal_position",
                "comment",
                "column_key",
            ],
        );
        drop_table(conn).await;
    }

    #[tokio::test]
    async fn oracle_table_indexes() {
        let mut conn = connect().await;
        ensure_table(&mut *conn).await;
        let sql = conn.dialect().table_indexes().to_string();
        let result = polar_mysql::backend::DbConn::exec(
            &mut *conn,
            &sql,
            &[Value::String("SYSTEM".into()), Value::String(TABLE.into())],
        )
        .await
        .expect("table_indexes");
        assert!(result.row_count >= 1);
        crate::common::assert_columns(
            &result.columns,
            &[
                "index_name",
                "is_unique",
                "is_primary",
                "columns",
                "index_type",
            ],
        );
        drop_table(conn).await;
    }

    #[tokio::test]
    async fn oracle_execute_query() {
        let mut conn = connect().await;
        let result = polar_mysql::backend::DbConn::query(&mut *conn, "SELECT 1 FROM dual")
            .await
            .expect("query");
        assert_eq!(result.row_count, 1);
    }

    #[tokio::test]
    async fn oracle_add_limit() {
        let conn = connect().await;
        let limited = conn.dialect().add_limit("SELECT * FROM t", 10);
        assert!(limited.contains("FETCH FIRST"));
    }

    #[tokio::test]
    async fn oracle_build_explain() {
        let conn = connect().await;
        let explain = conn
            .dialect()
            .build_explain("SELECT 1 FROM dual", false, "BASIC");
        assert!(explain.contains("EXPLAIN PLAN"));
    }

    #[tokio::test]
    async fn oracle_read_only_prefixes() {
        let conn = connect().await;
        let prefixes = conn.dialect().read_only_prefixes();
        assert!(prefixes.contains(&"SELECT"));
        assert!(prefixes.contains(&"WITH"));
        assert!(!prefixes.contains(&"SHOW"));
    }

    #[tokio::test]
    async fn oracle_query_error() {
        let mut conn = connect().await;
        let result =
            polar_mysql::backend::DbConn::query(&mut *conn, "SELECT * FROM nonexistent_xyz").await;
        assert!(result.is_err());
    }
}
