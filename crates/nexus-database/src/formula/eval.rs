//! Formula evaluator: walks the AST and produces a [`FormulaValue`].

use crate::error::{DatabaseError, Result};
use crate::formula::ast::{BinaryOp, Expr, LiteralValue, UnaryOp};
use crate::formula::functions;

/// The value type system for formula evaluation.
#[derive(Debug, Clone)]
pub enum FormulaValue {
    /// No value.
    Null,
    /// A number.
    Number(f64),
    /// A string.
    String(String),
    /// A boolean.
    Boolean(bool),
    /// A date (stored as ISO-8601 string for simplicity).
    Date(String),
    /// An array of values.
    Array(Vec<FormulaValue>),
}

impl FormulaValue {
    /// Try to extract as a number.
    #[must_use]
    pub fn as_number(&self) -> Option<f64> {
        match self {
            Self::Number(n) => Some(*n),
            Self::String(s) => s.parse().ok(),
            Self::Boolean(b) => Some(if *b { 1.0 } else { 0.0 }),
            _ => None,
        }
    }

    /// Try to extract as a boolean (truthiness).
    #[must_use]
    pub fn is_truthy(&self) -> bool {
        match self {
            Self::Null => false,
            Self::Boolean(b) => *b,
            Self::Number(n) => *n != 0.0,
            Self::String(s) => !s.is_empty(),
            Self::Date(_) => true,
            Self::Array(a) => !a.is_empty(),
        }
    }

    /// Coerce to a display string.
    #[must_use]
    pub fn to_display_string(&self) -> String {
        match self {
            Self::Null => String::new(),
            #[allow(clippy::cast_possible_truncation)]
            Self::Number(n) => {
                if n.fract() == 0.0 && n.abs() < 1e15 {
                    format!("{}", *n as i64)
                } else {
                    n.to_string()
                }
            }
            Self::String(s) => s.clone(),
            Self::Boolean(b) => if *b { "true" } else { "false" }.to_string(),
            Self::Date(d) => d.clone(),
            Self::Array(a) => {
                let parts: Vec<String> = a.iter().map(Self::to_display_string).collect();
                parts.join(", ")
            }
        }
    }
}

impl std::fmt::Display for FormulaValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_display_string())
    }
}

impl PartialEq for FormulaValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Null, Self::Null) => true,
            (Self::Number(a), Self::Number(b)) => (a - b).abs() < f64::EPSILON,
            (Self::String(a), Self::String(b))
            | (Self::Date(a), Self::Date(b)) => a == b,
            (Self::Boolean(a), Self::Boolean(b)) => a == b,
            _ => false,
        }
    }
}

/// Context for formula evaluation: provides property values from a record.
pub struct EvalContext<'a> {
    /// The record's fields.
    pub fields: &'a serde_json::Map<String, serde_json::Value>,
}

/// Maximum AST recursion depth permitted during formula evaluation.
///
/// The evaluator recurses through `FunctionCall`, `BinaryOp`, `UnaryOp`,
/// and `If` nodes; deeply nested input (`if(c, if(c, if(...)))`) would
/// otherwise blow the stack. 64 is comfortably past anything a
/// hand-written spreadsheet formula reaches and well under the default
/// stack budget. See issue #78.
pub const MAX_RECURSION_DEPTH: usize = 64;

/// Evaluate an AST expression against a record context.
///
/// # Errors
///
/// Returns `DatabaseError::FormulaError` on type errors, unknown properties,
/// unknown functions, or AST nesting that exceeds [`MAX_RECURSION_DEPTH`].
pub fn evaluate(expr: &Expr, ctx: &EvalContext<'_>) -> Result<FormulaValue> {
    evaluate_inner(expr, ctx, 0)
}

fn evaluate_inner(
    expr: &Expr,
    ctx: &EvalContext<'_>,
    depth: usize,
) -> Result<FormulaValue> {
    if depth > MAX_RECURSION_DEPTH {
        return Err(DatabaseError::FormulaError {
            position: 0,
            message: format!(
                "formula recursion depth exceeded {MAX_RECURSION_DEPTH}"
            ),
        });
    }
    let next = depth + 1;
    match expr {
        Expr::Literal(lit) => Ok(match lit {
            LiteralValue::Number(n) => FormulaValue::Number(*n),
            LiteralValue::String(s) => FormulaValue::String(s.clone()),
            LiteralValue::Boolean(b) => FormulaValue::Boolean(*b),
            LiteralValue::Null => FormulaValue::Null,
        }),

        Expr::PropertyRef(name) => {
            let value = ctx.fields.get(name).unwrap_or(&serde_json::Value::Null);
            Ok(json_to_formula_value(value))
        }

        Expr::FunctionCall { name, args } => {
            let evaluated_args: Vec<FormulaValue> = args
                .iter()
                .map(|a| evaluate_inner(a, ctx, next))
                .collect::<Result<_>>()?;
            functions::call(name, &evaluated_args)
        }

        Expr::BinaryOp { left, op, right } => {
            let lval = evaluate_inner(left, ctx, next)?;
            let rval = evaluate_inner(right, ctx, next)?;
            eval_binary_op(&lval, *op, &rval)
        }

        Expr::UnaryOp { op, operand } => {
            let val = evaluate_inner(operand, ctx, next)?;
            match op {
                UnaryOp::Neg => {
                    let n = val.as_number().ok_or_else(|| DatabaseError::FormulaError {
                        position: 0,
                        message: "cannot negate a non-numeric value".to_string(),
                    })?;
                    Ok(FormulaValue::Number(-n))
                }
                UnaryOp::Not => Ok(FormulaValue::Boolean(!val.is_truthy())),
            }
        }

        Expr::If {
            condition,
            then_branch,
            else_branch,
        } => {
            let cond = evaluate_inner(condition, ctx, next)?;
            if cond.is_truthy() {
                evaluate_inner(then_branch, ctx, next)
            } else {
                evaluate_inner(else_branch, ctx, next)
            }
        }
    }
}

fn eval_binary_op(left: &FormulaValue, op: BinaryOp, right: &FormulaValue) -> Result<FormulaValue> {
    match op {
        // Arithmetic.
        BinaryOp::Add => {
            // String concatenation if either side is a string.
            if matches!(left, FormulaValue::String(_)) || matches!(right, FormulaValue::String(_)) {
                return Ok(FormulaValue::String(format!(
                    "{}{}",
                    left.to_display_string(),
                    right.to_display_string()
                )));
            }
            let (l, r) = num_pair(left, right, "+")?;
            Ok(FormulaValue::Number(l + r))
        }
        BinaryOp::Sub => {
            let (l, r) = num_pair(left, right, "-")?;
            Ok(FormulaValue::Number(l - r))
        }
        BinaryOp::Mul => {
            let (l, r) = num_pair(left, right, "*")?;
            Ok(FormulaValue::Number(l * r))
        }
        BinaryOp::Div => {
            let (l, r) = num_pair(left, right, "/")?;
            if r == 0.0 {
                return Err(DatabaseError::FormulaError {
                    position: 0,
                    message: "division by zero".to_string(),
                });
            }
            Ok(FormulaValue::Number(l / r))
        }
        BinaryOp::Mod => {
            let (l, r) = num_pair(left, right, "%")?;
            if r == 0.0 {
                return Err(DatabaseError::FormulaError {
                    position: 0,
                    message: "modulo by zero".to_string(),
                });
            }
            Ok(FormulaValue::Number(l % r))
        }

        // Comparison.
        BinaryOp::Eq => Ok(FormulaValue::Boolean(left == right)),
        BinaryOp::Neq => Ok(FormulaValue::Boolean(left != right)),
        BinaryOp::Lt => {
            let (l, r) = num_or_str_cmp(left, right);
            Ok(FormulaValue::Boolean(l < r))
        }
        BinaryOp::Gt => {
            let (l, r) = num_or_str_cmp(left, right);
            Ok(FormulaValue::Boolean(l > r))
        }
        BinaryOp::LtEq => {
            let (l, r) = num_or_str_cmp(left, right);
            Ok(FormulaValue::Boolean(l <= r))
        }
        BinaryOp::GtEq => {
            let (l, r) = num_or_str_cmp(left, right);
            Ok(FormulaValue::Boolean(l >= r))
        }

        // Logical.
        BinaryOp::And => Ok(FormulaValue::Boolean(
            left.is_truthy() && right.is_truthy(),
        )),
        BinaryOp::Or => Ok(FormulaValue::Boolean(
            left.is_truthy() || right.is_truthy(),
        )),
    }
}

fn num_pair(left: &FormulaValue, right: &FormulaValue, op: &str) -> Result<(f64, f64)> {
    let l = left.as_number().ok_or_else(|| DatabaseError::FormulaError {
        position: 0,
        message: format!("left operand of '{op}' is not a number"),
    })?;
    let r = right
        .as_number()
        .ok_or_else(|| DatabaseError::FormulaError {
            position: 0,
            message: format!("right operand of '{op}' is not a number"),
        })?;
    Ok((l, r))
}

/// Compare two values numerically if possible, otherwise lexicographically.
fn num_or_str_cmp(left: &FormulaValue, right: &FormulaValue) -> (String, String) {
    // Try numeric comparison via formatted strings that sort correctly.
    if let (Some(l), Some(r)) = (left.as_number(), right.as_number()) {
        // Use a format that sorts correctly for f64 values.
        return (format!("{l:020.10}"), format!("{r:020.10}"));
    }
    // Fall back to string comparison.
    (left.to_display_string(), right.to_display_string())
}

/// Convert a `serde_json::Value` to a `FormulaValue`.
fn json_to_formula_value(value: &serde_json::Value) -> FormulaValue {
    match value {
        serde_json::Value::Null => FormulaValue::Null,
        serde_json::Value::Bool(b) => FormulaValue::Boolean(*b),
        serde_json::Value::Number(n) => FormulaValue::Number(n.as_f64().unwrap_or(0.0)),
        serde_json::Value::String(s) => FormulaValue::String(s.clone()),
        serde_json::Value::Array(arr) => {
            FormulaValue::Array(arr.iter().map(json_to_formula_value).collect())
        }
        serde_json::Value::Object(_) => FormulaValue::String(value.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(input: &str) -> FormulaValue {
        let empty = serde_json::Map::new();
        let tokens = crate::formula::token::tokenize(input).unwrap();
        let ast = crate::formula::parser::parse(&tokens).unwrap();
        evaluate(&ast, &EvalContext { fields: &empty }).unwrap()
    }

    fn eval_with_fields(input: &str, fields: &serde_json::Map<String, serde_json::Value>) -> FormulaValue {
        let tokens = crate::formula::token::tokenize(input).unwrap();
        let ast = crate::formula::parser::parse(&tokens).unwrap();
        evaluate(&ast, &EvalContext { fields }).unwrap()
    }

    #[test]
    fn literal_number() {
        assert_eq!(eval("42"), FormulaValue::Number(42.0));
    }

    #[test]
    fn literal_string() {
        assert_eq!(eval(r#""hello""#), FormulaValue::String("hello".to_string()));
    }

    #[test]
    fn arithmetic() {
        assert_eq!(eval("2 + 3 * 4"), FormulaValue::Number(14.0));
    }

    #[test]
    fn string_concatenation() {
        assert_eq!(
            eval(r#""hello" + " " + "world""#),
            FormulaValue::String("hello world".to_string())
        );
    }

    #[test]
    fn comparison() {
        assert_eq!(eval("5 > 3"), FormulaValue::Boolean(true));
        assert_eq!(eval("5 < 3"), FormulaValue::Boolean(false));
        assert_eq!(eval("5 == 5"), FormulaValue::Boolean(true));
        assert_eq!(eval("5 != 5"), FormulaValue::Boolean(false));
    }

    #[test]
    fn logical_operators() {
        assert_eq!(eval("true and false"), FormulaValue::Boolean(false));
        assert_eq!(eval("true or false"), FormulaValue::Boolean(true));
        assert_eq!(eval("not true"), FormulaValue::Boolean(false));
    }

    #[test]
    fn if_expression() {
        assert_eq!(eval(r#"if(true, "yes", "no")"#), FormulaValue::String("yes".to_string()));
        assert_eq!(eval(r#"if(false, "yes", "no")"#), FormulaValue::String("no".to_string()));
    }

    #[test]
    fn property_lookup() {
        let mut fields = serde_json::Map::new();
        fields.insert("status".to_string(), serde_json::json!("done"));
        fields.insert("priority".to_string(), serde_json::json!(5));

        assert_eq!(
            eval_with_fields(r#"prop("status")"#, &fields),
            FormulaValue::String("done".to_string())
        );
        assert_eq!(
            eval_with_fields(r#"prop("priority")"#, &fields),
            FormulaValue::Number(5.0)
        );
    }

    #[test]
    fn complex_formula() {
        let mut fields = serde_json::Map::new();
        fields.insert("priority".to_string(), serde_json::json!(4));

        let result = eval_with_fields(
            r#"if(prop("priority") > 3, "High", "Low")"#,
            &fields,
        );
        assert_eq!(result, FormulaValue::String("High".to_string()));
    }

    #[test]
    fn division_by_zero() {
        let empty = serde_json::Map::new();
        let tokens = crate::formula::token::tokenize("1 / 0").unwrap();
        let ast = crate::formula::parser::parse(&tokens).unwrap();
        assert!(evaluate(&ast, &EvalContext { fields: &empty }).is_err());
    }

    #[test]
    fn unary_negation() {
        assert_eq!(eval("-5"), FormulaValue::Number(-5.0));
    }

    #[test]
    fn missing_property_is_null() {
        assert_eq!(
            eval_with_fields(r#"prop("nonexistent")"#, &serde_json::Map::new()),
            FormulaValue::Null
        );
    }

    #[test]
    fn null_is_falsy() {
        let mut fields = serde_json::Map::new();
        fields.insert("x".to_string(), serde_json::Value::Null);
        let result = eval_with_fields(r#"if(prop("x"), "yes", "no")"#, &fields);
        assert_eq!(result, FormulaValue::String("no".to_string()));
    }
}
