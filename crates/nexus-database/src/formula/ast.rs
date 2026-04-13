//! Abstract syntax tree types for the formula language.

/// An expression node in the formula AST.
#[derive(Debug, Clone)]
pub enum Expr {
    /// A literal value (number, string, boolean, null).
    Literal(LiteralValue),
    /// A property reference: `prop("field_name")`.
    PropertyRef(String),
    /// A function call: `concat(a, b)`, `upper(s)`, etc.
    FunctionCall {
        /// Function name (lowercase).
        name: String,
        /// Arguments.
        args: Vec<Expr>,
    },
    /// A binary operation: `a + b`, `a == b`, `a and b`.
    BinaryOp {
        /// Left operand.
        left: Box<Expr>,
        /// Operator.
        op: BinaryOp,
        /// Right operand.
        right: Box<Expr>,
    },
    /// A unary operation: `-x`, `not x`.
    UnaryOp {
        /// Operator.
        op: UnaryOp,
        /// Operand.
        operand: Box<Expr>,
    },
    /// A conditional: `if(cond, then, else)`.
    If {
        /// Condition.
        condition: Box<Expr>,
        /// Value when condition is truthy.
        then_branch: Box<Expr>,
        /// Value when condition is falsy.
        else_branch: Box<Expr>,
    },
}

/// A literal value.
#[derive(Debug, Clone)]
pub enum LiteralValue {
    /// A number.
    Number(f64),
    /// A string.
    String(String),
    /// A boolean.
    Boolean(bool),
    /// Null.
    Null,
}

/// Binary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    /// Addition.
    Add,
    /// Subtraction.
    Sub,
    /// Multiplication.
    Mul,
    /// Division.
    Div,
    /// Modulo.
    Mod,
    /// Equality.
    Eq,
    /// Inequality.
    Neq,
    /// Less than.
    Lt,
    /// Greater than.
    Gt,
    /// Less than or equal.
    LtEq,
    /// Greater than or equal.
    GtEq,
    /// Logical AND.
    And,
    /// Logical OR.
    Or,
}

/// Unary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    /// Arithmetic negation.
    Neg,
    /// Logical NOT.
    Not,
}
