// ─── PostgreSQL/GaussDB type-to-JSON conversion ──────────────────────────
//
// Ported from rust-opengauss/tools/gaussdb-mcp/src/output.rs with
// adaptations for hepta-dbcli's backend architecture.  Provides a single
// public entry-point `format_value_at` that dispatches 25+ PG types
// through the gaussdb type system, producing `serde_json::Value`.
//
// The `typed_or_raw` pattern ensures that unsupported column types produce
// visible hex-dump placeholders instead of silent NULL — the original bug
// that motivated rust-opengauss's full type handler.

use gaussdb::types::{FromSql, Type};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::error::Error;
use std::net::IpAddr;

// ─── RawBytes: universal byte-extractor ──────────────────────────────────

/// Generic byte-extractor that accepts ANY Postgres type so the dispatch
/// fallback can read raw bytes for types without an explicit handler.
/// Without this, `try_get::<Option<&[u8]>>` would only succeed for BYTEA
/// (OID 17) and silently drop every other unsupported type back to NULL.
struct RawBytes<'a>(Option<&'a [u8]>);

impl<'a> FromSql<'a> for RawBytes<'a> {
    fn from_sql(_: &Type, raw: &'a [u8]) -> Result<Self, Box<dyn Error + Sync + Send>> {
        Ok(RawBytes(Some(raw)))
    }

    fn from_sql_null(_: &Type) -> Result<Self, Box<dyn Error + Sync + Send>> {
        Ok(RawBytes(None))
    }

    fn accepts(_: &Type) -> bool {
        true
    }
}

// ─── Public entry point ──────────────────────────────────────────────────

/// Convert the value at column `idx` of `row` to a `serde_json::Value`,
/// dispatching on the column's declared PostgreSQL type.
pub(crate) fn format_value_at(row: &gaussdb::Row, idx: usize) -> Value {
    let ty = row.columns()[idx].type_();
    format_value_with_type(row, idx, ty)
}

// ─── Type dispatch (internal, type-explicit) ─────────────────────────────

fn format_value_with_type(row: &gaussdb::Row, idx: usize, ty: &Type) -> Value {
    match *ty {
        Type::VARCHAR | Type::TEXT | Type::BPCHAR | Type::NAME | Type::UNKNOWN => {
            typed_or_raw(row, idx, ty, |r, i| {
                match r.try_get::<_, Option<String>>(i) {
                    Ok(Some(s)) => Some(Value::String(s)),
                    Ok(None) => Some(Value::Null),
                    Err(_) => None,
                }
            })
        }
        Type::INT2 => typed_or_raw(row, idx, ty, |r, i| {
            r.try_get::<_, Option<i16>>(i)
                .ok()
                .map(|v| v.map(|x| json!(x)).unwrap_or(Value::Null))
        }),
        Type::INT4 | Type::OID | Type::REGPROC | Type::REGTYPE => {
            typed_or_raw(row, idx, ty, |r, i| {
                r.try_get::<_, Option<i32>>(i)
                    .ok()
                    .map(|v| v.map(|x| json!(x)).unwrap_or(Value::Null))
            })
        }
        Type::INT8 | Type::REGCLASS => typed_or_raw(row, idx, ty, |r, i| {
            r.try_get::<_, Option<i64>>(i)
                .ok()
                .map(|v| v.map(|x| json!(x)).unwrap_or(Value::Null))
        }),
        Type::FLOAT4 => typed_or_raw(row, idx, ty, |r, i| {
            r.try_get::<_, Option<f32>>(i)
                .ok()
                .map(|v| v.map(|x| json!(x)).unwrap_or(Value::Null))
        }),
        Type::FLOAT8 => typed_or_raw(row, idx, ty, |r, i| {
            r.try_get::<_, Option<f64>>(i)
                .ok()
                .map(|v| v.map(|x| json!(x)).unwrap_or(Value::Null))
        }),
        Type::NUMERIC => typed_or_raw(row, idx, ty, |r, i| {
            match r.try_get::<_, Option<Decimal>>(i) {
                Ok(Some(d)) => Some(decimal_to_json(d)),
                Ok(None) => Some(Value::Null),
                Err(_) => None,
            }
        }),
        Type::BOOL => typed_or_raw(row, idx, ty, |r, i| match r.try_get::<_, Option<bool>>(i) {
            Ok(Some(v)) => Some(json!(v)),
            Ok(None) => Some(Value::Null),
            Err(_) => None,
        }),
        Type::BYTEA => typed_or_raw(row, idx, ty, |r, i| {
            match r.try_get::<_, Option<&[u8]>>(i) {
                Ok(Some(b)) => Some(Value::String(format!("\\x{}", hex_bytes(b)))),
                Ok(None) => Some(Value::Null),
                Err(_) => None,
            }
        }),
        Type::UUID => typed_or_raw(row, idx, ty, |r, i| {
            match r.try_get::<_, Option<uuid::Uuid>>(i) {
                Ok(Some(u)) => Some(Value::String(u.to_string())),
                Ok(None) => Some(Value::Null),
                Err(_) => None,
            }
        }),
        Type::JSON | Type::JSONB => typed_or_raw(row, idx, ty, |r, i| {
            match r.try_get::<_, Option<Value>>(i) {
                Ok(Some(v)) => Some(v),
                Ok(None) => Some(Value::Null),
                Err(_) => None,
            }
        }),
        Type::TIMESTAMP | Type::TIMESTAMPTZ => typed_or_raw(row, idx, ty, |r, i| {
            match r.try_get::<_, Option<chrono::NaiveDateTime>>(i) {
                Ok(Some(v)) => Some(Value::String(v.to_string())),
                Ok(None) => Some(Value::Null),
                Err(_) => None,
            }
        }),
        Type::DATE => typed_or_raw(row, idx, ty, |r, i| {
            match r.try_get::<_, Option<chrono::NaiveDate>>(i) {
                Ok(Some(v)) => Some(Value::String(v.to_string())),
                Ok(None) => Some(Value::Null),
                Err(_) => None,
            }
        }),
        Type::TIME | Type::TIMETZ => typed_or_raw(row, idx, ty, |r, i| {
            match r.try_get::<_, Option<chrono::NaiveTime>>(i) {
                Ok(Some(v)) => Some(Value::String(v.to_string())),
                Ok(None) => Some(Value::Null),
                Err(_) => None,
            }
        }),
        Type::INET | Type::CIDR => typed_or_raw(row, idx, ty, |r, i| {
            match r.try_get::<_, Option<IpAddr>>(i) {
                Ok(Some(ip)) => Some(Value::String(ip.to_string())),
                Ok(None) => Some(Value::Null),
                Err(_) => None,
            }
        }),
        Type::MACADDR => typed_or_raw(row, idx, ty, |r, i| match r.try_get::<_, RawBytes>(i) {
            Ok(RawBytes(Some(b))) if b.len() == 6 => Some(Value::String(format!(
                "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                b[0], b[1], b[2], b[3], b[4], b[5]
            ))),
            Ok(RawBytes(None)) => Some(Value::Null),
            _ => None,
        }),
        Type::MACADDR8 => typed_or_raw(row, idx, ty, |r, i| match r.try_get::<_, RawBytes>(i) {
            Ok(RawBytes(Some(b))) if b.len() == 8 => Some(Value::String(format!(
                "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]
            ))),
            Ok(RawBytes(None)) => Some(Value::Null),
            _ => None,
        }),
        Type::INTERVAL => typed_or_raw(row, idx, ty, |r, i| match r.try_get::<_, RawBytes>(i) {
            Ok(RawBytes(Some(b))) => Some(Value::String(format_interval(b))),
            Ok(RawBytes(None)) => Some(Value::Null),
            _ => None,
        }),
        _ => raw_bytes_fallback(row, idx, ty),
    }
}

// ─── Dispatch helpers ────────────────────────────────────────────────────

/// Try the typed extraction first; if it returns `None` (typed `FromSql`
/// failed), fall through to a raw-bytes placeholder rather than dropping
/// the value to NULL.  This closes the last loophole through which silent
/// data loss could recur on a known type whose decoder happens to reject a
/// particular value (e.g. a NUMERIC column with scale > 28 that
/// rust_decimal cannot represent).
fn typed_or_raw<F>(row: &gaussdb::Row, idx: usize, ty: &Type, extract: F) -> Value
where
    F: Fn(&gaussdb::Row, usize) -> Option<Value>,
{
    match extract(row, idx) {
        Some(v) => v,
        None => raw_bytes_fallback(row, idx, ty),
    }
}

fn raw_bytes_fallback(row: &gaussdb::Row, idx: usize, ty: &Type) -> Value {
    match row.try_get::<_, RawBytes>(idx) {
        Ok(RawBytes(Some(b))) => {
            tracing::warn!(
                type_name = ty.name(),
                column_index = idx,
                bytes_len = b.len(),
                "unsupported column type, emitting hex fallback"
            );
            Value::String(format_unsupported_type(ty.name(), b))
        }
        Ok(RawBytes(None)) => Value::Null,
        Err(_) => Value::Null,
    }
}

// ─── NUMERIC / Decimal rounding ──────────────────────────────────────────

/// Render a NUMERIC `Decimal` as a JSON value.
///
/// Prefers `Number` when the value fits in `i64`/`u64`/`f64` without loss;
/// falls back to `String` for high-precision values that JSON numbers
/// cannot represent exactly (fractional with total significant digits
/// greater than 15, which exceeds IEEE 754 double's ~15.95 decimal-digit
/// precision; or integers outside the i64/u64 range).
fn decimal_to_json(d: Decimal) -> Value {
    // Integer fast-paths: i64/u64 give exact JSON Number representation
    // for the full machine-integer range regardless of digit count.
    if d.is_integer() {
        if let Some(i) = d.to_i64() {
            return json!(i);
        }
        if let Some(u) = d.to_u64() {
            return json!(u);
        }
        return Value::String(d.to_string());
    }
    // Fractional: guard f64 precision. Total significant digits
    // (integer digits + scale) > 15 exceeds IEEE 754 double precision
    // and would silently round-trip lose precision via f64.
    let int_digits = integer_digit_count(&d);
    let total_digits = int_digits + d.scale() as usize;
    if total_digits > 15 {
        return Value::String(d.to_string());
    }
    if let Some(f) = d.to_f64() {
        if let Some(n) = serde_json::Number::from_f64(f) {
            return Value::Number(n);
        }
    }
    Value::String(d.to_string())
}

fn integer_digit_count(d: &Decimal) -> usize {
    if d.is_zero() {
        return 1;
    }
    let s = d.abs().to_string();
    let int_part = s.split('.').next().unwrap_or("");
    int_part.len()
}

// ─── INTERVAL binary format ──────────────────────────────────────────────

/// PostgreSQL INTERVAL binary layout: i64 microseconds + i32 days +
/// i32 months, all big-endian.  Format mirrors psql's pretty-printing.
fn format_interval(b: &[u8]) -> String {
    if b.len() != 16 {
        return format!("<malformed interval: {} bytes>", b.len());
    }
    let mut buf = [0u8; 16];
    buf.copy_from_slice(b);
    let micros = i64::from_be_bytes(buf[0..8].try_into().unwrap());
    let days = i32::from_be_bytes(buf[8..12].try_into().unwrap());
    let months = i32::from_be_bytes(buf[12..16].try_into().unwrap());

    let mut parts: Vec<String> = Vec::new();
    if months != 0 {
        let years = months / 12;
        let mons = months % 12;
        if years != 0 {
            parts.push(format!("{} years", years));
        }
        if mons != 0 {
            parts.push(format!("{} mons", mons));
        }
    }
    if days != 0 {
        parts.push(format!("{} days", days));
    }
    if micros != 0 {
        let total_secs = micros.div_euclid(1_000_000);
        let frac = micros.rem_euclid(1_000_000).abs();
        let h = total_secs / 3600;
        let m = (total_secs % 3600) / 60;
        let s = total_secs % 60;
        let time_part = if frac > 0 {
            format!("{:02}:{:02}:{:02}.{:06}", h.abs(), m.abs(), s.abs(), frac)
        } else {
            format!("{:02}:{:02}:{:02}", h.abs(), m.abs(), s.abs())
        };
        parts.push(time_part);
    }
    if parts.is_empty() {
        "00:00:00".to_string()
    } else {
        parts.join(" ")
    }
}

// ─── Utilities ───────────────────────────────────────────────────────────

/// Build a visible placeholder for unsupported column types so silent data
/// loss (the previous behaviour, returning `Null`) cannot recur.
fn format_unsupported_type(type_name: &str, bytes: &[u8]) -> String {
    format!("<unsupported type {}>: \\x{}", type_name, hex_bytes(bytes))
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut result = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        result.push_str(&format!("{:02x}", b));
    }
    result
}

// ─── Unit tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    // ── decimal_to_json ──────────────────────────────────────────────

    #[test]
    fn test_decimal_to_json_positive_integer() {
        let d = Decimal::from_str("3950").unwrap();
        let v = decimal_to_json(d);
        assert_eq!(v, json!(3950i64));
    }

    #[test]
    fn test_decimal_to_json_negative_integer() {
        let d = Decimal::from_str("-100").unwrap();
        let v = decimal_to_json(d);
        assert_eq!(v, json!(-100i64));
    }

    #[test]
    fn test_decimal_to_json_fraction_fits_f64() {
        let d = Decimal::from_str("3950.123456").unwrap();
        let v = decimal_to_json(d);
        let expected = serde_json::Number::from_f64(3950.123456).unwrap();
        assert_eq!(v, Value::Number(expected));
    }

    #[test]
    fn test_decimal_to_json_beyond_u64_falls_back_to_string() {
        let d = Decimal::from_str("18446744073709551616").unwrap();
        let v = decimal_to_json(d);
        assert_eq!(v, Value::String("18446744073709551616".to_string()));
    }

    #[test]
    fn test_decimal_to_json_negative_beyond_i64_falls_back_to_string() {
        let d = Decimal::from_str("-99999999999999999999").unwrap();
        let v = decimal_to_json(d);
        assert_eq!(v, Value::String("-99999999999999999999".to_string()));
    }

    #[test]
    fn test_decimal_to_json_high_precision_fraction_falls_back_to_string() {
        let d = Decimal::from_str("1.123456789012345").unwrap();
        let v = decimal_to_json(d);
        match v {
            Value::String(s) => assert_eq!(s, "1.123456789012345"),
            other => panic!("expected String fallback, got {other:?}"),
        }
    }

    #[test]
    fn test_decimal_to_json_integer_with_trailing_zeros_preserves_precision() {
        let d = Decimal::from_str("10000000000000000.1").unwrap();
        let v = decimal_to_json(d);
        match v {
            Value::String(s) => assert_eq!(s, "10000000000000000.1"),
            other => panic!("expected String fallback, got {other:?}"),
        }
    }

    // ── integer_digit_count ──────────────────────────────────────────

    #[test]
    fn test_integer_digit_count_no_trailing_zero_stripping() {
        assert_eq!(integer_digit_count(&Decimal::from_str("1000").unwrap()), 4);
        assert_eq!(
            integer_digit_count(&Decimal::from_str("1000000.5").unwrap()),
            7
        );
        assert_eq!(integer_digit_count(&Decimal::from_str("1.5").unwrap()), 1);
        assert_eq!(integer_digit_count(&Decimal::from_str("0.5").unwrap()), 1);
    }

    // ── format_unsupported_type ──────────────────────────────────────

    #[test]
    fn test_format_unsupported_type_visible() {
        let s = format_unsupported_type("hstore", &[0x01, 0x02, 0xff]);
        assert!(s.contains("hstore"));
        assert!(s.contains("0102ff"));
    }

    // ── format_interval ──────────────────────────────────────────────

    #[test]
    fn test_format_interval_zero() {
        let bytes = [0u8; 16];
        assert_eq!(format_interval(&bytes), "00:00:00");
    }

    #[test]
    fn test_format_interval_one_day() {
        let mut bytes = [0u8; 16];
        bytes[8..12].copy_from_slice(&1i32.to_be_bytes());
        assert_eq!(format_interval(&bytes), "1 days");
    }

    #[test]
    fn test_format_interval_hours_minutes_seconds() {
        let micros: i64 = (2 * 3600 + 3 * 60 + 4) * 1_000_000;
        let mut bytes = [0u8; 16];
        bytes[0..8].copy_from_slice(&micros.to_be_bytes());
        assert_eq!(format_interval(&bytes), "02:03:04");
    }

    #[test]
    fn test_format_interval_malformed() {
        let bytes = [0u8; 8];
        let s = format_interval(&bytes);
        assert!(s.contains("malformed"));
    }

    // ── hex_bytes ────────────────────────────────────────────────────

    #[test]
    fn test_hex_bytes_empty() {
        assert_eq!(hex_bytes(&[]), "");
    }

    #[test]
    fn test_hex_bytes_simple() {
        assert_eq!(hex_bytes(&[0xde, 0xad, 0xbe, 0xef]), "deadbeef");
    }

    #[test]
    fn test_hex_bytes_leading_zeros() {
        assert_eq!(hex_bytes(&[0x00, 0x01, 0x02]), "000102");
    }
}
