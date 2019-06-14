use crate::functions::expr::Expr;
use crate::functions::expr::arg::Arg;
use crate::functions::Error;

pub enum Thunk<'t> {
    Arg(Arg<'t>),
    Expr(Box<Expr<'t>>),
}

impl<'t> Thunk<'t> {
    pub fn eval(self) -> Result<Arg<'t>, Error> {
        match self {
            Self::Arg(o) => Ok(o),
            Self::Expr(e) => e.eval(),
        }
    }
}
