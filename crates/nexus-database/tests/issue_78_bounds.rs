//! Regression tests for issue #78 — unbounded parsing in `nexus-database`.
//!
//! 1. Formula evaluator's recursion depth was unbounded; deeply
//!    nested AST input would blow the stack.
//! 2. The `csv_import` IPC handler accepted arbitrary `Vec<u8>`
//!    contents and passed them to the CSV reader without an upfront
//!    size cap.

use nexus_database::formula::ast::{Expr, LiteralValue};
use nexus_database::formula::eval::{evaluate, EvalContext, MAX_RECURSION_DEPTH};
use nexus_database::DatabaseError;
use nexus_plugins::{CorePlugin, PluginError};

/// Build a hand-constructed `If` chain `n` levels deep — bypasses the
/// parser, which is itself recursive and would stack-overflow before
/// the evaluator's depth check could fire if we built via source text.
/// The audit's exploit shape is "pathological AST reaches the
/// evaluator"; the parser has its own bounding (input-text length
/// caps the AST size in practice) and is out of #78's scope.
fn nested_if_ast(depth: usize) -> Expr {
    let mut e = Expr::Literal(LiteralValue::Number(1.0));
    for _ in 0..depth {
        e = Expr::If {
            condition: Box::new(Expr::Literal(LiteralValue::Boolean(true))),
            then_branch: Box::new(e),
            else_branch: Box::new(Expr::Literal(LiteralValue::Number(0.0))),
        };
    }
    e
}

#[test]
fn formula_eval_rejects_pathological_recursion() {
    // 256 nesting levels — well past the 64-level cap.
    let ast = nested_if_ast(256);
    let fields = serde_json::Map::new();
    let ctx = EvalContext { fields: &fields };
    let err = evaluate(&ast, &ctx).expect_err("must reject deep recursion");
    match err {
        DatabaseError::FormulaError { message, .. } => {
            assert!(
                message.contains("recursion depth"),
                "expected recursion-depth error; got: {message}"
            );
        }
        other => panic!("expected FormulaError, got: {other:?}"),
    }
}

#[test]
fn formula_eval_accepts_bounded_recursion() {
    // Well under the cap — must still evaluate to the inner literal.
    let ast = nested_if_ast(MAX_RECURSION_DEPTH / 4);
    let fields = serde_json::Map::new();
    let ctx = EvalContext { fields: &fields };
    evaluate(&ast, &ctx).expect("bounded nesting must still evaluate");
}

#[test]
fn csv_import_rejects_oversize_input() {
    // Send a payload past the 10 MiB cap. We don't need the rows to
    // be valid CSV — the size gate fires before parsing.
    let mut plugin = nexus_database::DatabaseCorePlugin::new();
    let oversize = vec![b'a'; 11 * 1024 * 1024]; // 11 MiB
    let args = serde_json::json!({
        "csv_bytes": oversize,
        "field_names": ["x"],
        "has_header": false,
    });
    let err = plugin
        .dispatch(nexus_database::core_plugin::HANDLER_CSV_IMPORT, &args)
        .expect_err("must reject 11 MiB input");
    match err {
        PluginError::ExecutionFailed { reason, .. } => {
            assert!(
                reason.contains("max is") && reason.contains("bytes"),
                "expected size-cap rejection; got: {reason}"
            );
        }
        other => panic!("expected ExecutionFailed, got: {other:?}"),
    }
}
