//! Integer arithmetic for `$((...))`.
//!
//! A small tokenizer plus a recursive-descent evaluator over `i64`. Supports
//! `+ - * / %`, unary `+ - !`, comparisons (`== != < <= > >=`), logical
//! `&& ||`, parentheses, and bare variable names (which resolve like `$name`,
//! with unset → `0`). Comparisons and logicals yield `1`/`0`, as in the shell.
//!
//! Not (yet) supported: assignment/increment inside the expression (`i++`,
//! `i=...`), bitwise operators, and `**`.

pub fn eval(src: &str) -> Result<i64, String> {
    let tokens = tokenize(src)?;
    let mut parser = Parser { tokens, pos: 0 };
    let value = parser.parse_or()?;
    if parser.pos != parser.tokens.len() {
        return Err("syntax error in arithmetic expression".into());
    }
    Ok(value)
}

#[derive(Debug, PartialEq, Eq, Clone)]
enum Tok {
    Num(i64),
    Ident(String),
    Op(&'static str),
}

fn tokenize(src: &str) -> Result<Vec<Tok>, String> {
    let mut toks = Vec::new();
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            let n: i64 = chars[start..i]
                .iter()
                .collect::<String>()
                .parse()
                .map_err(|_| "number too large".to_string())?;
            toks.push(Tok::Num(n));
        } else if c == '_' || c.is_ascii_alphabetic() {
            let start = i;
            while i < chars.len() && (chars[i] == '_' || chars[i].is_ascii_alphanumeric()) {
                i += 1;
            }
            toks.push(Tok::Ident(chars[start..i].iter().collect()));
        } else {
            // Two-character operators first, then single-character.
            let two = if i + 1 < chars.len() {
                Some([chars[i], chars[i + 1]])
            } else {
                None
            };
            let op2 = two.and_then(|p| match p {
                ['=', '='] => Some("=="),
                ['!', '='] => Some("!="),
                ['<', '='] => Some("<="),
                ['>', '='] => Some(">="),
                ['&', '&'] => Some("&&"),
                ['|', '|'] => Some("||"),
                _ => None,
            });
            if let Some(op) = op2 {
                toks.push(Tok::Op(op));
                i += 2;
                continue;
            }
            let op1 = match c {
                '+' => "+",
                '-' => "-",
                '*' => "*",
                '/' => "/",
                '%' => "%",
                '<' => "<",
                '>' => ">",
                '!' => "!",
                '(' => "(",
                ')' => ")",
                _ => return Err(format!("unexpected character `{c}` in arithmetic")),
            };
            toks.push(Tok::Op(op1));
            i += 1;
        }
    }

    Ok(toks)
}

struct Parser {
    tokens: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn peek_op(&self) -> Option<&'static str> {
        match self.tokens.get(self.pos) {
            Some(Tok::Op(op)) => Some(op),
            _ => None,
        }
    }

    /// Consume the current token if it is one of `ops`.
    fn eat(&mut self, ops: &[&str]) -> Option<&'static str> {
        if let Some(op) = self.peek_op()
            && ops.contains(&op)
        {
            self.pos += 1;
            return Some(op);
        }
        None
    }

    fn parse_or(&mut self) -> Result<i64, String> {
        let mut value = self.parse_and()?;
        while self.eat(&["||"]).is_some() {
            let rhs = self.parse_and()?;
            value = bool_int(value != 0 || rhs != 0);
        }
        Ok(value)
    }

    fn parse_and(&mut self) -> Result<i64, String> {
        let mut value = self.parse_equality()?;
        while self.eat(&["&&"]).is_some() {
            let rhs = self.parse_equality()?;
            value = bool_int(value != 0 && rhs != 0);
        }
        Ok(value)
    }

    fn parse_equality(&mut self) -> Result<i64, String> {
        let mut value = self.parse_relational()?;
        while let Some(op) = self.eat(&["==", "!="]) {
            let rhs = self.parse_relational()?;
            value = bool_int(if op == "==" {
                value == rhs
            } else {
                value != rhs
            });
        }
        Ok(value)
    }

    fn parse_relational(&mut self) -> Result<i64, String> {
        let mut value = self.parse_additive()?;
        while let Some(op) = self.eat(&["<", "<=", ">", ">="]) {
            let rhs = self.parse_additive()?;
            value = bool_int(match op {
                "<" => value < rhs,
                "<=" => value <= rhs,
                ">" => value > rhs,
                _ => value >= rhs,
            });
        }
        Ok(value)
    }

    fn parse_additive(&mut self) -> Result<i64, String> {
        let mut value = self.parse_term()?;
        while let Some(op) = self.eat(&["+", "-"]) {
            let rhs = self.parse_term()?;
            value = if op == "+" { value + rhs } else { value - rhs };
        }
        Ok(value)
    }

    fn parse_term(&mut self) -> Result<i64, String> {
        let mut value = self.parse_unary()?;
        while let Some(op) = self.eat(&["*", "/", "%"]) {
            let rhs = self.parse_unary()?;
            if (op == "/" || op == "%") && rhs == 0 {
                return Err("division by zero".into());
            }
            value = match op {
                "*" => value * rhs,
                "/" => value / rhs,
                _ => value % rhs,
            };
        }
        Ok(value)
    }

    fn parse_unary(&mut self) -> Result<i64, String> {
        if let Some(op) = self.eat(&["-", "+", "!"]) {
            let v = self.parse_unary()?;
            return Ok(match op {
                "-" => -v,
                "!" => bool_int(v == 0),
                _ => v,
            });
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<i64, String> {
        match self.tokens.get(self.pos).cloned() {
            Some(Tok::Num(n)) => {
                self.pos += 1;
                Ok(n)
            }
            Some(Tok::Ident(name)) => {
                self.pos += 1;
                var_value(&name)
            }
            Some(Tok::Op("(")) => {
                self.pos += 1;
                let v = self.parse_or()?;
                if self.eat(&[")"]).is_none() {
                    return Err("missing `)` in arithmetic".into());
                }
                Ok(v)
            }
            _ => Err("unexpected end of arithmetic expression".into()),
        }
    }
}

fn bool_int(b: bool) -> i64 {
    if b { 1 } else { 0 }
}

/// A variable's value as an integer: unset/empty → 0, non-numeric → error.
fn var_value(name: &str) -> Result<i64, String> {
    let raw = crate::vars::get(name)
        .or_else(|| std::env::var(name).ok())
        .unwrap_or_default();
    let s = raw.trim();
    if s.is_empty() {
        return Ok(0);
    }
    s.parse::<i64>()
        .map_err(|_| format!("`{name}`: not an integer (`{raw}`)"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic() {
        assert_eq!(eval("1 + 2 * 3"), Ok(7));
        assert_eq!(eval("(1 + 2) * 3"), Ok(9));
        assert_eq!(eval("10 / 3"), Ok(3));
        assert_eq!(eval("10 % 3"), Ok(1));
        assert_eq!(eval("-5 + 2"), Ok(-3));
        assert_eq!(eval("2 + 2 == 4"), Ok(1));
        assert_eq!(eval("3 < 2"), Ok(0));
        assert_eq!(eval("1 && 0"), Ok(0));
        assert_eq!(eval("1 || 0"), Ok(1));
        assert_eq!(eval("!0"), Ok(1));
    }

    #[test]
    fn variables() {
        crate::vars::set("RUSH_N", "41");
        assert_eq!(eval("RUSH_N + 1"), Ok(42));
        crate::vars::unset("RUSH_UNSET_ARITH");
        assert_eq!(eval("RUSH_UNSET_ARITH + 5"), Ok(5)); // unset → 0
    }

    #[test]
    fn errors() {
        assert!(eval("1 / 0").is_err());
        assert!(eval("1 +").is_err());
        assert!(eval("2 2").is_err());
    }
}
