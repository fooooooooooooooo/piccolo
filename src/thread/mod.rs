mod thread;
mod vm;

use thiserror::Error;

use crate::TypeError;

pub use self::{
    thread::{BadThreadMode, Thread, ThreadMode},
    vm::BinaryOperatorError,
};

#[derive(Debug, Copy, Clone, Error)]
pub enum VMError {
    #[error("{}", if *.0 {
        "operation expects variable stack"
    } else {
        "unexpected variable stack during operation"
    })]
    ExpectedVariableStack(bool),
    #[error(transparent)]
    BadType(#[from] TypeError),
    #[error("_ENV upvalue is only allowed on top-level closure")]
    BadEnvUpValue,
}
