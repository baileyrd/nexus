//! Formula engine: parse and evaluate Notion-compatible expressions.
//!
//! # Usage
//!
//! ```ignore
//! use nexus_database::formula;
//!
//! let result = formula::evaluate(
//!     r#"if(prop("priority") > 3, "High", "Low")"#,
//!     &record.fields,
//! )?;
//! println!("{result}");
//! ```

pub mod ast;
pub mod eval;
pub mod functions;
pub mod parser;
pub mod token;

pub use eval::FormulaValue;

use crate::error::Result;

/// Evaluate a formula expression against a record's fields.
///
/// This is the main entry point for the formula engine. It tokenizes,
/// parses, and evaluates the expression in one call.
///
/// # Errors
///
/// Returns `DatabaseError::FormulaError` on parse or evaluation failure.
pub fn evaluate(
    expression: &str,
    fields: &serde_json::Map<String, serde_json::Value>,
) -> Result<FormulaValue> {
    let tokens = token::tokenize(expression)?;
    let ast = parser::parse(&tokens)?;
    eval::evaluate(&ast, &eval::EvalContext { fields })
}
