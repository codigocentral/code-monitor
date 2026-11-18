//! `${VAR}` interpolation as docker compose does it, from the project's `.env`
//!
//! `image: app:${TAG}` only means something once `TAG` is resolved; leaving it
//! unresolved would turn every parameterised service into a false divergence.
//! The variables come from the project's `.env` (or the files the
//! `environment_file` label names). The shell environment of whoever ran
//! `compose up` is not knowable, so a variable the files do not define makes
//! its field unknown — even with a default: `${PORT:-8080}` says what compose
//! uses when nobody sets `PORT`, not what it used, and deployments driven from
//! CI routinely set it in the job's environment. Comparing the default there
//! reports drift that is not. Unknown fields are not compared.

use std::collections::HashMap;
use std::fmt;

/// The variables available to interpolation.
///
/// Values live here only while the compose files are parsed, and end up only
/// in the fields that are compared (image, ports, volumes, networks, restart).
/// They are never logged: `Debug` prints the count, not the content.
#[derive(Default)]
pub struct Variables(HashMap<String, String>);

impl fmt::Debug for Variables {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Variables({} defined)", self.0.len())
    }
}

/// Characters a variable name is made of.
fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Length, in bytes, of the variable name at the start of `s`.
fn name_len(s: &str) -> usize {
    s.find(|c: char| !is_name_char(c)).unwrap_or(s.len())
}

/// Strip matching quotes, or an inline ` #` comment from an unquoted value.
fn unquote(raw: &str) -> String {
    for quote in ['"', '\''] {
        if raw.len() >= 2 && raw.starts_with(quote) && raw.ends_with(quote) {
            return raw[1..raw.len() - 1].to_string();
        }
    }
    match raw.find(" #") {
        Some(i) => raw[..i].trim_end().to_string(),
        None => raw.to_string(),
    }
}

/// `KEY=VALUE`, `export KEY=VALUE`; comments and blank lines are skipped.
fn parse_env_line(line: &str) -> Option<(String, String)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let line = line.strip_prefix("export ").unwrap_or(line);
    let (key, raw) = line.split_once('=')?;
    let key = key.trim();
    if key.is_empty() || name_len(key) != key.len() {
        return None;
    }
    Some((key.to_string(), unquote(raw.trim())))
}

/// Byte offset of the `}` closing a `${` whose body starts `body`.
fn closing_brace(body: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (i, c) in body.char_indices() {
        match c {
            '{' => depth += 1,
            '}' if depth == 0 => return Some(i),
            '}' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// Split the part of `${NAME...}` after the name into operator and word.
///
/// The two-character operators come first, so `:-` is not read as `:`.
fn split_operator(op: &str) -> Option<(&'static str, &str)> {
    [":-", ":+", ":?", "-", "+", "?"]
        .iter()
        .find_map(|candidate| op.strip_prefix(candidate).map(|word| (*candidate, word)))
}

impl Variables {
    /// Read the variables of an env file.
    pub fn parse_env_file(content: &str) -> Self {
        Self(content.lines().filter_map(parse_env_line).collect())
    }

    /// Add the variables of a later file; later files win, as in compose.
    pub fn extend(&mut self, other: Variables) {
        self.0.extend(other.0);
    }

    #[cfg(test)]
    pub fn from_pairs(pairs: &[(&str, &str)]) -> Self {
        Self(
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        )
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.0.get(name).map(String::as_str)
    }

    /// Interpolate `input`.
    ///
    /// `Err` carries the name of the first variable the files do not define
    /// (or the unterminated expression), never a value.
    pub fn interpolate(&self, input: &str) -> Result<String, String> {
        let mut out = String::with_capacity(input.len());
        let mut rest = input;
        while let Some(pos) = rest.find('$') {
            out.push_str(&rest[..pos]);
            let after = &rest[pos + 1..];
            let (text, used) = self.expand(after)?;
            out.push_str(&text);
            rest = &after[used..];
        }
        out.push_str(rest);
        Ok(out)
    }

    /// Expand what follows a `$`: the text and how many bytes it consumed.
    fn expand(&self, after: &str) -> Result<(String, usize), String> {
        if after.starts_with('$') {
            return Ok(("$".to_string(), 1));
        }
        if let Some(body) = after.strip_prefix('{') {
            let end = closing_brace(body).ok_or_else(|| format!("${{{body}"))?;
            return Ok((self.braced(&body[..end])?, end + 2));
        }
        let len = name_len(after);
        if len == 0 {
            return Ok(("$".to_string(), 0));
        }
        let name = &after[..len];
        let value = self.get(name).ok_or_else(|| name.to_string())?;
        Ok((value.to_string(), len))
    }

    /// `NAME`, `NAME:-default`, `NAME-default`, `NAME:+alt`, `NAME+alt`,
    /// `NAME:?err`, `NAME?err`.
    ///
    /// A name the files do not define is unresolved whatever the operator:
    /// its default or alternative is only what compose would use if the
    /// deploying shell did not set it, which cannot be known from here.
    fn braced(&self, expr: &str) -> Result<String, String> {
        let (name, op) = expr.split_at(name_len(expr));
        let value = self.get(name);
        if value.is_none() {
            return Err(name.to_string());
        }
        let set = value.is_some();
        let non_empty = value.is_some_and(|v| !v.is_empty());
        let unresolved = || name.to_string();
        let Some((operator, word)) = split_operator(op) else {
            return match (op.is_empty(), value) {
                (true, Some(v)) => Ok(v.to_string()),
                _ => Err(unresolved()),
            };
        };
        match operator {
            ":-" if non_empty => Ok(value.unwrap_or_default().to_string()),
            "-" if set => Ok(value.unwrap_or_default().to_string()),
            ":-" | "-" => self.interpolate(word),
            ":+" if non_empty => self.interpolate(word),
            "+" if set => self.interpolate(word),
            ":+" | "+" => Ok(String::new()),
            ":?" if non_empty => Ok(value.unwrap_or_default().to_string()),
            "?" if set => Ok(value.unwrap_or_default().to_string()),
            _ => Err(unresolved()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars() -> Variables {
        Variables::from_pairs(&[("TAG", "1.5.2"), ("EMPTY", ""), ("PORT", "8080")])
    }

    #[test]
    fn test_plain_and_braced_variables() {
        let v = vars();
        assert_eq!(v.interpolate("app:${TAG}").unwrap(), "app:1.5.2");
        assert_eq!(v.interpolate("app:$TAG").unwrap(), "app:1.5.2");
        assert_eq!(v.interpolate("${PORT}:80").unwrap(), "8080:80");
        assert_eq!(v.interpolate("no variables").unwrap(), "no variables");
    }

    #[test]
    fn test_defaults() {
        let v = vars();
        assert_eq!(v.interpolate("app:${EMPTY:-1.0}").unwrap(), "app:1.0");
        assert_eq!(v.interpolate("app:${EMPTY-1.0}").unwrap(), "app:");
        assert_eq!(v.interpolate("app:${TAG:-1.0}").unwrap(), "app:1.5.2");
        // A default may itself use a variable
        assert_eq!(v.interpolate("${EMPTY:-${PORT}}:80").unwrap(), "8080:80");
    }

    #[test]
    fn test_default_of_an_undefined_variable_is_unknown() {
        // The deploying shell (often a CI job) may have set it: the default is
        // only what compose uses when nobody did, so it is not compared.
        let v = vars();
        assert_eq!(v.interpolate("app:${MISSING:-1.0}").unwrap_err(), "MISSING");
        assert_eq!(v.interpolate("app:${MISSING-1.0}").unwrap_err(), "MISSING");
        assert_eq!(v.interpolate("${MISSING:+set}").unwrap_err(), "MISSING");
        assert_eq!(v.interpolate("${MISSING+set}").unwrap_err(), "MISSING");
        assert_eq!(v.interpolate("${MISSING:-${PORT}}").unwrap_err(), "MISSING");
    }

    #[test]
    fn test_alternatives_and_required() {
        let v = vars();
        assert_eq!(v.interpolate("${TAG:+set}").unwrap(), "set");
        assert_eq!(v.interpolate("${EMPTY:+set}").unwrap(), "");
        assert_eq!(v.interpolate("${EMPTY+set}").unwrap(), "set");
        assert_eq!(v.interpolate("${TAG:?needed}").unwrap(), "1.5.2");
        assert_eq!(v.interpolate("${EMPTY:?needed}").unwrap_err(), "EMPTY");
        assert_eq!(v.interpolate("${EMPTY?needed}").unwrap(), "");
    }

    #[test]
    fn test_unresolved_names_the_variable_only() {
        let v = vars();
        assert_eq!(v.interpolate("app:${MISSING}").unwrap_err(), "MISSING");
        assert_eq!(v.interpolate("app:$MISSING").unwrap_err(), "MISSING");
        assert!(v.interpolate("app:${TAG").is_err());
    }

    #[test]
    fn test_escaped_dollar() {
        let v = vars();
        assert_eq!(v.interpolate("cost $$5").unwrap(), "cost $5");
        assert_eq!(v.interpolate("trailing $").unwrap(), "trailing $");
    }

    #[test]
    fn test_env_file_parsing() {
        let v = Variables::parse_env_file(
            "# comment\n\nTAG=1.5.2\nexport PORT=8080\nQUOTED=\"a b\"\nSINGLE='c'\nINLINE=x # note\nbad line\n=nokey\n",
        );
        assert_eq!(v.interpolate("$TAG $PORT").unwrap(), "1.5.2 8080");
        assert_eq!(v.interpolate("${QUOTED}|${SINGLE}").unwrap(), "a b|c");
        assert_eq!(v.interpolate("${INLINE}").unwrap(), "x");
    }

    #[test]
    fn test_later_file_wins() {
        let mut v = Variables::parse_env_file("TAG=1.0\nPORT=1");
        v.extend(Variables::parse_env_file("TAG=2.0"));
        assert_eq!(v.interpolate("$TAG:$PORT").unwrap(), "2.0:1");
    }

    #[test]
    fn test_debug_never_prints_values() {
        let printed = format!("{:?}", vars());
        assert!(!printed.contains("1.5.2"));
        assert!(printed.contains("3 defined"));
    }
}
