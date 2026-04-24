//! Tokenizer for the formula language.

use crate::error::{DatabaseError, Result};

/// A token in the formula language.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    /// A numeric literal.
    NumberLit(f64),
    /// A string literal (contents only, no quotes).
    StringLit(String),
    /// `true` or `false`.
    BoolLit(bool),
    /// An identifier (function name or keyword).
    Ident(String),
    /// `+`
    Plus,
    /// `-`
    Minus,
    /// `*`
    Star,
    /// `/`
    Slash,
    /// `%`
    Percent,
    /// `==`
    Eq,
    /// `!=`
    Neq,
    /// `<`
    Lt,
    /// `>`
    Gt,
    /// `<=`
    LtEq,
    /// `>=`
    GtEq,
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `,`
    Comma,
    /// End of input.
    Eof,
}

/// A token with source position information.
#[derive(Debug, Clone)]
pub struct Spanned {
    /// The token.
    pub token: Token,
    /// Start position in the source string (byte offset).
    pub start: usize,
}

/// Tokenize a formula expression into a sequence of spanned tokens.
///
/// # Errors
///
/// Returns `DatabaseError::FormulaError` on unexpected characters or
/// unterminated strings.
#[allow(clippy::too_many_lines)]
pub fn tokenize(input: &str) -> Result<Vec<Spanned>> {
    let mut tokens = Vec::new();
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        let b = bytes[i];

        // Skip whitespace.
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }

        let start = i;

        // Single-character tokens.
        let simple = match b {
            b'+' => Some(Token::Plus),
            b'-' => Some(Token::Minus),
            b'*' => Some(Token::Star),
            b'/' => Some(Token::Slash),
            b'%' => Some(Token::Percent),
            b'(' => Some(Token::LParen),
            b')' => Some(Token::RParen),
            b',' => Some(Token::Comma),
            _ => None,
        };
        if let Some(tok) = simple {
            tokens.push(Spanned { token: tok, start });
            i += 1;
            continue;
        }

        // Two-character operators.
        if i + 1 < len {
            let two = &input[i..i + 2];
            let tok = match two {
                "==" => Some(Token::Eq),
                "!=" => Some(Token::Neq),
                "<=" => Some(Token::LtEq),
                ">=" => Some(Token::GtEq),
                _ => None,
            };
            if let Some(tok) = tok {
                tokens.push(Spanned { token: tok, start });
                i += 2;
                continue;
            }
        }

        // Single-character comparison operators (must come after two-char check).
        if b == b'<' {
            tokens.push(Spanned {
                token: Token::Lt,
                start,
            });
            i += 1;
            continue;
        }
        if b == b'>' {
            tokens.push(Spanned {
                token: Token::Gt,
                start,
            });
            i += 1;
            continue;
        }
        // Single `=` treated as `==` for user convenience.
        if b == b'=' {
            tokens.push(Spanned {
                token: Token::Eq,
                start,
            });
            i += 1;
            continue;
        }

        // String literals.
        if b == b'"' || b == b'\'' {
            let quote = b;
            i += 1;
            let mut s = String::new();
            while i < len && bytes[i] != quote {
                if bytes[i] == b'\\' && i + 1 < len {
                    i += 1;
                    match bytes[i] {
                        b'n' => s.push('\n'),
                        b't' => s.push('\t'),
                        b'\\' => s.push('\\'),
                        c if c == quote => s.push(char::from(quote)),
                        other => {
                            s.push('\\');
                            s.push(char::from(other));
                        }
                    }
                } else {
                    s.push(char::from(bytes[i]));
                }
                i += 1;
            }
            if i >= len {
                return Err(DatabaseError::FormulaError {
                    position: start,
                    message: "unterminated string literal".to_string(),
                });
            }
            i += 1; // skip closing quote
            tokens.push(Spanned {
                token: Token::StringLit(s),
                start,
            });
            continue;
        }

        // Numbers.
        if b.is_ascii_digit() || (b == b'.' && i + 1 < len && bytes[i + 1].is_ascii_digit()) {
            let num_start = i;
            while i < len && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                i += 1;
            }
            let num_str = &input[num_start..i];
            let n: f64 = num_str.parse().map_err(|_| DatabaseError::FormulaError {
                position: start,
                message: format!("invalid number: '{num_str}'"),
            })?;
            tokens.push(Spanned {
                token: Token::NumberLit(n),
                start,
            });
            continue;
        }

        // Identifiers and keywords.
        if b.is_ascii_alphabetic() || b == b'_' {
            let id_start = i;
            while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let word = &input[id_start..i];
            let token = match word {
                "true" => Token::BoolLit(true),
                "false" => Token::BoolLit(false),
                "and" => Token::Ident("and".to_string()),
                "or" => Token::Ident("or".to_string()),
                "not" => Token::Ident("not".to_string()),
                _ => Token::Ident(word.to_string()),
            };
            tokens.push(Spanned { token, start });
            continue;
        }

        return Err(DatabaseError::FormulaError {
            position: i,
            message: format!("unexpected character: '{}'", char::from(b)),
        });
    }

    tokens.push(Spanned {
        token: Token::Eof,
        start: len,
    });
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tok_types(input: &str) -> Vec<Token> {
        tokenize(input)
            .unwrap()
            .into_iter()
            .map(|s| s.token)
            .collect()
    }

    #[test]
    fn simple_arithmetic() {
        let tokens = tok_types("1 + 2 * 3");
        assert_eq!(
            tokens,
            vec![
                Token::NumberLit(1.0),
                Token::Plus,
                Token::NumberLit(2.0),
                Token::Star,
                Token::NumberLit(3.0),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn string_literal() {
        let tokens = tok_types(r#""hello world""#);
        assert_eq!(
            tokens,
            vec![Token::StringLit("hello world".to_string()), Token::Eof]
        );
    }

    #[test]
    fn string_with_escape() {
        let tokens = tok_types(r#""say \"hi\"""#);
        assert_eq!(
            tokens,
            vec![Token::StringLit("say \"hi\"".to_string()), Token::Eof]
        );
    }

    #[test]
    fn comparison_operators() {
        let tokens = tok_types("a == b != c <= d >= e < f > g");
        assert!(tokens.contains(&Token::Eq));
        assert!(tokens.contains(&Token::Neq));
        assert!(tokens.contains(&Token::LtEq));
        assert!(tokens.contains(&Token::GtEq));
        assert!(tokens.contains(&Token::Lt));
        assert!(tokens.contains(&Token::Gt));
    }

    #[test]
    fn function_call() {
        let tokens = tok_types("concat(a, b)");
        assert_eq!(
            tokens,
            vec![
                Token::Ident("concat".to_string()),
                Token::LParen,
                Token::Ident("a".to_string()),
                Token::Comma,
                Token::Ident("b".to_string()),
                Token::RParen,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn boolean_keywords() {
        let tokens = tok_types("true and false or not");
        assert_eq!(
            tokens,
            vec![
                Token::BoolLit(true),
                Token::Ident("and".to_string()),
                Token::BoolLit(false),
                Token::Ident("or".to_string()),
                Token::Ident("not".to_string()),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn unterminated_string_error() {
        assert!(tokenize(r#""unterminated"#).is_err());
    }

    #[test]
    fn single_equals_is_eq() {
        let tokens = tok_types("a = b");
        assert!(tokens.contains(&Token::Eq));
    }

    #[test]
    // 3.14 here is the literal we're lexing, not an approximation of PI.
    #[allow(clippy::approx_constant)]
    fn decimal_number() {
        let tokens = tok_types("3.14");
        assert_eq!(tokens[0], Token::NumberLit(3.14));
    }
}
