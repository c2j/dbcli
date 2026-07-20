use mysql_async::consts::ColumnType;
use mysql_async::Row;
use serde_json::{json, Value};

pub(crate) fn format_row_value(row: &Row, idx: usize) -> Value {
    let columns = row.columns_ref();
    if idx >= columns.len() {
        return Value::Null;
    }
    let col_type = columns[idx].column_type();
    format_value_by_type(row, idx, col_type)
}

fn format_value_by_type(row: &Row, idx: usize, col_type: ColumnType) -> Value {
    match col_type {
        ColumnType::MYSQL_TYPE_NULL => Value::Null,

        ColumnType::MYSQL_TYPE_TINY => get_val::<i8>(row, idx).map_or(Value::Null, |v| json!(v)),

        ColumnType::MYSQL_TYPE_SHORT | ColumnType::MYSQL_TYPE_YEAR => {
            get_val::<i16>(row, idx).map_or(Value::Null, |v| json!(v))
        }

        ColumnType::MYSQL_TYPE_INT24 | ColumnType::MYSQL_TYPE_LONG => {
            get_val::<i32>(row, idx).map_or(Value::Null, |v| json!(v))
        }

        ColumnType::MYSQL_TYPE_LONGLONG => {
            get_val::<i64>(row, idx).map_or(Value::Null, |v| json!(v))
        }

        ColumnType::MYSQL_TYPE_FLOAT => get_val::<f32>(row, idx).map_or(Value::Null, |v| json!(v)),

        ColumnType::MYSQL_TYPE_DOUBLE => get_val::<f64>(row, idx).map_or(Value::Null, |v| json!(v)),

        ColumnType::MYSQL_TYPE_DECIMAL | ColumnType::MYSQL_TYPE_NEWDECIMAL => {
            get_str(row, idx).map_or(Value::Null, Value::String)
        }

        ColumnType::MYSQL_TYPE_BIT => get_val::<bool>(row, idx).map_or(Value::Null, |v| json!(v)),

        ColumnType::MYSQL_TYPE_DATE
        | ColumnType::MYSQL_TYPE_TIME
        | ColumnType::MYSQL_TYPE_DATETIME
        | ColumnType::MYSQL_TYPE_TIMESTAMP
        | ColumnType::MYSQL_TYPE_NEWDATE => get_str(row, idx).map_or(Value::Null, Value::String),

        ColumnType::MYSQL_TYPE_JSON => match get_str(row, idx) {
            Some(s) => match serde_json::from_str(&s) {
                Ok(v) => v,
                Err(_) => Value::String(s),
            },
            None => Value::Null,
        },

        ColumnType::MYSQL_TYPE_ENUM
        | ColumnType::MYSQL_TYPE_SET
        | ColumnType::MYSQL_TYPE_GEOMETRY => get_str(row, idx).map_or(Value::Null, Value::String),

        ColumnType::MYSQL_TYPE_VARCHAR
        | ColumnType::MYSQL_TYPE_VAR_STRING
        | ColumnType::MYSQL_TYPE_STRING => get_str(row, idx).map_or(Value::Null, Value::String),

        ColumnType::MYSQL_TYPE_TINY_BLOB
        | ColumnType::MYSQL_TYPE_MEDIUM_BLOB
        | ColumnType::MYSQL_TYPE_LONG_BLOB
        | ColumnType::MYSQL_TYPE_BLOB => get_bytes(row, idx).map_or(Value::Null, |bytes| {
            Value::String(format!("0x{}", hex_encode(&bytes)))
        }),

        _ => get_str(row, idx).map_or(Value::Null, Value::String),
    }
}

fn get_val<T: mysql_async::prelude::FromValue>(row: &Row, idx: usize) -> Option<T> {
    row.get_opt::<T, usize>(idx).and_then(|r| r.ok())
}

fn get_str(row: &Row, idx: usize) -> Option<String> {
    row.get_opt::<String, usize>(idx).and_then(|r| r.ok())
}

fn get_bytes(row: &Row, idx: usize) -> Option<Vec<u8>> {
    row.get_opt::<Vec<u8>, usize>(idx).and_then(|r| r.ok())
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut result = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        result.push_str(&format!("{:02x}", b));
    }
    result
}

/// Get a nullable column value as Option<String>.
/// This is O(N) per call and used by server.rs for JSON serialization.
pub(crate) fn get_column_string(row: &Row, idx: usize) -> Option<String> {
    row.get_opt::<Option<String>, usize>(idx)
        .and_then(|r| r.ok())
        .flatten()
}

/// Get a nullable column value as Option<u64>.
pub(crate) fn get_column_u64(row: &Row, idx: usize) -> Option<u64> {
    row.get_opt::<Option<u64>, usize>(idx)
        .and_then(|r| r.ok())
        .flatten()
}

/// Get a nullable column value as Option<i32>.
pub(crate) fn get_column_i32(row: &Row, idx: usize) -> Option<i32> {
    row.get_opt::<Option<i32>, usize>(idx)
        .and_then(|r| r.ok())
        .flatten()
}

/// Get a nullable column value as Option<bool>.
pub(crate) fn get_column_bool(row: &Row, idx: usize) -> Option<bool> {
    row.get_opt::<Option<bool>, usize>(idx)
        .and_then(|r| r.ok())
        .flatten()
}

pub(crate) fn format_table(columns: &[String], rows: &[Vec<Value>]) -> String {
    if columns.is_empty() {
        return String::new();
    }

    let mut col_widths: Vec<usize> = columns.iter().map(|c| c.chars().count()).collect();
    for row in rows {
        for (i, val) in row.iter().enumerate() {
            if i < col_widths.len() {
                let val_str = value_to_string(val);
                col_widths[i] = col_widths[i].max(val_str.chars().count());
            }
        }
    }

    let top: String = {
        let parts: Vec<String> = col_widths.iter().map(|w| "─".repeat(w + 2)).collect();
        format!("┌{}┐", parts.join("┬"))
    };

    let header_sep: String = {
        let parts: Vec<String> = col_widths.iter().map(|w| "─".repeat(w + 2)).collect();
        format!("├{}┤", parts.join("┼"))
    };

    let bottom: String = {
        let parts: Vec<String> = col_widths.iter().map(|w| "─".repeat(w + 2)).collect();
        format!("└{}┘", parts.join("┴"))
    };

    let make_cells = |values: &[String]| -> Vec<String> {
        values
            .iter()
            .enumerate()
            .map(|(i, s)| format!("{:width$}", s, width = col_widths[i]))
            .collect()
    };

    let format_row = |cells: &[String]| -> String {
        let inner = cells.join(" │ ");
        format!("│ {} │", inner)
    };

    let mut result = String::new();
    result.push_str(&top);
    result.push('\n');

    let header_cells = make_cells(columns);
    result.push_str(&format_row(&header_cells));
    result.push('\n');

    result.push_str(&header_sep);
    result.push('\n');

    for row in rows {
        let cell_strs: Vec<String> = row.iter().map(value_to_string).collect();
        let row_cells = make_cells(&cell_strs);
        result.push_str(&format_row(&row_cells));
        result.push('\n');
    }

    result.push_str(&bottom);
    result.push('\n');

    result
}

fn value_to_string(val: &Value) -> String {
    match val {
        Value::Null => "NULL".to_string(),
        Value::String(s) => s.clone(),
        v => v.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_table_empty_columns() {
        let result = format_table(&[], &[]);
        assert_eq!(result, "");
    }

    #[test]
    fn test_format_table_single_cell() {
        let columns = vec!["val".to_string()];
        let rows = vec![vec![Value::String("x".to_string())]];
        let result = format_table(&columns, &rows);
        let expected = "\
┌─────┐
│ val │
├─────┤
│ x   │
└─────┘
";
        assert_eq!(result, expected);
    }

    #[test]
    fn test_format_table_multiple_columns() {
        let columns = vec!["a".to_string(), "b".to_string()];
        let rows = vec![
            vec![
                Value::String("1".to_string()),
                Value::String("2".to_string()),
            ],
            vec![
                Value::String("3".to_string()),
                Value::String("4".to_string()),
            ],
        ];
        let result = format_table(&columns, &rows);
        let expected = "\
┌───┬───┐
│ a │ b │
├───┼───┤
│ 1 │ 2 │
│ 3 │ 4 │
└───┴───┘
";
        assert_eq!(result, expected);
    }

    #[test]
    fn test_format_table_null_value() {
        let columns = vec!["name".to_string(), "status".to_string()];
        let rows = vec![vec![Value::String("alice".to_string()), Value::Null]];
        let result = format_table(&columns, &rows);
        assert!(result.contains("NULL"));
    }
}
