//! Minimal JSON parser for JSON-lines logs.
//!
//! Only what log records need: objects, arrays, strings with escapes, numbers,
//! booleans and null. It is deliberately strict about trailing characters,
//! because a half-parsed log line is worse than a reported parse error.

use crate::value::Value;

/// Parse a JSON value from the start of `input`.
///
/// Returns the value and the number of bytes consumed. Callers that parse a
/// whole line check that only whitespace remains.
pub fn parse_value(input: &str) -> Result<(Value, usize), String> {
    let mut parser = Parser::new(input);
    let value = parser.value()?;
    parser.whitespace();
    if parser.pos != parser.chars.len() {
        return Err(format!(
            "unexpected trailing data at position {}",
            parser.pos
        ));
    }
    Ok((value, parser.pos))
}

/// Parse a single JSON object. Log lines are objects; a bare array or scalar is
/// rejected so a malformed line does not silently become one empty record.
pub fn parse_object(input: &str) -> Result<Vec<(String, Value)>, String> {
    let (value, _) = parse_value(input)?;
    match value {
        Value::Object(entries) => Ok(entries),
        other => Err(format!("expected a JSON object, found {}", kind(&other))),
    }
}

fn kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::Str(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

struct Parser<'a> {
    chars: Vec<char>,
    pos: usize,
    _source: &'a str,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            chars: input.chars().collect(),
            pos: 0,
            _source: input,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn next(&mut self) -> Option<char> {
        let ch = self.peek();
        if ch.is_some() {
            self.pos += 1;
        }
        ch
    }

    fn whitespace(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t' | '\r' | '\n')) {
            self.pos += 1;
        }
    }

    fn expect(&mut self, wanted: char) -> Result<(), String> {
        match self.next() {
            Some(ch) if ch == wanted => Ok(()),
            Some(ch) => Err(format!(
                "expected '{wanted}', found '{ch}' at position {}",
                self.pos - 1
            )),
            None => Err(format!("expected '{wanted}', found end of input")),
        }
    }

    fn value(&mut self) -> Result<Value, String> {
        self.whitespace();
        match self.next() {
            Some('{') => self.object(),
            Some('[') => self.array(),
            Some('"') => self.string_body().map(Value::Str),
            Some('t') => self.literal("rue", Value::Bool(true)),
            Some('f') => self.literal("alse", Value::Bool(false)),
            Some('n') => self.literal("ull", Value::Null),
            Some(ch) if ch == '-' || ch.is_ascii_digit() => {
                self.pos -= 1;
                self.number()
            }
            Some(ch) => Err(format!("unexpected '{ch}' at position {}", self.pos - 1)),
            None => Err("unexpected end of input".to_string()),
        }
    }

    fn literal(&mut self, rest: &str, value: Value) -> Result<Value, String> {
        let start = self.pos;
        for wanted in rest.chars() {
            match self.next() {
                Some(ch) if ch == wanted => {}
                _ => {
                    return Err(format!(
                        "invalid literal at position {}",
                        start.saturating_sub(1)
                    ));
                }
            }
        }
        Ok(value)
    }

    fn object(&mut self) -> Result<Value, String> {
        let mut entries = Vec::new();
        self.whitespace();
        if self.peek() == Some('}') {
            self.pos += 1;
            return Ok(Value::Object(entries));
        }
        loop {
            self.whitespace();
            self.expect('"')?;
            let key = self.string_body()?;
            self.whitespace();
            self.expect(':')?;
            let value = self.value()?;
            entries.push((key, value));
            self.whitespace();
            match self.next() {
                Some(',') => continue,
                Some('}') => return Ok(Value::Object(entries)),
                Some(ch) => {
                    return Err(format!(
                        "expected ',' or '}}', found '{ch}' at position {}",
                        self.pos - 1
                    ));
                }
                None => return Err("unexpected end of object".to_string()),
            }
        }
    }

    fn array(&mut self) -> Result<Value, String> {
        let mut items = Vec::new();
        self.whitespace();
        if self.peek() == Some(']') {
            self.pos += 1;
            return Ok(Value::Array(items));
        }
        loop {
            items.push(self.value()?);
            self.whitespace();
            match self.next() {
                Some(',') => continue,
                Some(']') => return Ok(Value::Array(items)),
                Some(ch) => {
                    return Err(format!(
                        "expected ',' or ']', found '{ch}' at position {}",
                        self.pos - 1
                    ));
                }
                None => return Err("unexpected end of array".to_string()),
            }
        }
    }

    fn string_body(&mut self) -> Result<String, String> {
        let mut out = String::new();
        loop {
            match self.next() {
                Some('"') => return Ok(out),
                Some('\\') => match self.next() {
                    Some('"') => out.push('"'),
                    Some('\\') => out.push('\\'),
                    Some('/') => out.push('/'),
                    Some('b') => out.push('\u{0008}'),
                    Some('f') => out.push('\u{000c}'),
                    Some('n') => out.push('\n'),
                    Some('r') => out.push('\r'),
                    Some('t') => out.push('\t'),
                    Some('u') => {
                        let mut code = self.hex4()?;
                        if (0xD800..=0xDBFF).contains(&code) && self.peek() == Some('\\') {
                            self.pos += 1;
                            if self.peek() == Some('u') {
                                self.pos += 1;
                                let low = self.hex4()?;
                                if (0xDC00..=0xDFFF).contains(&low) {
                                    code = 0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00);
                                }
                            }
                        }
                        out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
                    }
                    Some(ch) => {
                        return Err(format!(
                            "invalid escape '\\{ch}' at position {}",
                            self.pos - 1
                        ));
                    }
                    None => return Err("unterminated escape".to_string()),
                },
                Some(ch) if (ch as u32) < 0x20 => {
                    return Err(format!(
                        "control character in string at position {}",
                        self.pos - 1
                    ));
                }
                Some(ch) => out.push(ch),
                None => return Err("unterminated string".to_string()),
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let mut code = 0u32;
        for _ in 0..4 {
            let ch = self
                .next()
                .ok_or_else(|| "truncated unicode escape".to_string())?;
            let digit = ch
                .to_digit(16)
                .ok_or_else(|| format!("invalid hex digit '{ch}'"))?;
            code = (code << 4) | digit;
        }
        Ok(code)
    }

    fn number(&mut self) -> Result<Value, String> {
        let start = self.pos;
        if self.peek() == Some('-') {
            self.pos += 1;
        }
        while matches!(self.peek(), Some(ch) if ch.is_ascii_digit()) {
            self.pos += 1;
        }
        if self.peek() == Some('.') {
            self.pos += 1;
            while matches!(self.peek(), Some(ch) if ch.is_ascii_digit()) {
                self.pos += 1;
            }
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some('+' | '-')) {
                self.pos += 1;
            }
            while matches!(self.peek(), Some(ch) if ch.is_ascii_digit()) {
                self.pos += 1;
            }
        }
        let text: String = self.chars[start..self.pos].iter().collect();
        text.parse::<f64>()
            .map(Value::Number)
            .map_err(|_| format!("invalid number '{text}'"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_flat_object() {
        let entries =
            parse_object(r#"{"level":"info","status":200,"ok":true,"note":null}"#).unwrap();
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[1].1, Value::Number(200.0));
        assert_eq!(entries[2].1, Value::Bool(true));
        assert_eq!(entries[3].1, Value::Null);
    }

    #[test]
    fn parses_nested_objects_and_arrays() {
        let (value, consumed) = parse_value(r#"{"http":{"status":200},"tags":["a","b"]}"#).unwrap();
        assert!(consumed > 10);
        match value {
            Value::Object(entries) => {
                assert_eq!(entries.len(), 2);
                assert!(matches!(entries[0].1, Value::Object(_)));
                assert!(matches!(entries[1].1, Value::Array(_)));
            }
            _ => panic!("expected object"),
        }
    }

    #[test]
    fn handles_string_escapes_and_unicode() {
        let entries =
            parse_object(r#"{"msg":"line\nnext \"quoted\" \u00e9 \ud83d\ude00"}"#).unwrap();
        assert_eq!(
            entries[0].1,
            Value::Str("line\nnext \"quoted\" é 😀".to_string())
        );
    }

    #[test]
    fn rejects_trailing_data_and_malformed_json() {
        assert!(parse_value(r#"{"a":1} extra"#).is_err());
        assert!(parse_object(r#"{"a":1"#).is_err());
        assert!(parse_object("[1,2]").is_err());
    }

    #[test]
    fn numbers_include_fractions_and_exponents() {
        let entries = parse_object(r#"{"a":-1.5,"b":2e3,"c":1E-2}"#).unwrap();
        assert_eq!(entries[0].1, Value::Number(-1.5));
        assert_eq!(entries[1].1, Value::Number(2000.0));
        assert_eq!(entries[2].1, Value::Number(0.01));
    }
}
