//! Recursive descent parser for the formula language.
//!
//! Operator precedence (lowest to highest):
//! 1. `or`
//! 2. `and`
//! 3. `==`, `!=`
//! 4. `<`, `>`, `<=`, `>=`
//! 5. `+`, `-`
//! 6. `*`, `/`, `%`
//! 7. Unary `-`, `not`
//! 8. Function calls, property refs, literals, parenthesized expressions

use crate::error::{DatabaseError, Result};
use crate::formula::ast::{BinaryOp, Expr, LiteralValue, UnaryOp};
use crate::formula::token::{Spanned, Token};

/// Parse a token stream into an AST.
///
/// # Errors
///
/// Returns `DatabaseError::FormulaError` on syntax errors.
pub fn parse(tokens: &[Spanned]) -> Result<Expr> {
    let mut parser = Parser { tokens, pos: 0 };
    let expr = parser.parse_or()?;
    if !parser.at_eof() {
        return Err(parser.error("unexpected token after expression"));
    }
    Ok(expr)
}

struct Parser<'a> {
    tokens: &'a [Spanned],
    pos: usize,
}

impl Parser<'_> {
    fn current(&self) -> &Token {
        &self.tokens[self.pos.min(self.tokens.len() - 1)].token
    }

    fn current_pos(&self) -> usize {
        self.tokens[self.pos.min(self.tokens.len() - 1)].start
    }

    fn advance(&mut self) -> &Token {
        let tok = &self.tokens[self.pos.min(self.tokens.len() - 1)].token;
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
        tok
    }

    fn at_eof(&self) -> bool {
        matches!(self.current(), Token::Eof)
    }

    fn expect(&mut self, expected: &Token) -> Result<()> {
        if self.current() == expected {
            self.advance();
            Ok(())
        } else {
            Err(self.error(&format!("expected {expected:?}, got {:?}", self.current())))
        }
    }

    fn error(&self, message: &str) -> DatabaseError {
        DatabaseError::FormulaError {
            position: self.current_pos(),
            message: message.to_string(),
        }
    }

    // ── Precedence levels ───────────────────────────────────────────────────

    fn parse_or(&mut self) -> Result<Expr> {
        let mut left = self.parse_and()?;
        while matches!(self.current(), Token::Ident(s) if s == "or") {
            self.advance();
            let right = self.parse_and()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op: BinaryOp::Or,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr> {
        let mut left = self.parse_equality()?;
        while matches!(self.current(), Token::Ident(s) if s == "and") {
            self.advance();
            let right = self.parse_equality()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op: BinaryOp::And,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_equality(&mut self) -> Result<Expr> {
        let mut left = self.parse_comparison()?;
        loop {
            let op = match self.current() {
                Token::Eq => BinaryOp::Eq,
                Token::Neq => BinaryOp::Neq,
                _ => break,
            };
            self.advance();
            let right = self.parse_comparison()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_comparison(&mut self) -> Result<Expr> {
        let mut left = self.parse_addition()?;
        loop {
            let op = match self.current() {
                Token::Lt => BinaryOp::Lt,
                Token::Gt => BinaryOp::Gt,
                Token::LtEq => BinaryOp::LtEq,
                Token::GtEq => BinaryOp::GtEq,
                _ => break,
            };
            self.advance();
            let right = self.parse_addition()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_addition(&mut self) -> Result<Expr> {
        let mut left = self.parse_multiplication()?;
        loop {
            let op = match self.current() {
                Token::Plus => BinaryOp::Add,
                Token::Minus => BinaryOp::Sub,
                _ => break,
            };
            self.advance();
            let right = self.parse_multiplication()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_multiplication(&mut self) -> Result<Expr> {
        let mut left = self.parse_unary()?;
        loop {
            let op = match self.current() {
                Token::Star => BinaryOp::Mul,
                Token::Slash => BinaryOp::Div,
                Token::Percent => BinaryOp::Mod,
                _ => break,
            };
            self.advance();
            let right = self.parse_unary()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr> {
        if matches!(self.current(), Token::Minus) {
            self.advance();
            let operand = self.parse_unary()?;
            return Ok(Expr::UnaryOp {
                op: UnaryOp::Neg,
                operand: Box::new(operand),
            });
        }
        if matches!(self.current(), Token::Ident(s) if s == "not") {
            self.advance();
            let operand = self.parse_unary()?;
            return Ok(Expr::UnaryOp {
                op: UnaryOp::Not,
                operand: Box::new(operand),
            });
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Expr> {
        match self.current().clone() {
            Token::NumberLit(n) => {
                self.advance();
                Ok(Expr::Literal(LiteralValue::Number(n)))
            }
            Token::StringLit(s) => {
                self.advance();
                Ok(Expr::Literal(LiteralValue::String(s)))
            }
            Token::BoolLit(b) => {
                self.advance();
                Ok(Expr::Literal(LiteralValue::Boolean(b)))
            }
            Token::Ident(name) => {
                self.advance();
                // Check for function call.
                if matches!(self.current(), Token::LParen) {
                    self.advance(); // consume '('
                    let args = self.parse_arg_list()?;
                    self.expect(&Token::RParen)?;

                    // Special forms.
                    match name.as_str() {
                        "prop" => {
                            if args.len() != 1 {
                                return Err(self.error("prop() requires exactly 1 argument"));
                            }
                            if let Expr::Literal(LiteralValue::String(field)) = &args[0] {
                                return Ok(Expr::PropertyRef(field.clone()));
                            }
                            return Err(self.error("prop() argument must be a string literal"));
                        }
                        "if" => {
                            if args.len() != 3 {
                                return Err(self.error("if() requires exactly 3 arguments"));
                            }
                            let mut args = args;
                            let else_branch = args.pop().unwrap();
                            let then_branch = args.pop().unwrap();
                            let condition = args.pop().unwrap();
                            return Ok(Expr::If {
                                condition: Box::new(condition),
                                then_branch: Box::new(then_branch),
                                else_branch: Box::new(else_branch),
                            });
                        }
                        _ => {}
                    }

                    Ok(Expr::FunctionCall { name, args })
                } else {
                    // Bare identifier treated as property reference (shorthand for prop("name")).
                    Ok(Expr::PropertyRef(name))
                }
            }
            Token::LParen => {
                self.advance();
                let expr = self.parse_or()?;
                self.expect(&Token::RParen)?;
                Ok(expr)
            }
            _ => Err(self.error(&format!("unexpected token: {:?}", self.current()))),
        }
    }

    fn parse_arg_list(&mut self) -> Result<Vec<Expr>> {
        let mut args = Vec::new();
        if matches!(self.current(), Token::RParen) {
            return Ok(args);
        }
        args.push(self.parse_or()?);
        while matches!(self.current(), Token::Comma) {
            self.advance();
            args.push(self.parse_or()?);
        }
        Ok(args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formula::token::tokenize;

    fn parse_str(input: &str) -> Expr {
        let tokens = tokenize(input).unwrap();
        parse(&tokens).unwrap()
    }

    #[test]
    fn literal_number() {
        let expr = parse_str("42");
        assert!(
            matches!(expr, Expr::Literal(LiteralValue::Number(n)) if (n - 42.0).abs() < f64::EPSILON)
        );
    }

    #[test]
    fn literal_string() {
        let expr = parse_str(r#""hello""#);
        assert!(matches!(expr, Expr::Literal(LiteralValue::String(s)) if s == "hello"));
    }

    #[test]
    fn literal_boolean() {
        let expr = parse_str("true");
        assert!(matches!(expr, Expr::Literal(LiteralValue::Boolean(true))));
    }

    #[test]
    fn property_ref_via_prop() {
        let expr = parse_str(r#"prop("status")"#);
        assert!(matches!(expr, Expr::PropertyRef(s) if s == "status"));
    }

    #[test]
    fn bare_identifier_as_property_ref() {
        let expr = parse_str("status");
        assert!(matches!(expr, Expr::PropertyRef(s) if s == "status"));
    }

    #[test]
    fn binary_addition() {
        let expr = parse_str("1 + 2");
        assert!(matches!(
            expr,
            Expr::BinaryOp {
                op: BinaryOp::Add,
                ..
            }
        ));
    }

    #[test]
    fn operator_precedence_mul_over_add() {
        // 1 + 2 * 3 should parse as 1 + (2 * 3)
        let expr = parse_str("1 + 2 * 3");
        match expr {
            Expr::BinaryOp {
                op: BinaryOp::Add,
                right,
                ..
            } => {
                assert!(matches!(
                    *right,
                    Expr::BinaryOp {
                        op: BinaryOp::Mul,
                        ..
                    }
                ));
            }
            _ => panic!("expected Add at top level"),
        }
    }

    #[test]
    fn parenthesized_expression() {
        // (1 + 2) * 3 should parse as (1 + 2) * 3
        let expr = parse_str("(1 + 2) * 3");
        match expr {
            Expr::BinaryOp {
                op: BinaryOp::Mul,
                left,
                ..
            } => {
                assert!(matches!(
                    *left,
                    Expr::BinaryOp {
                        op: BinaryOp::Add,
                        ..
                    }
                ));
            }
            _ => panic!("expected Mul at top level"),
        }
    }

    #[test]
    fn function_call() {
        let expr = parse_str("upper(\"hello\")");
        match expr {
            Expr::FunctionCall { name, args } => {
                assert_eq!(name, "upper");
                assert_eq!(args.len(), 1);
            }
            _ => panic!("expected FunctionCall"),
        }
    }

    #[test]
    fn if_expression() {
        let expr = parse_str(r#"if(status == "done", 1, 0)"#);
        assert!(matches!(expr, Expr::If { .. }));
    }

    #[test]
    fn unary_negation() {
        let expr = parse_str("-5");
        assert!(matches!(
            expr,
            Expr::UnaryOp {
                op: UnaryOp::Neg,
                ..
            }
        ));
    }

    #[test]
    fn unary_not() {
        let expr = parse_str("not true");
        assert!(matches!(
            expr,
            Expr::UnaryOp {
                op: UnaryOp::Not,
                ..
            }
        ));
    }

    #[test]
    fn logical_and_or() {
        let expr = parse_str("a and b or c");
        // Should parse as (a and b) or c
        assert!(matches!(
            expr,
            Expr::BinaryOp {
                op: BinaryOp::Or,
                ..
            }
        ));
    }

    #[test]
    fn comparison_operators() {
        let expr = parse_str("x > 5");
        assert!(matches!(
            expr,
            Expr::BinaryOp {
                op: BinaryOp::Gt,
                ..
            }
        ));
    }

    #[test]
    fn nested_function_calls() {
        let expr = parse_str(r#"concat(upper("a"), lower("B"))"#);
        match expr {
            Expr::FunctionCall { name, args } => {
                assert_eq!(name, "concat");
                assert_eq!(args.len(), 2);
            }
            _ => panic!("expected FunctionCall"),
        }
    }

    #[test]
    fn complex_formula() {
        // Should not panic.
        let _ = parse_str(r#"if(prop("priority") > 3, "High", "Low")"#);
    }

    #[test]
    fn error_on_unexpected_token() {
        let tokens = tokenize(") bad").unwrap();
        assert!(parse(&tokens).is_err());
    }
}
