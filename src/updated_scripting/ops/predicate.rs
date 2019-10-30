
use std::convert::TryFrom;
use std::convert::TryInto;
use std::cmp::Ordering;
use std::collections::BTreeMap;

use crate::util::Number;
use crate::metadata::types::MetaKey;
use crate::metadata::types::MetaVal;
use crate::updated_scripting::Error;
use crate::updated_scripting::util::IterableLike;

pub enum Predicate {
    AllEqual,
    IsEmpty,
    Not,
    All(Box<Predicate>),
    Any(Box<Predicate>),
    And(bool),
    Or(bool),
    Xor(bool),
    Eq(Number),
    Ne(Number),
    Lt(Number),
    Le(Number),
    Gt(Number),
    Ge(Number),
    HasKey_A(MetaKey),
    HasKey_B(BTreeMap<MetaKey, MetaVal>),
}

impl Predicate {
    pub fn test(&self, mv: &MetaVal) -> Result<bool, Error> {
        match self {
            &Self::AllEqual => IterableLike::try_from(mv)?.all_equal(),
            &Self::IsEmpty => IterableLike::try_from(mv)?.is_empty(),
            &Self::Not => Ok(!bool::try_from(mv).map_err(|_| Error::NotBoolean)?),
            &Self::All(ref pred) => {
                // TODO: Have `IterableLike::all()` accept this `Predicate` type and use it instead of trait.
                for v in IterableLike::try_from(mv)? {
                    if !pred.test((v?).as_ref())? { return Ok(false) }
                }

                Ok(true)
            },
            &Self::Any(ref pred) => {
                // TODO: Have `IterableLike::any()` accept this `Predicate` type and use it instead of trait.
                for v in IterableLike::try_from(mv)? {
                    if pred.test((v?).as_ref())? { return Ok(true) }
                }

                Ok(false)
            },
            &Self::And(b) => Ok(bool::try_from(mv).map_err(|_| Error::NotBoolean)? && b),
            &Self::Or(b) => Ok(bool::try_from(mv).map_err(|_| Error::NotBoolean)? || b),
            &Self::Xor(b) => Ok(bool::try_from(mv).map_err(|_| Error::NotBoolean)? ^ b),
            &Self::Eq(ref n) => Ok(Number::try_from(mv).map_err(|_| Error::NotNumeric)?.val_cmp(&n) == Ordering::Equal),
            &Self::Ne(ref n) => Ok(Number::try_from(mv).map_err(|_| Error::NotNumeric)?.val_cmp(&n) != Ordering::Equal),
            &Self::Lt(ref n) => Ok(Number::try_from(mv).map_err(|_| Error::NotNumeric)?.val_cmp(&n) == Ordering::Less),
            &Self::Le(ref n) => Ok(Number::try_from(mv).map_err(|_| Error::NotNumeric)?.val_cmp(&n) != Ordering::Greater),
            &Self::Gt(ref n) => Ok(Number::try_from(mv).map_err(|_| Error::NotNumeric)?.val_cmp(&n) == Ordering::Greater),
            &Self::Ge(ref n) => Ok(Number::try_from(mv).map_err(|_| Error::NotNumeric)?.val_cmp(&n) != Ordering::Less),
            &Self::HasKey_A(ref k) => {
                match mv {
                    &MetaVal::Map(ref m) => Ok(m.contains_key(k)),
                    _ => Err(Error::NotMapping),
                }
            },
            _ => Ok(false),
        }
    }
}

#[derive(Clone, Debug)]
pub enum Predicate2 {
    All,
    Any,
    And,
    Or,
    Xor,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    HasKey,
}