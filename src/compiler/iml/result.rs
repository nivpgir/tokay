use crate::compiler::*;
use crate::reader::Offset;
use crate::value::{RefValue, Value};

/** Intermediate traversal result.

This enum is used to allow either for a value or ops created during the AST traversal in the compiler.
*/
#[derive(Debug)]
pub enum ImlResult {
    Empty,
    Value(ImlValue),
    Identifier(String, Option<Offset>),
    Ops(Vec<ImlOp>),
}

impl ImlResult {
    /** Turns a traversal result into a vector of operations;

    In case the result is a Value, it can either be called when calling with 0 arguments is possible,
    which is specified by the call flag.
    */
    pub fn into_ops(self, compiler: &mut Compiler, call: bool) -> Vec<ImlOp> {
        match self {
            ImlResult::Empty => Vec::new(),
            ImlResult::Value(value) => {
                vec![ImlOp::Op(if call && value.is_callable(false) {
                    if let ImlValue::Value(value) = &value {
                        if value.name() == "token" {
                            compiler.mark_consuming();
                        }
                    }

                    Op::CallStatic(compiler.define_value(value))
                } else {
                    let mut op = None;

                    if let ImlValue::Value(value) = &value {
                        op = match &*value.borrow() {
                            Value::Integer(0) => Some(Op::Push0),
                            Value::Integer(1) => Some(Op::Push1),
                            Value::Void => Some(Op::PushVoid),
                            Value::Null => Some(Op::PushNull),
                            Value::True => Some(Op::PushTrue),
                            Value::False => Some(Op::PushFalse),
                            _ => None,
                        }
                    }

                    op.unwrap_or_else(|| Op::LoadStatic(compiler.define_value(value)))
                })]
            }
            ImlResult::Identifier(name, offset) => {
                // In case there is a use of a known constant,
                // directly return its value as ImlResult.
                if let Some(value) = compiler.get_constant(&name) {
                    ImlResult::Value(value).into_ops(compiler, call)
                } else {
                    let usage = if call {
                        Usage::CallOrCopy { name, offset }
                    } else {
                        Usage::Load { name, offset }
                    };

                    usage.resolve_or_dispose(compiler)
                }
            }
            ImlResult::Ops(ops) => {
                // Filter any Op::Nop from the ops.
                ops.into_iter()
                    .filter(|op| !matches!(op, ImlOp::Nop))
                    .collect()
            }
        }
    }

    /** Returns a value to operate with or evaluate during compile-time.

    The function will only return Ok(Value) when static_expression_evaluation-feature
    is enabled, the ImlResult contains a value and this value is NOT a callable! */
    pub fn get_evaluable_value(&self) -> Result<RefValue, ()> {
        if cfg!(feature = "static_expression_evaluation") {
            if let ImlResult::Value(ImlValue::Value(value)) = self {
                if !value.is_callable(false) {
                    return Ok(value.clone().into());
                }
            }
        }

        Err(())
    }
}
