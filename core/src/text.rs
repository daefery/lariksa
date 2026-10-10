//! Small text helpers that reproduce the Python behaviour the app was tuned on.
//!
//! The notulen guardrails were calibrated against Python's string and rounding
//! semantics (``" ".join(s.split())``, banker's rounding, ``difflib``), so
//! these helpers keep the Rust port's output identical.

use serde_json::Value;

/// ``" ".join(text.split())``: collapse every run of whitespace to one space.
pub fn squash(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The first ``n`` characters (not bytes).
pub fn take_chars(text: &str, n: usize) -> String {
    text.chars().take(n).collect()
}

pub fn char_len(text: &str) -> usize {
    text.chars().count()
}

/// ``str(value or "")`` for a JSON value from the model or the UI.
pub fn val_str(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(false)) => String::new(),
        Some(Value::Bool(true)) => "True".into(),
        Some(Value::Number(n)) => {
            if n.as_f64() == Some(0.0) {
                String::new()
            } else {
                n.to_string()
            }
        }
        Some(other) => other.to_string(),
    }
}

/// Python truthiness of a JSON value.
pub fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// Python's ``round(x, digits)``: correctly rounded on the exact binary value,
/// ties to even. Rust's float formatting uses the same rule.
pub fn py_round(x: f64, digits: usize) -> f64 {
    format!("{x:.digits$}").parse().unwrap_or(x)
}

/// ``difflib.SequenceMatcher(None, a, b).ratio()`` over characters, autojunk included.
pub fn sequence_ratio(a: &str, b: &str) -> f64 {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let total = a.len() + b.len();
    if total == 0 {
        return 1.0;
    }
    2.0 * matching_chars(&a, &b) as f64 / total as f64
}

fn matching_chars(a: &[char], b: &[char]) -> usize {
    use std::collections::HashMap;
    // b2j: positions of each element of b, minus "popular" elements (autojunk).
    let mut b2j: HashMap<char, Vec<usize>> = HashMap::new();
    for (j, c) in b.iter().enumerate() {
        b2j.entry(*c).or_default().push(j);
    }
    if b.len() >= 200 {
        let limit = b.len() / 100 + 1;
        b2j.retain(|_, v| v.len() <= limit);
    }

    let longest = |alo: usize, ahi: usize, blo: usize, bhi: usize| -> (usize, usize, usize) {
        let (mut besti, mut bestj, mut best) = (alo, blo, 0usize);
        let mut j2len: HashMap<usize, usize> = HashMap::new();
        for (i, ai) in a.iter().enumerate().take(ahi).skip(alo) {
            let mut next: HashMap<usize, usize> = HashMap::new();
            if let Some(js) = b2j.get(ai) {
                for &j in js {
                    if j < blo {
                        continue;
                    }
                    if j >= bhi {
                        break;
                    }
                    let k = j.checked_sub(1).and_then(|p| j2len.get(&p)).copied().unwrap_or(0) + 1;
                    next.insert(j, k);
                    if k > best {
                        besti = i + 1 - k;
                        bestj = j + 1 - k;
                        best = k;
                    }
                }
            }
            j2len = next;
        }
        while besti > alo && bestj > blo && a[besti - 1] == b[bestj - 1] {
            besti -= 1;
            bestj -= 1;
            best += 1;
        }
        while besti + best < ahi && bestj + best < bhi && a[besti + best] == b[bestj + best] {
            best += 1;
        }
        (besti, bestj, best)
    };

    let mut total = 0;
    let mut queue = vec![(0, a.len(), 0, b.len())];
    while let Some((alo, ahi, blo, bhi)) = queue.pop() {
        let (i, j, k) = longest(alo, ahi, blo, bhi);
        if k > 0 {
            total += k;
            if alo < i && blo < j {
                queue.push((alo, i, blo, j));
            }
            if i + k < ahi && j + k < bhi {
                queue.push((i + k, ahi, j + k, bhi));
            }
        }
    }
    total
}

/// Random URL-safe token, like Python's ``secrets.token_urlsafe(n)``.
pub fn token_urlsafe(n: usize) -> String {
    let mut bytes = vec![0u8; n];
    getrandom::getrandom(&mut bytes).expect("OS random source");
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let v = chunk.iter().enumerate().fold(0u32, |acc, (i, b)| acc | (*b as u32) << (16 - 8 * i));
        let chars = chunk.len() + 1;
        for i in 0..chars {
            out.push(ABC[((v >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    out
}

pub fn random_hex(n_chars: usize) -> String {
    let mut bytes = vec![0u8; n_chars.div_ceil(2)];
    getrandom::getrandom(&mut bytes).expect("OS random source");
    bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()[..n_chars].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ratio_matches_difflib() {
        // Values computed with Python's difflib.SequenceMatcher.
        assert!((sequence_ratio("deadlinenya hari jumat ya tolong dicek", "deadline nya hari jumat ya tolong di cek") - 0.9743589743589743).abs() < 1e-9);
        assert!((sequence_ratio("abcd", "bcde") - 0.75).abs() < 1e-9);
        assert_eq!(sequence_ratio("", ""), 1.0);
        assert_eq!(sequence_ratio("abc", ""), 0.0);
    }

    #[test]
    fn ratio_autojunk_on_long_text() {
        let a = "a".repeat(150) + &"b".repeat(100);
        let b = "a".repeat(250);
        // Python: SequenceMatcher(None, a, b).ratio() == 0.6 (autojunk drops the popular 'a').
        assert!((sequence_ratio(&a, &b) - 0.6).abs() < 1e-9);
    }

    #[test]
    fn rounding_is_bankers() {
        assert_eq!(py_round(12.5, 0), 12.0);
        assert_eq!(py_round(13.5, 0), 14.0);
        assert_eq!(py_round(0.1234, 3), 0.123);
        assert_eq!(py_round(2.675, 2), 2.67);
        assert_eq!(py_round(0.375, 2), 0.38);
    }

    #[test]
    fn token_lengths() {
        assert_eq!(token_urlsafe(18).len(), 24);
        assert_eq!(token_urlsafe(24).len(), 32);
        assert_eq!(random_hex(6).len(), 6);
    }
}
