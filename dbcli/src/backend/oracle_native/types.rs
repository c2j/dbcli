use serde_json::Value;

pub(crate) fn format_oracle_row(row: &oracle::Row, col_count: usize) -> Vec<Value> {
    let mut result = Vec::with_capacity(col_count);
    for idx in 0..col_count {
        result.push(value_at(row, idx));
    }
    result
}

fn value_at(row: &oracle::Row, idx: usize) -> Value {
    if let Ok(v) = row.get::<_, String>(idx) {
        if let Ok(json_val) = serde_json::from_str(&v) {
            return json_val;
        }
        return Value::String(v);
    }
    if let Ok(v) = row.get::<_, i64>(idx) {
        return Value::Number(serde_json::Number::from(v));
    }
    if let Ok(v) = row.get::<_, f64>(idx) {
        if let Some(n) = serde_json::Number::from_f64(v) {
            return Value::Number(n);
        }
    }
    Value::Null
}
