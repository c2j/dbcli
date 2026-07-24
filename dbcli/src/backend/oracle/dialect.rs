use crate::backend::Dialect;

pub(crate) struct OracleDialect;

impl Dialect for OracleDialect {
    fn database_info(&self) -> &str {
        "SELECT \
         (SELECT banner FROM v$version WHERE banner LIKE 'Oracle%' AND ROWNUM = 1) AS version, \
         SYS_CONTEXT('USERENV','CURRENT_SCHEMA') AS database, \
         SYS_CONTEXT('USERENV','SESSION_USER') AS current_user, \
         SYS_CONTEXT('USERENV','HOST') AS hostname, \
         CAST(NULL AS VARCHAR2(1)) AS port, \
         CAST(NULL AS VARCHAR2(1)) AS os, \
         (SELECT value FROM nls_database_parameters WHERE parameter = 'NLS_CHARACTERSET') AS charset, \
         (SELECT value FROM nls_database_parameters WHERE parameter = 'NLS_SORT') AS collation, \
         (SELECT banner FROM v$version WHERE banner LIKE 'Oracle%' AND ROWNUM = 1) AS version_comment \
         FROM dual"
    }

    fn list_tables(&self) -> &str {
        "SELECT \
         t.OWNER AS schema_name, \
         t.TABLE_NAME AS table_name, \
         t.TABLE_TYPE AS table_type, \
         NULL AS engine, \
         t.NUM_ROWS AS row_count, \
         s.BYTES AS total_size, \
         c.COMMENTS AS comment \
         FROM all_tables t \
         LEFT JOIN all_tab_comments c ON c.OWNER = t.OWNER AND c.TABLE_NAME = t.TABLE_NAME \
         LEFT JOIN dba_segments s ON s.OWNER = t.OWNER AND s.SEGMENT_NAME = t.TABLE_NAME \
         WHERE t.OWNER NOT IN ('SYS','SYSTEM','OUTLN','DBSNMP','XDB','CTXSYS','MDSYS','ORDSYS') \
         ORDER BY t.OWNER, t.TABLE_NAME"
    }

    fn table_columns(&self) -> &str {
        "SELECT \
         c.COLUMN_NAME AS column_name, \
         c.DATA_TYPE || \
           CASE \
             WHEN c.DATA_TYPE IN ('VARCHAR2','NVARCHAR2','CHAR','NCHAR','RAW') \
               THEN '(' || c.DATA_LENGTH || ')' \
             WHEN c.DATA_TYPE = 'NUMBER' \
               THEN '(' || NVL(TO_CHAR(c.DATA_PRECISION),'*') || ',' || NVL(TO_CHAR(c.DATA_SCALE),'*') || ')' \
             ELSE '' \
           END AS data_type, \
         CASE WHEN c.NULLABLE = 'Y' THEN 1 ELSE 0 END AS nullable, \
         c.DATA_DEFAULT AS default_value, \
         c.COLUMN_ID AS ordinal_position, \
         com.COMMENTS AS comment, \
         (SELECT LISTAGG(cc.CONSTRAINT_TYPE, ',') WITHIN GROUP (ORDER BY cc.CONSTRAINT_TYPE) \
          FROM all_cons_columns acc \
          JOIN all_constraints cc ON cc.CONSTRAINT_NAME = acc.CONSTRAINT_NAME \
            AND cc.OWNER = acc.OWNER \
          WHERE acc.OWNER = c.OWNER \
            AND acc.TABLE_NAME = c.TABLE_NAME \
            AND acc.COLUMN_NAME = c.COLUMN_NAME \
            AND cc.CONSTRAINT_TYPE IN ('P','U','R')) AS column_key \
         FROM all_tab_columns c \
         LEFT JOIN all_col_comments com \
           ON com.OWNER = c.OWNER AND com.TABLE_NAME = c.TABLE_NAME AND com.COLUMN_NAME = c.COLUMN_NAME \
         WHERE c.OWNER = :1 AND c.TABLE_NAME = :2 \
         ORDER BY c.COLUMN_ID"
    }

    fn table_indexes(&self) -> &str {
        "SELECT \
         i.INDEX_NAME AS index_name, \
         CASE WHEN i.UNIQUENESS = 'UNIQUE' THEN 1 ELSE 0 END AS is_unique, \
         CASE WHEN c.CONSTRAINT_TYPE = 'P' THEN 1 ELSE 0 END AS is_primary, \
         (SELECT LISTAGG(ic.COLUMN_NAME, ', ') WITHIN GROUP (ORDER BY ic.COLUMN_POSITION) \
          FROM all_ind_columns ic \
          WHERE ic.INDEX_OWNER = i.OWNER AND ic.INDEX_NAME = i.INDEX_NAME) AS columns, \
         i.INDEX_TYPE AS index_type \
         FROM all_indexes i \
         LEFT JOIN all_constraints c \
           ON c.OWNER = i.OWNER \
           AND c.INDEX_NAME = i.INDEX_NAME \
           AND c.CONSTRAINT_TYPE = 'P' \
         WHERE i.OWNER = :1 AND i.TABLE_NAME = :2 \
         ORDER BY i.INDEX_NAME"
    }

    fn read_only_prefixes(&self) -> &[&str] {
        &["SELECT", "EXPLAIN", "WITH"]
    }

    fn add_limit(&self, sql: &str, n: usize) -> String {
        let upper = sql.trim().to_uppercase();
        if upper.contains("FETCH FIRST") || upper.contains("ROWNUM") {
            sql.trim().to_string()
        } else {
            format!("{} FETCH FIRST {} ROWS ONLY", sql.trim(), n)
        }
    }

    fn build_explain(&self, sql: &str, analyze: bool, format: &str) -> String {
        if analyze {
            format!(
                "EXPLAIN PLAN SET STATEMENT_ID = 'polar_explain' FOR {}; \
                 SELECT * FROM TABLE(DBMS_XPLAN.DISPLAY(NULL, 'polar_explain', 'ALL'))",
                sql
            )
        } else {
            let fmt = match format.to_uppercase().as_str() {
                "JSON" => "BASIC",
                _ => "TYPICAL",
            };
            format!(
                "EXPLAIN PLAN SET STATEMENT_ID = 'polar_explain' FOR {}; \
                 SELECT * FROM TABLE(DBMS_XPLAN.DISPLAY(NULL, 'polar_explain', '{}'))",
                sql, fmt
            )
        }
    }

    fn set_statement_timeout_sql(&self, _ms: u64) -> Option<String> {
        None
    }

    fn kill_own_connection_sql(&self) -> Option<String> {
        None
    }

    fn default_port(&self) -> u16 {
        1521
    }

    fn url_scheme(&self) -> &str {
        "oracle"
    }

    fn identifier_quote(&self) -> char {
        '"'
    }

    fn supports_hash_comment(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::Dialect;

    #[test]
    fn test_database_info_contains_keywords() {
        let d = OracleDialect;
        let sql = d.database_info();
        assert!(sql.contains("v$version"));
        assert!(sql.contains("SYS_CONTEXT"));
        assert!(sql.contains("dual"));
    }

    #[test]
    fn test_list_tables_contains_all_tables() {
        let d = OracleDialect;
        let sql = d.list_tables();
        assert!(sql.contains("all_tables"));
        assert!(sql.contains("SYS"));
        assert!(sql.contains("SYSTEM"));
    }

    #[test]
    fn test_table_columns_uses_bind_params() {
        let d = OracleDialect;
        let sql = d.table_columns();
        assert!(sql.contains(":1"));
        assert!(sql.contains(":2"));
        assert!(sql.contains("all_tab_columns"));
    }

    #[test]
    fn test_table_indexes_uses_listagg() {
        let d = OracleDialect;
        let sql = d.table_indexes();
        assert!(sql.contains("LISTAGG"));
        assert!(sql.contains("all_indexes"));
        assert!(sql.contains("all_ind_columns"));
    }

    #[test]
    fn test_read_only_prefixes_no_show_describe() {
        let d = OracleDialect;
        let prefixes = d.read_only_prefixes();
        assert!(prefixes.contains(&"SELECT"));
        assert!(prefixes.contains(&"EXPLAIN"));
        assert!(!prefixes.contains(&"SHOW"));
        assert!(!prefixes.contains(&"DESCRIBE"));
    }

    #[test]
    fn test_add_limit_fetch_first() {
        let d = OracleDialect;
        let result = d.add_limit("SELECT * FROM dual", 10);
        assert!(result.contains("FETCH FIRST 10 ROWS ONLY"));
    }

    #[test]
    fn test_add_limit_no_double_limit() {
        let d = OracleDialect;
        let result = d.add_limit("SELECT * FROM dual FETCH FIRST 5 ROWS ONLY", 10);
        assert_eq!(result, "SELECT * FROM dual FETCH FIRST 5 ROWS ONLY");
    }

    #[test]
    fn test_build_explain_contains_dbms_xplan() {
        let d = OracleDialect;
        let sql = d.build_explain("SELECT * FROM dual", false, "TYPICAL");
        assert!(sql.contains("EXPLAIN PLAN"));
        assert!(sql.contains("DBMS_XPLAN.DISPLAY"));
        assert!(sql.contains("polar_explain"));
    }

    #[test]
    fn test_build_explain_analyze() {
        let d = OracleDialect;
        let sql = d.build_explain("SELECT * FROM dual", true, "TEXT");
        assert!(sql.contains("EXPLAIN PLAN"));
        assert!(sql.contains("ALL"));
    }

    #[test]
    fn test_no_statement_timeout() {
        let d = OracleDialect;
        assert!(d.set_statement_timeout_sql(1000).is_none());
    }

    #[test]
    fn test_no_kill_own_connection() {
        let d = OracleDialect;
        assert!(d.kill_own_connection_sql().is_none());
    }

    #[test]
    fn test_default_port() {
        assert_eq!(OracleDialect.default_port(), 1521);
    }

    #[test]
    fn test_url_scheme() {
        assert_eq!(OracleDialect.url_scheme(), "oracle");
    }

    #[test]
    fn test_identifier_quote_is_double_quote() {
        assert_eq!(OracleDialect.identifier_quote(), '"');
    }

    #[test]
    fn test_no_hash_comment() {
        assert!(!OracleDialect.supports_hash_comment());
    }
}
