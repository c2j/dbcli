use mysql_async::prelude::*;

#[tokio::main(flavor = "current_thread")]
async fn main() -> mysql_async::Result<()> {
    let url = "mysql://mcp:ProtoTest_2026@127.0.0.1:3306/mysql";
    let pool = mysql_async::Pool::new(url);
    let mut conn = pool.get_conn().await?;

    let v: Option<String> = conn.query_first("SELECT VERSION()").await?;
    println!("VERSION: {v:?}");

    let db_rows: Vec<mysql_async::Row> = conn
        .query("SHOW DATABASES")
        .await?
        .into_iter()
        .collect();
    for row in db_rows {
        let db: Option<String> = row.get(0);
        println!("  DB: {}", db.unwrap_or_default());
    }

    conn.query_drop("DROP TABLE IF EXISTS poc").await?;
    conn.query_drop("CREATE TABLE poc (id INT AUTO_INCREMENT PRIMARY KEY, name VARCHAR(100))").await?;
    conn.exec_drop("INSERT INTO poc (name) VALUES (:name)", params! { "name" => "hello" }).await?;

    let poc_rows = conn.query_map("SELECT id, name FROM poc", |(id, name): (i32, String)| (id, name)).await?;
    for (id, name) in &poc_rows { println!("  {id}: {name}"); }

    let tbl_rows: Vec<mysql_async::Row> = conn
        .query("SELECT TABLE_NAME FROM information_schema.TABLES WHERE TABLE_SCHEMA = DATABASE() LIMIT 5")
        .await?
        .into_iter()
        .collect();
    for row in tbl_rows {
        let t: Option<String> = row.get(0);
        println!("  TABLE: {}", t.unwrap_or_default());
    }

    let explain_rows: Vec<mysql_async::Row> = conn
        .query("EXPLAIN SELECT count(*) FROM poc")
        .await?
        .into_iter()
        .collect();
    println!("EXPLAIN: got {} rows", explain_rows.len());

    pool.disconnect().await?;
    println!("OK");
    Ok(())
}
