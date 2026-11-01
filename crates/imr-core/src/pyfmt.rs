//! Python's number and text conventions, which the exports, labels and rule matching rely on.

use chrono::{NaiveDateTime, Timelike};

/// Python `repr(float)`: shortest round-trip digits, `50000000.0`, `1e+16`, `1.5e-05`.
pub fn py_float_repr(f: f64) -> String {
    if f.is_nan() {
        return "nan".into();
    }
    if f.is_infinite() {
        return if f > 0.0 { "inf".into() } else { "-inf".into() };
    }
    if f == 0.0 {
        return if f.is_sign_negative() { "-0.0".into() } else { "0.0".into() };
    }
    let sci = format!("{f:e}");
    let (mantissa, exp) = sci.split_once('e').expect("exponent");
    let exp: i32 = exp.parse().expect("exponent number");
    let negative = mantissa.starts_with('-');
    let digits: String = mantissa.trim_start_matches('-').chars().filter(|c| *c != '.').collect();
    let sign = if negative { "-" } else { "" };
    if (-4..16).contains(&exp) {
        if exp >= 0 {
            let int_len = exp as usize + 1;
            if digits.len() <= int_len {
                format!("{sign}{digits}{}.0", "0".repeat(int_len - digits.len()))
            } else {
                format!("{sign}{}.{}", &digits[..int_len], &digits[int_len..])
            }
        } else {
            format!("{sign}0.{}{digits}", "0".repeat((-exp - 1) as usize))
        }
    } else {
        let mant = if digits.len() > 1 { format!("{}.{}", &digits[..1], &digits[1..]) } else { digits };
        let esign = if exp < 0 { '-' } else { '+' };
        format!("{sign}{mant}e{esign}{:02}", exp.abs())
    }
}

/// Python `float(text)`: surrounding whitespace, `inf` / `nan` / `infinity` in any case, and
/// underscores between digits are accepted.
pub fn py_float(text: &str) -> Option<f64> {
    let s = text.trim();
    if s.is_empty() {
        return None;
    }
    let body = s.trim_start_matches(['+', '-']);
    if body.len() + 1 < s.len() {
        return None; // more than one sign
    }
    let negative = s.starts_with('-');
    match body.to_ascii_lowercase().as_str() {
        "inf" | "infinity" => return Some(if negative { f64::NEG_INFINITY } else { f64::INFINITY }),
        "nan" => return Some(f64::NAN),
        _ => {}
    }
    let cleaned;
    let s = if s.contains('_') {
        let chars: Vec<char> = s.chars().collect();
        for (i, c) in chars.iter().enumerate() {
            if *c == '_' {
                let before = i.checked_sub(1).and_then(|j| chars.get(j));
                let after = chars.get(i + 1);
                if !before.is_some_and(|c| c.is_ascii_digit()) || !after.is_some_and(|c| c.is_ascii_digit()) {
                    return None;
                }
            }
        }
        cleaned = s.replace('_', "");
        cleaned.as_str()
    } else {
        s
    };
    if !s.bytes().all(|b| b.is_ascii_digit() || matches!(b, b'+' | b'-' | b'.' | b'e' | b'E')) {
        return None;
    }
    s.parse::<f64>().ok()
}

/// pandas `to_numeric` of one string: a plain number (no thousands separators or percent signs).
pub fn pd_numeric(text: &str) -> Option<f64> {
    let s = text.trim();
    if s.contains('_') {
        return None;
    }
    py_float(s)
}

/// Python `str(Timestamp)`: `2024-01-05 13:00:00`, with microseconds when there are any.
pub fn py_str_datetime(d: &NaiveDateTime) -> String {
    if d.nanosecond() == 0 {
        d.format("%Y-%m-%d %H:%M:%S").to_string()
    } else {
        d.format("%Y-%m-%d %H:%M:%S%.6f").to_string()
    }
}

/// Python `repr(str)`: single quotes unless the text has a single quote and no double quote.
pub fn py_repr_str(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7f => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// Python `str.casefold()` (full Unicode case folding: `Straße` -> `strasse`).
pub fn casefold(s: &str) -> String {
    caseless::default_case_fold_str(s)
}

/// Python `html.escape(s, quote=True)`.
pub fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            c => out.push(c),
        }
    }
    out
}

/// Escape every non-ASCII character as `\uXXXX`, as Python's `json.dumps` does by default.
/// Non-ASCII can only occur inside JSON strings, so this is safe on a whole document.
pub fn json_ensure_ascii(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_ascii() {
            out.push(c);
        } else {
            let mut buf = [0u16; 2];
            for unit in c.encode_utf16(&mut buf) {
                out.push_str(&format!("\\u{unit:04x}"));
            }
        }
    }
    out
}

/// Python `format(x, "e")` with `prec` digits after the point: `6.01e+07`.
pub fn py_format_e(x: f64, prec: usize) -> String {
    if let Some(s) = non_finite(x) {
        return s;
    }
    let s = format!("{x:.prec$e}");
    let (mant, exp) = s.split_once('e').expect("exponent");
    let exp: i32 = exp.parse().expect("exponent number");
    format!("{mant}e{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs())
}

fn non_finite(x: f64) -> Option<String> {
    if x.is_nan() {
        Some("nan".into())
    } else if x.is_infinite() {
        Some(if x > 0.0 { "inf".into() } else { "-inf".into() })
    } else {
        None
    }
}

/// Python `format(x, "g")` with `prec` significant digits (`6` for a bare `:g`): `1.2e+07`, `0.5`, `100`.
pub fn py_format_g(x: f64, prec: usize) -> String {
    py_format_g_sep(x, prec, false)
}

/// Python `format(x, ",.{prec}g")`: as [`py_format_g`] with thousands separators in fixed notation.
pub fn py_format_g_sep(x: f64, prec: usize, thousands: bool) -> String {
    if let Some(s) = non_finite(x) {
        return s;
    }
    let prec = prec.max(1);
    if x == 0.0 {
        return if x.is_sign_negative() { "-0".into() } else { "0".into() };
    }
    let sci = format!("{x:.p$e}", p = prec - 1);
    let (_, exp) = sci.split_once('e').expect("exponent");
    let exp: i32 = exp.parse().expect("exponent number");
    if exp < -4 || exp >= prec as i32 {
        let s = py_format_e(x, prec - 1);
        let (mant, e) = s.split_once('e').expect("exponent");
        let mant = if mant.contains('.') { mant.trim_end_matches('0').trim_end_matches('.') } else { mant };
        return format!("{mant}e{e}");
    }
    let decimals = (prec as i32 - 1 - exp).max(0) as usize;
    let fixed = format!("{x:.decimals$}");
    let fixed = if fixed.contains('.') {
        fixed.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        fixed
    };
    if thousands { group_thousands(&fixed) } else { fixed }
}

/// Python `format(x, ".{decimals}f")`.
pub fn py_format_f(x: f64, decimals: usize) -> String {
    if let Some(s) = non_finite(x) {
        return s;
    }
    format!("{x:.decimals$}")
}

/// `1234567.5` -> `1,234,567.5` (Python's `,` format option).
fn group_thousands(s: &str) -> String {
    let (sign, rest) = s.strip_prefix('-').map_or(("", s), |r| ("-", r));
    let (int, frac) = rest.split_once('.').map_or((rest, None), |(i, f)| (i, Some(f)));
    let mut out = String::new();
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    match frac {
        Some(f) => format!("{sign}{out}.{f}"),
        None => format!("{sign}{out}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_repr() {
        for (f, s) in [
            (5e7, "50000000.0"),
            (1e16, "1e+16"),
            (1.5e-5, "1.5e-05"),
            (0.0001, "0.0001"),
            (123456789012345678.0, "1.2345678901234568e+17"),
            (-0.25, "-0.25"),
            (2.0, "2.0"),
            (0.1, "0.1"),
            (1e15, "1000000000000000.0"),
        ] {
            assert_eq!(py_float_repr(f), s);
        }
    }

    #[test]
    fn float_parse() {
        assert_eq!(py_float(" 5 "), Some(5.0));
        assert_eq!(py_float("1_000"), Some(1000.0));
        assert_eq!(py_float("1__0"), None);
        assert_eq!(py_float("Infinity"), Some(f64::INFINITY));
        assert!(py_float("nan").unwrap().is_nan());
        assert_eq!(py_float("1."), Some(1.0));
        assert_eq!(py_float("-.5"), Some(-0.5));
        assert_eq!(py_float("0x10"), None);
        assert_eq!(py_float("--1"), None);
    }

    #[test]
    fn python_formats() {
        assert_eq!(py_format_g(60110386.92, 6), "6.01104e+07");
        assert_eq!(py_format_g(3.0, 6), "3");
        assert_eq!(py_format_g(0.0001, 6), "0.0001");
        assert_eq!(py_format_g(0.00001, 6), "1e-05");
        assert_eq!(py_format_g(123456.0, 6), "123456");
        assert_eq!(py_format_g(1234567.0, 6), "1.23457e+06");
        assert_eq!(py_format_g_sep(12345.678, 4, true), "1.235e+04");
        assert_eq!(py_format_g_sep(1234.5678, 4, true), "1,235");
        assert_eq!(py_format_g_sep(-99999.0, 6, true), "-99,999");
        assert_eq!(py_format_e(60110386.92, 2), "6.01e+07");
        assert_eq!(py_format_e(0.000123, 0), "1e-04");
        assert_eq!(py_format_f(0.5 * 100.0, 0), "50");
        assert_eq!(py_format_f(2.5, 0), "2");
    }

    #[test]
    fn repr_and_escape() {
        assert_eq!(py_repr_str("a"), "'a'");
        assert_eq!(py_repr_str("it's"), "\"it's\"");
        assert_eq!(html_escape("<a href='x'>&\""), "&lt;a href=&#x27;x&#x27;&gt;&amp;&quot;");
        assert_eq!(json_ensure_ascii("a › b 😀"), "a \\u203a b \\ud83d\\ude00");
    }
}
