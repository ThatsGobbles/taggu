use std::borrow::Cow;
use std::convert::TryFrom;
use std::convert::TryInto;

use crate::metadata::types::MetaVal;
use crate::functions::Error;
use crate::functions::util::value_producer::ValueProducer;
use crate::functions::util::value_producer::Flatten;
use crate::functions::util::value_producer::Dedup;
use crate::functions::util::value_producer::Unique;
use crate::functions::util::value_producer::Filter;
use crate::functions::util::value_producer::Map;
use crate::functions::util::value_producer::StepBy;
use crate::functions::util::value_producer::Chain;
use crate::functions::util::value_producer::Zip;
use crate::functions::util::value_producer::Skip;
use crate::functions::util::value_producer::Take;
use crate::functions::util::value_producer::SkipWhile;
use crate::functions::util::value_producer::TakeWhile;
use crate::functions::operand::Operand;
use crate::functions::util::NumberLike;
use crate::functions::util::UnaryPred;
use crate::functions::util::UnaryConv;

#[derive(Clone, Copy)]
enum MinMax { Min, Max, }

#[derive(Clone, Copy)]
enum RevSort { Rev, Sort, }

#[derive(Clone, Copy)]
enum SumProd { Sum, Prod, }

#[derive(Clone, Copy)]
enum AllAny { All, Any, }

impl AllAny {
    fn target(self) -> bool {
        match self {
            Self::All => false,
            Self::Any => true,
        }
    }
}

/// Represents one of several different kinds of iterables, producing meta values.
pub enum IterableLike<'il> {
    Sequence(Vec<MetaVal<'il>>),
    Producer(ValueProducer<'il>),
}

impl<'il> IterableLike<'il> {
    pub fn is_lazy(&self) -> bool {
        match self {
            &Self::Sequence(..) => false,
            &Self::Producer(..) => true,
        }
    }

    pub fn is_eager(&self) -> bool {
        !self.is_lazy()
    }

    pub fn collect(self) -> Result<Vec<MetaVal<'il>>, Error> {
        match self {
            Self::Sequence(s) => Ok(s),
            Self::Producer(p) => p.collect::<Result<Vec<_>, _>>(),
        }
    }

    pub fn count(self) -> Result<usize, Error> {
        match self {
            Self::Sequence(s) => Ok(s.len()),
            Self::Producer(p) => {
                let mut c: usize = 0;
                for res_mv in p { res_mv?; c += 1; }
                Ok(c)
            },
        }
    }

    pub fn first(self) -> Result<MetaVal<'il>, Error> {
        match self {
            Self::Sequence(s) => s.into_iter().next().ok_or(Error::EmptySequence),
            Self::Producer(p) => p.into_iter().next().ok_or(Error::EmptyProducer)?,
        }
    }

    pub fn last(self) -> Result<MetaVal<'il>, Error> {
        match self {
            Self::Sequence(s) => s.into_iter().last().ok_or(Error::EmptySequence),
            Self::Producer(p) => {
                let mut last = None;
                for res_mv in p { last = Some(res_mv?); }
                last.ok_or(Error::EmptyProducer)
            },
        }
    }

    fn min_in_max_in(self, flag: MinMax) -> Result<NumberLike, Error> {
        let (new_p, err) = match self {
            Self::Sequence(s) => (ValueProducer::from(s), Error::EmptySequence),
            Self::Producer(p) => (p, Error::EmptyProducer),
        };

        let mut it = new_p.into_iter();
        match it.next() {
            None => Err(err),
            Some(first_res_mv) => {
                let mut target_nl: NumberLike = first_res_mv?.try_into()?;

                for res_mv in it {
                    let nl: NumberLike = res_mv?.try_into()?;
                    target_nl = match flag {
                        MinMax::Min => target_nl.val_min(nl),
                        MinMax::Max => target_nl.val_max(nl),
                    };
                }

                Ok(target_nl)
            }
        }
    }

    pub fn min_in(self) -> Result<NumberLike, Error> {
        self.min_in_max_in(MinMax::Min)
    }

    pub fn max_in(self) -> Result<NumberLike, Error> {
        self.min_in_max_in(MinMax::Max)
    }

    fn smart_sort_by<'mv>(a: &MetaVal<'mv>, b: &MetaVal<'mv>) -> std::cmp::Ordering {
        // Smooth over comparsions between integers and decimals.
        // TODO: Create a stable ordering for equal integers and decimals. (e.g. I(5) vs D(5.0))
        match (a, b) {
            (&MetaVal::Int(ref i), &MetaVal::Dec(ref d)) => {
                let i_d = (*i).into();
                // NOTE: Do this to avoid having to import other modules just for type inference.
                d.cmp(&i_d).reverse()
            },
            (&MetaVal::Dec(ref d), &MetaVal::Int(ref i)) => {
                let i_d = (*i).into();
                d.cmp(&i_d)
            },
            (na, nb) => na.cmp(&nb),
        }
    }

    fn rev_sort(self, flag: RevSort) -> Result<Vec<MetaVal<'il>>, Error> {
        let mut new_s = self.collect()?;

        match flag {
            RevSort::Rev => new_s.reverse(),
            RevSort::Sort => new_s.sort_by(Self::smart_sort_by),
        };

        Ok(new_s)
    }

    pub fn rev(self) -> Result<Vec<MetaVal<'il>>, Error> {
        self.rev_sort(RevSort::Rev)
    }

    pub fn sort(self) -> Result<Vec<MetaVal<'il>>, Error> {
        self.rev_sort(RevSort::Sort)
    }

    fn sum_prod(self, flag: SumProd) -> Result<NumberLike, Error> {
        let mut total = match flag {
            SumProd::Sum => NumberLike::Integer(0),
            SumProd::Prod => NumberLike::Integer(1),
        };

        let new_p = match self {
            Self::Sequence(s) => ValueProducer::from(s),
            Self::Producer(p) => p,
        };

        for res_mv in new_p {
            let nl: NumberLike = res_mv?.try_into()?;

            match flag {
                SumProd::Sum => { total += nl; },
                SumProd::Prod => { total *= nl; },
            };
        }

        Ok(total)
    }

    pub fn sum(self) -> Result<NumberLike, Error> {
        self.sum_prod(SumProd::Sum)
    }

    pub fn prod(self) -> Result<NumberLike, Error> {
        self.sum_prod(SumProd::Prod)
    }

    fn all_equal_agnostic<'a, I>(it: I) -> Result<bool, Error>
    where
        I: Iterator<Item = Result<Cow<'a, MetaVal<'a>>, Error>>,
    {
        let mut it = it.into_iter();
        Ok(match it.next() {
            None => true,
            Some(res_first_mv) => {
                let first_mv = res_first_mv?;
                for res_mv in it {
                    let mv = res_mv?;
                    if mv != first_mv { return Ok(false) }
                }

                true
            },
        })
    }

    fn all_equal_s<'a>(r_s: &Vec<MetaVal<'a>>) -> Result<bool, Error> {
        Self::all_equal_agnostic(r_s.into_iter().map(Cow::Borrowed).map(Result::Ok))
    }

    fn all_equal_pred<'a>(ref_mv: &MetaVal<'a>) -> Result<bool, Error> {
        // Conforms to the predicate interface.
        match ref_mv {
            &MetaVal::Seq(ref s) => Self::all_equal_s(&s),
            _ => Err(Error::NotSequence),
        }
    }

    fn all_equal_p<'a>(p: ValueProducer<'a>) -> Result<bool, Error> {
        Self::all_equal_agnostic(p.map(|res| res.map(Cow::Owned)))
    }

    pub fn all_equal(self) -> Result<bool, Error> {
        match self {
            Self::Sequence(ref s) => Self::all_equal_s(&s),
            Self::Producer(p) => Self::all_equal_p(p),
        }
    }

    pub fn flatten(self) -> Result<Self, Error> {
        Ok(match self {
            Self::Sequence(s) => Self::Sequence(Flatten::new(s.into()).collect::<Result<Vec<_>, _>>()?),
            Self::Producer(p) => Self::Producer(ValueProducer::Flatten(Flatten::new(p))),
        })
    }

    pub fn dedup(self) -> Result<Self, Error> {
        Ok(match self {
            Self::Sequence(s) => Self::Sequence(Dedup::new(s.into()).collect::<Result<Vec<_>, _>>()?),
            Self::Producer(p) => Self::Producer(ValueProducer::Dedup(Dedup::new(p))),
        })
    }

    pub fn unique(self) -> Result<Self, Error> {
        Ok(match self {
            Self::Sequence(s) => Self::Sequence(Unique::new(s.into()).collect::<Result<Vec<_>, _>>()?),
            Self::Producer(p) => Self::Producer(ValueProducer::Unique(Unique::new(p))),
        })
    }

    pub fn nth(self, n: usize) -> Result<MetaVal<'il>, Error> {
        match self {
            Self::Sequence(s) => s.into_iter().nth(n).ok_or(Error::OutOfBounds),
            Self::Producer(p) => {
                let mut i = 0;
                for res_mv in p {
                    let mv = res_mv?;

                    if i == n { return Ok(mv) }
                    else { i += 1; }
                }

                Err(Error::OutOfBounds)
            },
        }
    }

    fn all_any(self, u_pred: UnaryPred, flag: AllAny) -> Result<bool, Error> {
        let new_p = match self {
            Self::Sequence(s) => ValueProducer::from(s),
            Self::Producer(p) => p,
        };

        let target = flag.target();
        for res_mv in new_p {
            let mv = res_mv?;
            if u_pred(&mv)? == target { return Ok(target) }
        }

        Ok(!target)
    }

    pub fn all(self, u_pred: UnaryPred) -> Result<bool, Error> {
        self.all_any(u_pred, AllAny::All)
    }

    pub fn any(self, u_pred: UnaryPred) -> Result<bool, Error> {
        self.all_any(u_pred, AllAny::Any)
    }

    pub fn find(self, u_pred: UnaryPred) -> Result<MetaVal<'il>, Error> {
        let new_p = match self {
            Self::Sequence(s) => ValueProducer::from(s),
            Self::Producer(p) => p,
        };

        for res_mv in new_p {
            let mv = res_mv?;
            if u_pred(&mv)? { return Ok(mv) }
        }

        Err(Error::ItemNotFound)
    }

    pub fn position(self, u_pred: UnaryPred) -> Result<usize, Error> {
        let new_p = match self {
            Self::Sequence(s) => ValueProducer::from(s),
            Self::Producer(p) => p,
        };

        let mut i = 0;
        for res_mv in new_p {
            let mv = res_mv?;
            if u_pred(&mv)? { return Ok(i) }
            i += 1;
        }

        Err(Error::ItemNotFound)
    }

    pub fn filter(self, u_pred: UnaryPred) -> Result<Self, Error> {
        Ok(match self {
            Self::Sequence(s) => Self::Sequence(Filter::new(s.into(), u_pred).collect::<Result<Vec<_>, _>>()?),
            Self::Producer(p) => Self::Producer(ValueProducer::Filter(Filter::new(p, u_pred))),
        })
    }

    pub fn map(self, u_conv: UnaryConv) -> Result<Self, Error> {
        Ok(match self {
            Self::Sequence(s) => Self::Sequence(Map::new(s.into(), u_conv).collect::<Result<Vec<_>, _>>()?),
            Self::Producer(p) => Self::Producer(ValueProducer::Map(Map::new(p, u_conv))),
        })
    }

    pub fn step_by(self, step: usize) -> Result<Self, Error> {
        Ok(match self {
            Self::Sequence(s) => Self::Sequence(StepBy::new(s.into(), step)?.collect::<Result<Vec<_>, _>>()?),
            Self::Producer(p) => Self::Producer(ValueProducer::StepBy(StepBy::new(p, step)?)),
        })
    }

    pub fn chain(self, other: IterableLike<'il>) -> Self {
        let (new_p_a, new_p_b) = match (self, other) {
            (Self::Sequence(s_a), Self::Sequence(s_b)) => {
                let mut s_a = s_a;
                s_a.extend(s_b);
                return Self::Sequence(s_a)
            },
            (Self::Sequence(s_a), Self::Producer(p_b)) => (ValueProducer::from(s_a), p_b),
            (Self::Producer(p_a), Self::Sequence(s_b)) => (p_a, ValueProducer::from(s_b)),
            (Self::Producer(p_a), Self::Producer(p_b)) => (p_a, p_b),
        };

        Self::Producer(ValueProducer::Chain(Chain::new(new_p_a, new_p_b)))
    }

    pub fn zip(self, other: IterableLike<'il>) -> Result<Self, Error> {
        let collect_after = self.is_eager() && other.is_eager();
        let (new_p_a, new_p_b) = match (self, other) {
            (Self::Sequence(s_a), Self::Sequence(s_b)) => (ValueProducer::from(s_a), ValueProducer::from(s_b)),
            (Self::Sequence(s_a), Self::Producer(p_b)) => (ValueProducer::from(s_a), p_b),
            (Self::Producer(p_a), Self::Sequence(s_b)) => (p_a, ValueProducer::from(s_b)),
            (Self::Producer(p_a), Self::Producer(p_b)) => (p_a, p_b),
        };

        let ret_p = ValueProducer::Zip(Zip::new(new_p_a, new_p_b));

        Ok(match collect_after {
            true => Self::Sequence(ret_p.try_into()?),
            false => Self::Producer(ret_p),
        })
    }

    pub fn skip(self, n: usize) -> Self {
        match self {
            Self::Sequence(s) => {
                let mut s = s;
                if n >= s.len() { Self::Sequence(vec![]) }
                else { Self::Sequence(s.split_off(n)) }
            },
            Self::Producer(s) => Self::Producer(ValueProducer::Skip(Skip::new(s, n))),
        }
    }

    pub fn take(self, n: usize) -> Self {
        match self {
            Self::Sequence(s) => {
                let mut s = s;
                s.truncate(n);
                Self::Sequence(s)
            },
            Self::Producer(s) => Self::Producer(ValueProducer::Take(Take::new(s, n))),
        }
    }

    pub fn skip_while(self, u_pred: UnaryPred) -> Result<Self, Error> {
        let collect_after = self.is_eager();
        let p = ValueProducer::from(self);

        let ret_p = ValueProducer::SkipWhile(SkipWhile::new(p, u_pred));

        Ok(match collect_after {
            true => Self::Sequence(ret_p.try_into()?),
            false => Self::Producer(ret_p),
        })
    }

    pub fn take_while(self, u_pred: UnaryPred) -> Result<Self, Error> {
        let collect_after = self.is_eager();
        let p = ValueProducer::from(self);

        let ret_p = ValueProducer::TakeWhile(TakeWhile::new(p, u_pred));

        Ok(match collect_after {
            true => Self::Sequence(ret_p.try_into()?),
            false => Self::Producer(ret_p),
        })
    }

    // pub fn intersperse(self, mv: MetaVal<'il>) -> Self {
    // }

    // pub fn interleave(self, other: IterableLike<'il>) -> Self {
    // }
}

impl<'il> From<IterableLike<'il>> for Operand<'il> {
    fn from(il: IterableLike<'il>) -> Self {
        match il {
            IterableLike::Sequence(sequence) => Self::Value(MetaVal::Seq(sequence)),
            IterableLike::Producer(producer) => Self::Producer(producer),
        }
    }
}

impl<'il> TryFrom<Operand<'il>> for IterableLike<'il> {
    type Error = Error;

    fn try_from(value: Operand<'il>) -> Result<Self, Self::Error> {
        match value {
            Operand::Value(mv) => Self::try_from(mv),
            Operand::Producer(s) => Ok(Self::Producer(s)),
            _ => Err(Error::NotIterable),
        }
    }
}

impl<'il> TryFrom<MetaVal<'il>> for IterableLike<'il> {
    type Error = Error;

    fn try_from(value: MetaVal<'il>) -> Result<Self, Self::Error> {
        match value {
            MetaVal::Seq(s) => Ok(Self::Sequence(s)),
            _ => Err(Error::NotIterable),
        }
    }
}

impl<'il> From<IterableLike<'il>> for ValueProducer<'il> {
    fn from(il: IterableLike<'il>) -> Self {
        match il {
            IterableLike::Sequence(s) => s.into(),
            IterableLike::Producer(p) => p,
        }
    }
}

impl<'il> TryFrom<IterableLike<'il>> for Vec<MetaVal<'il>> {
    type Error = Error;

    fn try_from(il: IterableLike<'il>) -> Result<Self, Self::Error> {
        match il {
            IterableLike::Sequence(s) => Ok(s),
            IterableLike::Producer(p) => p.collect(),
        }
    }
}

impl<'il> From<Vec<MetaVal<'il>>> for IterableLike<'il> {
    fn from(s: Vec<MetaVal<'il>>) -> Self {
        IterableLike::Sequence(s)
    }
}

impl<'il> From<ValueProducer<'il>> for IterableLike<'il> {
    fn from(p: ValueProducer<'il>) -> Self {
        IterableLike::Producer(p)
    }
}

impl<'il> IntoIterator for IterableLike<'il> {
    type Item = Result<MetaVal<'il>, Error>;
    type IntoIter = IteratorLike<'il>;

    fn into_iter(self) -> Self::IntoIter {
        match self {
            Self::Sequence(s) => IteratorLike::Sequence(s.into_iter()),
            Self::Producer(s) => IteratorLike::Producer(s),
        }
    }
}

pub enum IteratorLike<'il> {
    Sequence(std::vec::IntoIter<MetaVal<'il>>),
    Producer(ValueProducer<'il>),
}

impl<'il> Iterator for IteratorLike<'il> {
    type Item = Result<MetaVal<'il>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            &mut Self::Sequence(ref mut it) => it.next().map(Result::Ok),
            &mut Self::Producer(ref mut it) => it.next(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::IterableLike as IL;

    use crate::test_util::TestUtil as TU;

    use crate::metadata::types::MetaVal;
    use crate::functions::Error;
    use crate::functions::ErrorKind;
    use crate::functions::util::value_producer::ValueProducer as VP;
    use crate::functions::util::NumberLike;

    #[test]
    fn test_collect() {
        let inputs_and_expected: Vec<(IL, Result<Vec<MetaVal>, ErrorKind>)> = vec![
            (
                vec![].into(),
                Ok(vec![]),
            ),
            (
                TU::core_nested_sequence().into(),
                Ok(TU::core_nested_sequence()),
            ),
            (
                VP::fixed(vec![]).into(),
                Ok(vec![]),
            ),
            (
                VP::fixed(TU::core_nested_sequence()).into(),
                Ok(TU::core_nested_sequence()),
            ),
            (
                VP::raw(vec![Err(Error::Sentinel)]).into(),
                Err(ErrorKind::Sentinel),
            ),
            (
                VP::raw(vec![Ok(TU::b(true)), Ok(TU::b(true)), Err(Error::Sentinel)]).into(),
                Err(ErrorKind::Sentinel),
            ),
        ];

        for (input, expected) in inputs_and_expected {
            let produced = input.collect().map_err(ErrorKind::from);
            assert_eq!(expected, produced);
        }
    }

    #[test]
    fn test_count() {
        let inputs_and_expected: Vec<(IL, Result<usize, ErrorKind>)> = vec![
            (
                vec![].into(),
                Ok(0),
            ),
            (
                TU::core_nested_sequence().into(),
                Ok(7),
            ),
            (
                VP::fixed(vec![]).into(),
                Ok(0),
            ),
            (
                VP::fixed(TU::core_nested_sequence()).into(),
                Ok(7),
            ),
            (
                VP::raw(vec![Err(Error::Sentinel)]).into(),
                Err(ErrorKind::Sentinel),
            ),
            (
                VP::raw(vec![Ok(TU::b(true)), Ok(TU::b(true)), Err(Error::Sentinel)]).into(),
                Err(ErrorKind::Sentinel),
            ),
        ];

        for (input, expected) in inputs_and_expected {
            let produced = input.count().map_err(ErrorKind::from);
            assert_eq!(expected, produced);
        }
    }

    #[test]
    fn test_first() {
        let inputs_and_expected: Vec<(IL, Result<MetaVal, ErrorKind>)> = vec![
            (
                vec![].into(),
                Err(ErrorKind::EmptySequence),
            ),
            (
                TU::core_nested_sequence().into(),
                Ok(TU::core_nested_sequence()[0].clone()),
            ),
            (
                VP::fixed(vec![]).into(),
                Err(ErrorKind::EmptyProducer),
            ),
            (
                VP::fixed(TU::core_nested_sequence()).into(),
                Ok(TU::core_nested_sequence()[0].clone()),
            ),
            (
                VP::raw(vec![Err(Error::Sentinel), Ok(TU::b(true)), Ok(TU::b(false))]).into(),
                Err(ErrorKind::Sentinel),
            ),
            (
                VP::raw(vec![Ok(TU::b(true)), Ok(TU::b(false)), Err(Error::Sentinel)]).into(),
                Ok(TU::b(true)),
            ),
        ];

        for (input, expected) in inputs_and_expected {
            let produced = input.first().map_err(ErrorKind::from);
            assert_eq!(expected, produced);
        }
    }

    #[test]
    fn test_last() {
        let inputs_and_expected: Vec<(IL, Result<MetaVal, ErrorKind>)> = vec![
            (
                vec![].into(),
                Err(ErrorKind::EmptySequence),
            ),
            (
                TU::core_nested_sequence().into(),
                Ok(TU::core_nested_sequence().pop().unwrap()),
            ),
            (
                VP::fixed(vec![]).into(),
                Err(ErrorKind::EmptyProducer),
            ),
            (
                VP::fixed(TU::core_nested_sequence()).into(),
                Ok(TU::core_nested_sequence().pop().unwrap()),
            ),
            (
                VP::raw(vec![Err(Error::Sentinel), Ok(TU::b(true)), Ok(TU::b(false))]).into(),
                Err(ErrorKind::Sentinel),
            ),
            (
                VP::raw(vec![Ok(TU::b(true)), Ok(TU::b(false)), Err(Error::Sentinel)]).into(),
                Err(ErrorKind::Sentinel),
            ),
        ];

        for (input, expected) in inputs_and_expected {
            let produced = input.last().map_err(ErrorKind::from);
            assert_eq!(expected, produced);
        }
    }

    #[test]
    fn test_min_in() {
        let inputs_and_expected: Vec<(IL, Result<NumberLike, ErrorKind>)> = vec![
            (
                vec![].into(),
                Err(ErrorKind::EmptySequence),
            ),
            (
                TU::core_number_sequence(2, false, true, false).into(),
                Ok(NumberLike::Integer(-2)),
            ),
            (
                TU::core_number_sequence(2, true, true, false).into(),
                Ok(NumberLike::Decimal(TU::d_raw(-25, 1))),
            ),
            (
                vec![TU::i(1)].into(),
                Ok(NumberLike::Integer(1)),
            ),
            (
                vec![TU::i(1), TU::b(true)].into(),
                Err(ErrorKind::NotNumeric),
            ),
            (
                VP::fixed(vec![]).into(),
                Err(ErrorKind::EmptyProducer),
            ),
            (
                VP::fixed(TU::core_number_sequence(2, false, true, false)).into(),
                Ok(NumberLike::Integer(-2)),
            ),
            (
                VP::fixed(TU::core_number_sequence(2, true, true, false)).into(),
                Ok(NumberLike::Decimal(TU::d_raw(-25, 1))),
            ),
            (
                VP::raw(vec![Ok(TU::i(1))]).into(),
                Ok(NumberLike::Integer(1)),
            ),
            (
                VP::raw(vec![Ok(TU::i(1)), Ok(TU::b(false))]).into(),
                Err(ErrorKind::NotNumeric),
            ),
            (
                VP::raw(vec![Ok(TU::i(1)), Err(Error::Sentinel)]).into(),
                Err(ErrorKind::Sentinel),
            ),
        ];

        for (input, expected) in inputs_and_expected {
            let produced = input.min_in().map_err(ErrorKind::from);
            assert_eq!(expected, produced);
        }
    }

    #[test]
    fn test_max_in() {
        let inputs_and_expected: Vec<(IL, Result<NumberLike, ErrorKind>)> = vec![
            (
                vec![].into(),
                Err(ErrorKind::EmptySequence),
            ),
            (
                TU::core_number_sequence(2, false, true, false).into(),
                Ok(NumberLike::Integer(2)),
            ),
            (
                TU::core_number_sequence(2, true, true, false).into(),
                Ok(NumberLike::Decimal(TU::d_raw(25, 1))),
            ),
            (
                vec![TU::i(1)].into(),
                Ok(NumberLike::Integer(1)),
            ),
            (
                vec![TU::i(1), TU::b(true)].into(),
                Err(ErrorKind::NotNumeric),
            ),
            (
                VP::fixed(vec![]).into(),
                Err(ErrorKind::EmptyProducer),
            ),
            (
                VP::fixed(TU::core_number_sequence(2, false, true, false)).into(),
                Ok(NumberLike::Integer(2)),
            ),
            (
                VP::fixed(TU::core_number_sequence(2, true, true, false)).into(),
                Ok(NumberLike::Decimal(TU::d_raw(25, 1))),
            ),
            (
                VP::raw(vec![Ok(TU::i(1))]).into(),
                Ok(NumberLike::Integer(1)),
            ),
            (
                VP::raw(vec![Ok(TU::i(1)), Ok(TU::b(false))]).into(),
                Err(ErrorKind::NotNumeric),
            ),
            (
                VP::raw(vec![Ok(TU::i(1)), Err(Error::Sentinel)]).into(),
                Err(ErrorKind::Sentinel),
            ),
        ];

        for (input, expected) in inputs_and_expected {
            let produced = input.max_in().map_err(ErrorKind::from);
            assert_eq!(expected, produced);
        }
    }

    #[test]
    fn test_rev() {
        let inputs_and_expected: Vec<(IL, Result<Vec<MetaVal>, ErrorKind>)> = vec![
            (
                vec![].into(),
                Ok(vec![]),
            ),
            (
                TU::core_nested_sequence().into(),
                Ok({ let mut s = TU::core_nested_sequence(); s.reverse(); s }),
            ),
            (
                vec![TU::i(1)].into(),
                Ok(vec![TU::i(1)]),
            ),
            (
                VP::fixed(vec![]).into(),
                Ok(vec![]),
            ),
            (
                VP::fixed(TU::core_nested_sequence()).into(),
                Ok({ let mut s = TU::core_nested_sequence(); s.reverse(); s }),
            ),
            (
                VP::raw(vec![Ok(TU::i(1))]).into(),
                Ok(vec![TU::i(1)]),
            ),
            (
                VP::raw(vec![Ok(TU::i(1)), Err(Error::Sentinel)]).into(),
                Err(ErrorKind::Sentinel),
            ),
        ];

        for (input, expected) in inputs_and_expected {
            let produced = input.rev().map_err(ErrorKind::from);
            assert_eq!(expected, produced);
        }
    }

    #[test]
    fn test_sort() {
        let inputs_and_expected: Vec<(IL, Result<Vec<MetaVal>, ErrorKind>)> = vec![
            (
                vec![].into(),
                Ok(vec![]),
            ),
            (
                TU::core_number_sequence(2, false, true, true).into(),
                Ok(vec![TU::i(-2), TU::d(-15, 1), TU::i(-1), TU::d(-5, 1), TU::i(0), TU::d(5, 1), TU::i(1), TU::d(15, 1), TU::i(2)]),
            ),
            (
                vec![TU::i(1)].into(),
                Ok(vec![TU::i(1)]),
            ),
            (
                VP::fixed(vec![]).into(),
                Ok(vec![]),
            ),
            (
                VP::fixed(TU::core_number_sequence(2, false, true, true)).into(),
                Ok(vec![TU::i(-2), TU::d(-15, 1), TU::i(-1), TU::d(-5, 1), TU::i(0), TU::d(5, 1), TU::i(1), TU::d(15, 1), TU::i(2)]),
            ),
            (
                VP::raw(vec![Ok(TU::i(1))]).into(),
                Ok(vec![TU::i(1)]),
            ),
            (
                VP::raw(vec![Ok(TU::i(1)), Err(Error::Sentinel)]).into(),
                Err(ErrorKind::Sentinel),
            ),
        ];

        for (input, expected) in inputs_and_expected {
            let produced = input.sort().map_err(ErrorKind::from);
            assert_eq!(expected, produced);
        }
    }

    #[test]
    fn test_sum() {
        let inputs_and_expected: Vec<(IL, Result<NumberLike, ErrorKind>)> = vec![
            (
                vec![].into(),
                Ok(NumberLike::Integer(0)),
            ),
            (
                TU::core_number_sequence(2, false, true, true).into(),
                Ok(NumberLike::Decimal(TU::d_raw(0, 0))),
            ),
            (
                vec![TU::i(-2), TU::i(3), TU::i(5), TU::i(7)].into(),
                Ok(NumberLike::Integer(13)),
            ),
            (
                vec![TU::i(-2), TU::i(3), TU::d(55, 1), TU::i(7)].into(),
                Ok(NumberLike::Decimal(TU::d_raw(135, 1))),
            ),
            (
                vec![TU::i(1)].into(),
                Ok(NumberLike::Integer(1)),
            ),
            (
                vec![TU::i(1), TU::b(true)].into(),
                Err(ErrorKind::NotNumeric),
            ),
            (
                VP::fixed(vec![]).into(),
                Ok(NumberLike::Integer(0)),
            ),
            (
                VP::fixed(TU::core_number_sequence(2, false, true, true)).into(),
                Ok(NumberLike::Decimal(TU::d_raw(0, 0))),
            ),
            (
                VP::raw(vec![Ok(TU::i(-2)), Ok(TU::i(3)), Ok(TU::i(5)), Ok(TU::i(7))]).into(),
                Ok(NumberLike::Integer(13)),
            ),
            (
                VP::raw(vec![Ok(TU::i(-2)), Ok(TU::i(3)), Ok(TU::d(55, 1)), Ok(TU::i(7))]).into(),
                Ok(NumberLike::Decimal(TU::d_raw(135, 1))),
            ),
            (
                VP::raw(vec![Ok(TU::i(1))]).into(),
                Ok(NumberLike::Integer(1)),
            ),
            (
                VP::raw(vec![Ok(TU::i(1)), Ok(TU::b(true))]).into(),
                Err(ErrorKind::NotNumeric),
            ),
            (
                VP::raw(vec![Ok(TU::i(1)), Err(Error::Sentinel)]).into(),
                Err(ErrorKind::Sentinel),
            ),
        ];

        for (input, expected) in inputs_and_expected {
            let produced = input.sum().map_err(ErrorKind::from);
            assert_eq!(expected, produced);
        }
    }

    #[test]
    fn test_prod() {
        let inputs_and_expected: Vec<(IL, Result<NumberLike, ErrorKind>)> = vec![
            (
                vec![].into(),
                Ok(NumberLike::Integer(1)),
            ),
            (
                TU::core_number_sequence(2, false, true, true).into(),
                Ok(NumberLike::Decimal(TU::d_raw(0, 0))),
            ),
            (
                TU::core_number_sequence(2, false, true, false).into(),
                Ok(NumberLike::Decimal(TU::d_raw(225, 2))),
            ),
            (
                vec![TU::i(-2), TU::i(3), TU::i(5), TU::i(7)].into(),
                Ok(NumberLike::Integer(-210)),
            ),
            (
                vec![TU::i(-2), TU::i(3), TU::d(55, 1), TU::i(7)].into(),
                Ok(NumberLike::Decimal(TU::d_raw(-231, 0))),
            ),
            (
                vec![TU::i(1)].into(),
                Ok(NumberLike::Integer(1)),
            ),
            (
                vec![TU::i(1), TU::b(true)].into(),
                Err(ErrorKind::NotNumeric),
            ),
            (
                VP::fixed(vec![]).into(),
                Ok(NumberLike::Integer(1)),
            ),
            (
                VP::fixed(TU::core_number_sequence(2, false, true, true)).into(),
                Ok(NumberLike::Decimal(TU::d_raw(0, 0))),
            ),
            (
                VP::fixed(TU::core_number_sequence(2, false, true, false)).into(),
                Ok(NumberLike::Decimal(TU::d_raw(225, 2))),
            ),
            (
                VP::raw(vec![Ok(TU::i(-2)), Ok(TU::i(3)), Ok(TU::i(5)), Ok(TU::i(7))]).into(),
                Ok(NumberLike::Integer(-210)),
            ),
            (
                VP::raw(vec![Ok(TU::i(-2)), Ok(TU::i(3)), Ok(TU::d(55, 1)), Ok(TU::i(7))]).into(),
                Ok(NumberLike::Decimal(TU::d_raw(-231, 0))),
            ),
            (
                VP::raw(vec![Ok(TU::i(1))]).into(),
                Ok(NumberLike::Integer(1)),
            ),
            (
                VP::raw(vec![Ok(TU::i(1)), Ok(TU::b(true))]).into(),
                Err(ErrorKind::NotNumeric),
            ),
            (
                VP::raw(vec![Ok(TU::i(1)), Err(Error::Sentinel)]).into(),
                Err(ErrorKind::Sentinel),
            ),
        ];

        for (input, expected) in inputs_and_expected {
            let produced = input.prod().map_err(ErrorKind::from);
            assert_eq!(expected, produced);
        }
    }

    #[test]
    fn test_all_equal() {
        let inputs_and_expected: Vec<(IL, Result<bool, ErrorKind>)> = vec![
            (
                vec![].into(),
                Ok(true),
            ),
            (
                vec![TU::i(1), TU::i(1), TU::i(1)].into(),
                Ok(true),
            ),
            (
                vec![TU::i(1), TU::i(1), TU::i(2)].into(),
                Ok(false),
            ),
            (
                vec![TU::i(1)].into(),
                Ok(true),
            ),
            (
                vec![TU::i(1), TU::b(true)].into(),
                Ok(false),
            ),
            (
                VP::fixed(vec![]).into(),
                Ok(true),
            ),
            (
                VP::raw(vec![Ok(TU::i(1)), Ok(TU::i(1)), Ok(TU::i(1))]).into(),
                Ok(true),
            ),
            (
                VP::raw(vec![Ok(TU::i(1)), Ok(TU::i(1)), Ok(TU::i(2))]).into(),
                Ok(false),
            ),
            (
                VP::raw(vec![Ok(TU::i(1))]).into(),
                Ok(true),
            ),
            (
                VP::raw(vec![Ok(TU::i(1)), Ok(TU::b(true))]).into(),
                Ok(false),
            ),
            (
                VP::raw(vec![Ok(TU::i(1)), Err(Error::Sentinel)]).into(),
                Err(ErrorKind::Sentinel),
            ),
        ];

        for (input, expected) in inputs_and_expected {
            let produced = input.all_equal().map_err(ErrorKind::from);
            assert_eq!(expected, produced);
        }
    }

    #[test]
    fn test_flatten() {
        let inputs_and_expected: Vec<(IL, Result<Vec<Result<MetaVal, ErrorKind>>, ErrorKind>)> = vec![
            (
                vec![].into(),
                Ok(vec![]),
            ),
            (
                TU::core_flat_sequence().into(),
                Ok(TU::core_flat_sequence().into_iter().map(Result::Ok).collect()),
            ),
            (
                TU::core_nested_sequence().into(),
                Ok({
                    let mut s = TU::core_flat_sequence();
                    s.extend(TU::core_flat_sequence());
                    s.push(TU::sample_flat_mapping());
                    s
                }.into_iter().map(Result::Ok).collect()),
            ),
            (
                VP::fixed(vec![]).into(),
                Ok(vec![]),
            ),
            (
                VP::fixed(TU::core_flat_sequence()).into(),
                Ok(TU::core_flat_sequence().into_iter().map(Result::Ok).collect()),
            ),
            (
                VP::fixed(TU::core_nested_sequence()).into(),
                Ok({
                    let mut s = TU::core_flat_sequence();
                    s.extend(TU::core_flat_sequence());
                    s.push(TU::sample_flat_mapping());
                    s
                }.into_iter().map(Result::Ok).collect()),
            ),
            (
                VP::raw(vec![Ok(TU::i(1)), Err(Error::Sentinel)]).into(),
                Ok(vec![Ok(TU::i(1)), Err(ErrorKind::Sentinel)]),
            ),
        ];

        for (input, expected) in inputs_and_expected {
            let produced = input.flatten()
                .map_err(ErrorKind::from)
                .map(|il| {
                    il.into_iter().map(|res| {
                        res.map_err(ErrorKind::from)
                    })
                    .collect::<Vec<_>>()
                })
            ;
            assert_eq!(expected, produced);
        }
    }

    #[test]
    fn test_dedup() {
        let inputs_and_expected: Vec<(IL, Result<Vec<Result<MetaVal, ErrorKind>>, ErrorKind>)> = vec![
            (
                vec![].into(),
                Ok(vec![]),
            ),
            (
                TU::core_flat_sequence().into(),
                Ok(TU::core_flat_sequence().into_iter().map(Result::Ok).collect()),
            ),
            (
                TU::core_nested_sequence().into(),
                Ok(TU::core_nested_sequence().into_iter().map(Result::Ok).collect()),
            ),
            (
                vec![TU::i(1), TU::i(1), TU::i(1), TU::i(2), TU::i(2), TU::i(3), TU::i(3), TU::i(3), TU::i(1)].into(),
                Ok(vec![Ok(TU::i(1)), Ok(TU::i(2)), Ok(TU::i(3)), Ok(TU::i(1))]),
            ),
            (
                vec![TU::i(1), TU::i(2), TU::i(3), TU::i(4), TU::i(5)].into(),
                Ok(vec![Ok(TU::i(1)), Ok(TU::i(2)), Ok(TU::i(3)), Ok(TU::i(4)), Ok(TU::i(5))]),
            ),
            (
                vec![TU::i(1), TU::i(1), TU::i(1), TU::i(1), TU::i(1)].into(),
                Ok(vec![Ok(TU::i(1))]),
            ),
            (
                VP::fixed(vec![]).into(),
                Ok(vec![]),
            ),
            (
                VP::fixed(TU::core_flat_sequence()).into(),
                Ok(TU::core_flat_sequence().into_iter().map(Result::Ok).collect()),
            ),
            (
                VP::fixed(TU::core_nested_sequence()).into(),
                Ok(TU::core_nested_sequence().into_iter().map(Result::Ok).collect()),
            ),
            (
                VP::fixed(vec![TU::i(1), TU::i(1), TU::i(1), TU::i(2), TU::i(2), TU::i(3), TU::i(3), TU::i(3), TU::i(1)]).into(),
                Ok(vec![Ok(TU::i(1)), Ok(TU::i(2)), Ok(TU::i(3)), Ok(TU::i(1))]),
            ),
            (
                VP::fixed(vec![TU::i(1), TU::i(2), TU::i(3), TU::i(4), TU::i(5)]).into(),
                Ok(vec![Ok(TU::i(1)), Ok(TU::i(2)), Ok(TU::i(3)), Ok(TU::i(4)), Ok(TU::i(5))]),
            ),
            (
                VP::fixed(vec![TU::i(1), TU::i(1), TU::i(1), TU::i(1), TU::i(1)]).into(),
                Ok(vec![Ok(TU::i(1))]),
            ),
            (
                VP::raw(vec![Ok(TU::i(1)), Err(Error::Sentinel)]).into(),
                Ok(vec![Ok(TU::i(1)), Err(ErrorKind::Sentinel)]),
            ),
        ];

        for (input, expected) in inputs_and_expected {
            let produced = input.dedup()
                .map_err(ErrorKind::from)
                .map(|il| {
                    il.into_iter().map(|res| {
                        res.map_err(ErrorKind::from)
                    })
                    .collect::<Vec<_>>()
                })
            ;
            assert_eq!(expected, produced);
        }
    }

    #[test]
    fn test_unique() {
        let inputs_and_expected: Vec<(IL, Result<Vec<Result<MetaVal, ErrorKind>>, ErrorKind>)> = vec![
            (
                vec![].into(),
                Ok(vec![]),
            ),
            (
                TU::core_flat_sequence().into(),
                Ok(TU::core_flat_sequence().into_iter().map(Result::Ok).collect()),
            ),
            (
                TU::core_nested_sequence().into(),
                Ok(TU::core_nested_sequence().into_iter().map(Result::Ok).collect()),
            ),
            (
                vec![TU::i(1), TU::i(1), TU::i(1), TU::i(2), TU::i(2), TU::i(3), TU::i(3), TU::i(3), TU::i(1)].into(),
                Ok(vec![Ok(TU::i(1)), Ok(TU::i(2)), Ok(TU::i(3))]),
            ),
            (
                vec![TU::i(1), TU::i(2), TU::i(3), TU::i(3), TU::i(2), TU::i(1)].into(),
                Ok(vec![Ok(TU::i(1)), Ok(TU::i(2)), Ok(TU::i(3))]),
            ),
            (
                vec![TU::i(1), TU::i(2), TU::i(3), TU::i(4), TU::i(5)].into(),
                Ok(vec![Ok(TU::i(1)), Ok(TU::i(2)), Ok(TU::i(3)), Ok(TU::i(4)), Ok(TU::i(5))]),
            ),
            (
                vec![TU::i(1), TU::i(1), TU::i(1), TU::i(1), TU::i(1)].into(),
                Ok(vec![Ok(TU::i(1))]),
            ),
            (
                VP::fixed(vec![]).into(),
                Ok(vec![]),
            ),
            (
                VP::fixed(TU::core_flat_sequence()).into(),
                Ok(TU::core_flat_sequence().into_iter().map(Result::Ok).collect()),
            ),
            (
                VP::fixed(TU::core_nested_sequence()).into(),
                Ok(TU::core_nested_sequence().into_iter().map(Result::Ok).collect()),
            ),
            (
                VP::fixed(vec![TU::i(1), TU::i(1), TU::i(1), TU::i(2), TU::i(2), TU::i(3), TU::i(3), TU::i(3), TU::i(1)]).into(),
                Ok(vec![Ok(TU::i(1)), Ok(TU::i(2)), Ok(TU::i(3))]),
            ),
            (
                VP::fixed(vec![TU::i(1), TU::i(2), TU::i(3), TU::i(3), TU::i(2), TU::i(1)]).into(),
                Ok(vec![Ok(TU::i(1)), Ok(TU::i(2)), Ok(TU::i(3))]),
            ),
            (
                VP::fixed(vec![TU::i(1), TU::i(2), TU::i(3), TU::i(4), TU::i(5)]).into(),
                Ok(vec![Ok(TU::i(1)), Ok(TU::i(2)), Ok(TU::i(3)), Ok(TU::i(4)), Ok(TU::i(5))]),
            ),
            (
                VP::fixed(vec![TU::i(1), TU::i(1), TU::i(1), TU::i(1), TU::i(1)]).into(),
                Ok(vec![Ok(TU::i(1))]),
            ),
            (
                VP::raw(vec![Ok(TU::i(1)), Err(Error::Sentinel)]).into(),
                Ok(vec![Ok(TU::i(1)), Err(ErrorKind::Sentinel)]),
            ),
        ];

        for (input, expected) in inputs_and_expected {
            let produced = input.unique()
                .map_err(ErrorKind::from)
                .map(|il| {
                    il.into_iter().map(|res| {
                        res.map_err(ErrorKind::from)
                    })
                    .collect::<Vec<_>>()
                })
            ;
            assert_eq!(expected, produced);
        }
    }

    fn is_even_int(mv: &MetaVal) -> Result<bool, Error> {
        match mv {
            MetaVal::Int(i) => Ok(i % 2 == 0),
            _ => Err(Error::NotNumeric),
        }
    }

    fn is_boolean(mv: &MetaVal) -> Result<bool, Error> {
        match mv {
            MetaVal::Bul(..) => Ok(true),
            _ => Ok(false),
        }
    }

    fn is_lt_4_int(mv: &MetaVal) -> Result<bool, Error> {
        match mv {
            MetaVal::Int(i) => Ok(i < &4),
            _ => Err(Error::NotNumeric),
        }
    }

    fn conv_repr(mv: MetaVal) -> Result<MetaVal, Error> {
        Ok(
            MetaVal::Str(
                match mv {
                    MetaVal::Bul(..) => "boolean",
                    MetaVal::Dec(..) => "decimal",
                    MetaVal::Int(..) => "integer",
                    MetaVal::Map(..) => "mapping",
                    MetaVal::Nil => "null",
                    MetaVal::Seq(..) => "sequence",
                    MetaVal::Str(..) => "string",
                }.to_string()
            )
        )
    }

    fn conv_add_3(mv: MetaVal) -> Result<MetaVal, Error> {
        match mv {
            MetaVal::Dec(d) => Ok(MetaVal::Dec(d + dec!(3))),
            MetaVal::Int(i) => Ok(MetaVal::Int(i + 3)),
            _ => Err(Error::NotNumeric),
        }
    }

    #[test]
    fn test_nth() {
        let inputs_and_expected: Vec<((IL, usize), Result<MetaVal, ErrorKind>)> = vec![
            (
                (vec![].into(), 0),
                Err(ErrorKind::OutOfBounds),
            ),
            (
                (TU::core_nested_sequence().into(), 0),
                Ok(TU::sample_string()),
            ),
            (
                (TU::core_nested_sequence().into(), 100),
                Err(ErrorKind::OutOfBounds),
            ),
            (
                (vec![TU::i(1), TU::i(2)].into(), 1),
                Ok(TU::i(2)),
            ),
            (
                (VP::fixed(vec![]).into(), 0),
                Err(ErrorKind::OutOfBounds),
            ),
            (
                (VP::fixed(TU::core_nested_sequence()).into(), 0),
                Ok(TU::sample_string()),
            ),
            (
                (VP::fixed(TU::core_nested_sequence()).into(), 100),
                Err(ErrorKind::OutOfBounds),
            ),
            (
                (VP::raw(vec![Ok(TU::i(1)), Ok(TU::i(2)), Err(Error::Sentinel)]).into(), 1),
                Ok(TU::i(2)),
            ),
            (
                (VP::raw(vec![Err(Error::Sentinel), Ok(TU::i(1)), Ok(TU::i(2))]).into(), 1),
                Err(ErrorKind::Sentinel),
            ),
        ];

        for (inputs, expected) in inputs_and_expected {
            let (input, extra) = inputs;
            let produced = input.nth(extra).map_err(ErrorKind::from);
            assert_eq!(expected, produced);
        }
    }

    #[test]
    fn test_all() {
        let inputs_and_expected: Vec<((IL, fn(&MetaVal) -> Result<bool, Error>), Result<bool, ErrorKind>)> = vec![
            (
                (vec![].into(), is_boolean),
                Ok(true),
            ),
            (
                (TU::core_nested_sequence().into(), is_boolean),
                Ok(false),
            ),
            (
                (vec![TU::b(true), TU::b(true)].into(), is_boolean),
                Ok(true),
            ),
            (
                (vec![TU::b(true), TU::i(0)].into(), is_boolean),
                Ok(false),
            ),
            (
                (vec![TU::i(0), TU::i(2), TU::i(4), TU::i(6), TU::i(8)].into(), is_even_int),
                Ok(true),
            ),
            (
                (vec![TU::i(1), TU::i(3), TU::i(4), TU::i(7), TU::i(9)].into(), is_even_int),
                Ok(false),
            ),
            (
                (vec![TU::i(0), TU::i(2), TU::i(5), TU::i(6), TU::i(8)].into(), is_even_int),
                Ok(false),
            ),
            (
                (vec![TU::i(1), TU::i(3), TU::i(5), TU::i(7), TU::i(9)].into(), is_even_int),
                Ok(false),
            ),
            (
                (vec![TU::i(0), TU::i(2), TU::b(false), TU::i(6), TU::i(8)].into(), is_even_int),
                Err(ErrorKind::NotNumeric),
            ),
            (
                (vec![TU::i(1), TU::i(3), TU::b(false), TU::i(7), TU::i(9)].into(), is_even_int),
                Ok(false),
            ),
            (
                (VP::fixed(vec![]).into(), is_boolean),
                Ok(true),
            ),
            (
                (VP::fixed(TU::core_nested_sequence()).into(), is_boolean),
                Ok(false),
            ),
            (
                (VP::raw(vec![Ok(TU::b(true)), Ok(TU::b(true)), Err(Error::Sentinel)]).into(), is_boolean),
                Err(ErrorKind::Sentinel),
            ),
            (
                (VP::raw(vec![Err(Error::Sentinel), Ok(TU::b(true)), Ok(TU::b(true))]).into(), is_boolean),
                Err(ErrorKind::Sentinel),
            ),
            (
                (VP::raw(vec![Ok(TU::b(true)), Ok(TU::i(0)), Err(Error::Sentinel)]).into(), is_boolean),
                Ok(false),
            ),
            (
                (VP::fixed(vec![TU::i(0), TU::i(2), TU::i(4), TU::i(6), TU::i(8)]).into(), is_even_int),
                Ok(true),
            ),
            (
                (VP::fixed(vec![TU::i(1), TU::i(3), TU::i(4), TU::i(7), TU::i(9)]).into(), is_even_int),
                Ok(false),
            ),
            (
                (VP::fixed(vec![TU::i(0), TU::i(2), TU::i(5), TU::i(6), TU::i(8)]).into(), is_even_int),
                Ok(false),
            ),
            (
                (VP::fixed(vec![TU::i(1), TU::i(3), TU::i(5), TU::i(7), TU::i(9)]).into(), is_even_int),
                Ok(false),
            ),
            (
                (VP::fixed(vec![TU::i(0), TU::i(2), TU::b(false), TU::i(6), TU::i(8)]).into(), is_even_int),
                Err(ErrorKind::NotNumeric),
            ),
            (
                (VP::fixed(vec![TU::i(1), TU::i(3), TU::b(false), TU::i(7), TU::i(9)]).into(), is_even_int),
                Ok(false),
            ),
        ];

        for (inputs, expected) in inputs_and_expected {
            let (input, extra) = inputs;
            let produced = input.all(extra).map_err(ErrorKind::from);
            assert_eq!(expected, produced);
        }
    }

    #[test]
    fn test_any() {
        let inputs_and_expected: Vec<((IL, fn(&MetaVal) -> Result<bool, Error>), Result<bool, ErrorKind>)> = vec![
            (
                (vec![].into(), is_boolean),
                Ok(false),
            ),
            (
                (TU::core_nested_sequence().into(), is_boolean),
                Ok(true),
            ),
            (
                (vec![TU::b(true), TU::b(true)].into(), is_boolean),
                Ok(true),
            ),
            (
                (vec![TU::b(true), TU::i(0)].into(), is_boolean),
                Ok(true),
            ),
            (
                (vec![TU::i(0), TU::i(2), TU::i(4), TU::i(6), TU::i(8)].into(), is_even_int),
                Ok(true),
            ),
            (
                (vec![TU::i(1), TU::i(3), TU::i(4), TU::i(7), TU::i(9)].into(), is_even_int),
                Ok(true),
            ),
            (
                (vec![TU::i(0), TU::i(2), TU::i(5), TU::i(6), TU::i(8)].into(), is_even_int),
                Ok(true),
            ),
            (
                (vec![TU::i(1), TU::i(3), TU::i(5), TU::i(7), TU::i(9)].into(), is_even_int),
                Ok(false),
            ),
            (
                (vec![TU::i(0), TU::i(2), TU::b(false), TU::i(6), TU::i(8)].into(), is_even_int),
                Ok(true),
            ),
            (
                (vec![TU::i(1), TU::i(3), TU::b(false), TU::i(7), TU::i(9)].into(), is_even_int),
                Err(ErrorKind::NotNumeric),
            ),
            (
                (VP::fixed(vec![]).into(), is_boolean),
                Ok(false),
            ),
            (
                (VP::fixed(TU::core_nested_sequence()).into(), is_boolean),
                Ok(true),
            ),
            (
                (VP::raw(vec![Ok(TU::b(true)), Ok(TU::b(true)), Err(Error::Sentinel)]).into(), is_boolean),
                Ok(true),
            ),
            (
                (VP::raw(vec![Err(Error::Sentinel), Ok(TU::b(true)), Ok(TU::b(true))]).into(), is_boolean),
                Err(ErrorKind::Sentinel),
            ),
            (
                (VP::raw(vec![Ok(TU::b(true)), Ok(TU::i(0)), Err(Error::Sentinel)]).into(), is_boolean),
                Ok(true),
            ),
            (
                (VP::fixed(vec![TU::i(0), TU::i(2), TU::i(4), TU::i(6), TU::i(8)]).into(), is_even_int),
                Ok(true),
            ),
            (
                (VP::fixed(vec![TU::i(1), TU::i(3), TU::i(4), TU::i(7), TU::i(9)]).into(), is_even_int),
                Ok(true),
            ),
            (
                (VP::fixed(vec![TU::i(0), TU::i(2), TU::i(5), TU::i(6), TU::i(8)]).into(), is_even_int),
                Ok(true),
            ),
            (
                (VP::fixed(vec![TU::i(1), TU::i(3), TU::i(5), TU::i(7), TU::i(9)]).into(), is_even_int),
                Ok(false),
            ),
            (
                (VP::fixed(vec![TU::i(0), TU::i(2), TU::b(false), TU::i(6), TU::i(8)]).into(), is_even_int),
                Ok(true),
            ),
            (
                (VP::fixed(vec![TU::i(1), TU::i(3), TU::b(false), TU::i(7), TU::i(9)]).into(), is_even_int),
                Err(ErrorKind::NotNumeric),
            ),
        ];

        for (inputs, expected) in inputs_and_expected {
            let (input, extra) = inputs;
            let produced = input.any(extra).map_err(ErrorKind::from);
            assert_eq!(expected, produced);
        }
    }

    #[test]
    fn test_find() {
        let inputs_and_expected: Vec<((IL, fn(&MetaVal) -> Result<bool, Error>), Result<MetaVal, ErrorKind>)> = vec![
            (
                (vec![].into(), is_boolean),
                Err(ErrorKind::ItemNotFound),
            ),
            (
                (TU::core_nested_sequence().into(), is_boolean),
                Ok(TU::sample_boolean()),
            ),
            (
                (vec![TU::b(false), TU::b(true)].into(), is_boolean),
                Ok(TU::b(false)),
            ),
            (
                (vec![TU::i(0), TU::i(2), TU::i(4), TU::i(6), TU::i(8)].into(), is_even_int),
                Ok(TU::i(0)),
            ),
            (
                (vec![TU::i(1), TU::i(3), TU::i(4), TU::i(7), TU::i(9)].into(), is_even_int),
                Ok(TU::i(4)),
            ),
            (
                (vec![TU::i(0), TU::i(2), TU::i(5), TU::i(6), TU::i(8)].into(), is_even_int),
                Ok(TU::i(0)),
            ),
            (
                (vec![TU::i(1), TU::i(3), TU::i(5), TU::i(7), TU::i(9)].into(), is_even_int),
                Err(ErrorKind::ItemNotFound),
            ),
            (
                (vec![TU::i(0), TU::i(2), TU::b(false), TU::i(6), TU::i(8)].into(), is_even_int),
                Ok(TU::i(0)),
            ),
            (
                (vec![TU::i(1), TU::i(3), TU::b(false), TU::i(7), TU::i(9)].into(), is_even_int),
                Err(ErrorKind::NotNumeric),
            ),
            (
                (vec![].into(), is_boolean),
                Err(ErrorKind::ItemNotFound),
            ),
            (
                (VP::fixed(TU::core_nested_sequence()).into(), is_boolean),
                Ok(TU::sample_boolean()),
            ),
            (
                (VP::raw(vec![Ok(TU::b(false)), Ok(TU::b(true)), Err(Error::Sentinel)]).into(), is_boolean),
                Ok(TU::b(false)),
            ),
            (
                (VP::raw(vec![Err(Error::Sentinel), Ok(TU::b(true)), Ok(TU::b(true))]).into(), is_boolean),
                Err(ErrorKind::Sentinel),
            ),
            (
                (VP::fixed(vec![TU::i(0), TU::i(2), TU::i(4), TU::i(6), TU::i(8)]).into(), is_even_int),
                Ok(TU::i(0)),
            ),
            (
                (VP::fixed(vec![TU::i(1), TU::i(3), TU::i(4), TU::i(7), TU::i(9)]).into(), is_even_int),
                Ok(TU::i(4)),
            ),
            (
                (VP::fixed(vec![TU::i(0), TU::i(2), TU::i(5), TU::i(6), TU::i(8)]).into(), is_even_int),
                Ok(TU::i(0)),
            ),
            (
                (VP::fixed(vec![TU::i(1), TU::i(3), TU::i(5), TU::i(7), TU::i(9)]).into(), is_even_int),
                Err(ErrorKind::ItemNotFound),
            ),
            (
                (VP::fixed(vec![TU::i(0), TU::i(2), TU::b(false), TU::i(6), TU::i(8)]).into(), is_even_int),
                Ok(TU::i(0)),
            ),
            (
                (VP::fixed(vec![TU::i(1), TU::i(3), TU::b(false), TU::i(7), TU::i(9)]).into(), is_even_int),
                Err(ErrorKind::NotNumeric),
            ),
        ];

        for (inputs, expected) in inputs_and_expected {
            let (input, extra) = inputs;
            let produced = input.find(extra).map_err(ErrorKind::from);
            assert_eq!(expected, produced);
        }
    }

    #[test]
    fn test_position() {
        let inputs_and_expected: Vec<((IL, fn(&MetaVal) -> Result<bool, Error>), Result<usize, ErrorKind>)> = vec![
            (
                (vec![].into(), is_boolean),
                Err(ErrorKind::ItemNotFound),
            ),
            (
                (TU::core_nested_sequence().into(), is_boolean),
                Ok(3),
            ),
            (
                (vec![TU::b(false), TU::b(true)].into(), is_boolean),
                Ok(0),
            ),
            (
                (vec![TU::i(0), TU::i(2), TU::i(4), TU::i(6), TU::i(8)].into(), is_even_int),
                Ok(0),
            ),
            (
                (vec![TU::i(1), TU::i(3), TU::i(4), TU::i(7), TU::i(9)].into(), is_even_int),
                Ok(2),
            ),
            (
                (vec![TU::i(0), TU::i(2), TU::i(5), TU::i(6), TU::i(8)].into(), is_even_int),
                Ok(0),
            ),
            (
                (vec![TU::i(1), TU::i(3), TU::i(5), TU::i(7), TU::i(9)].into(), is_even_int),
                Err(ErrorKind::ItemNotFound),
            ),
            (
                (vec![TU::i(0), TU::i(2), TU::b(false), TU::i(6), TU::i(8)].into(), is_even_int),
                Ok(0),
            ),
            (
                (vec![TU::i(1), TU::i(3), TU::b(false), TU::i(7), TU::i(9)].into(), is_even_int),
                Err(ErrorKind::NotNumeric),
            ),
            (
                (vec![].into(), is_boolean),
                Err(ErrorKind::ItemNotFound),
            ),
            (
                (VP::fixed(TU::core_nested_sequence()).into(), is_boolean),
                Ok(3),
            ),
            (
                (VP::raw(vec![Ok(TU::b(false)), Ok(TU::b(true)), Err(Error::Sentinel)]).into(), is_boolean),
                Ok(0),
            ),
            (
                (VP::raw(vec![Err(Error::Sentinel), Ok(TU::b(true)), Ok(TU::b(true))]).into(), is_boolean),
                Err(ErrorKind::Sentinel),
            ),
            (
                (VP::fixed(vec![TU::i(0), TU::i(2), TU::i(4), TU::i(6), TU::i(8)]).into(), is_even_int),
                Ok(0),
            ),
            (
                (VP::fixed(vec![TU::i(1), TU::i(3), TU::i(4), TU::i(7), TU::i(9)]).into(), is_even_int),
                Ok(2),
            ),
            (
                (VP::fixed(vec![TU::i(0), TU::i(2), TU::i(5), TU::i(6), TU::i(8)]).into(), is_even_int),
                Ok(0),
            ),
            (
                (VP::fixed(vec![TU::i(1), TU::i(3), TU::i(5), TU::i(7), TU::i(9)]).into(), is_even_int),
                Err(ErrorKind::ItemNotFound),
            ),
            (
                (VP::fixed(vec![TU::i(0), TU::i(2), TU::b(false), TU::i(6), TU::i(8)]).into(), is_even_int),
                Ok(0),
            ),
            (
                (VP::fixed(vec![TU::i(1), TU::i(3), TU::b(false), TU::i(7), TU::i(9)]).into(), is_even_int),
                Err(ErrorKind::NotNumeric),
            ),
        ];

        for (inputs, expected) in inputs_and_expected {
            let (input, extra) = inputs;
            let produced = input.position(extra).map_err(ErrorKind::from);
            assert_eq!(expected, produced);
        }
    }

    // #[test]
    // fn test_filter() {
    //     let inputs_and_expected: Vec<((_, fn(&MetaVal) -> Result<bool, Error>), _)> = vec![
    //         (
    //             (vec![], is_boolean),
    //             vec![],
    //         ),
    //         (
    //             (TU::core_nested_sequence().into_iter().map(Result::Ok).collect(), is_boolean),
    //             vec![Ok(TU::sample_boolean())],
    //         ),
    //         (
    //             (vec![Ok(TU::b(false)), Ok(MetaVal::Int(1)), Err(Error::Sentinel)], is_boolean),
    //             vec![Ok(TU::b(false)), Err(ErrorKind::Sentinel)],
    //         ),
    //         (
    //             (vec![Err(Error::Sentinel), Ok(TU::b(false)), Ok(MetaVal::Int(1))], is_boolean),
    //             vec![Err(ErrorKind::Sentinel), Ok(TU::b(false))],
    //         ),
    //         (
    //             (vec![Ok(TU::i(0)), Ok(TU::i(2)), Ok(TU::i(4)), Ok(TU::i(6)), Ok(TU::i(8))], is_even_int),
    //             vec![Ok(TU::i(0)), Ok(TU::i(2)), Ok(TU::i(4)), Ok(TU::i(6)), Ok(TU::i(8))],
    //         ),
    //         (
    //             (vec![Ok(TU::i(0)), Ok(TU::i(2)), Ok(TU::i(5)), Ok(TU::i(6)), Ok(TU::i(8))], is_even_int),
    //             vec![Ok(TU::i(0)), Ok(TU::i(2)), Ok(TU::i(6)), Ok(TU::i(8))],
    //         ),
    //         (
    //             (vec![Ok(TU::i(1)), Ok(TU::i(3)), Ok(TU::i(5)), Ok(TU::i(7)), Ok(TU::i(9))], is_even_int),
    //             vec![],
    //         ),
    //         (
    //             (vec![Ok(TU::i(0)), Ok(TU::i(2)), Ok(TU::b(false)), Ok(TU::i(6)), Ok(TU::i(8))], is_even_int),
    //             vec![Ok(TU::i(0)), Ok(TU::i(2)), Err(ErrorKind::NotNumeric), Ok(TU::i(6)), Ok(TU::i(8))],
    //         ),
    //         (
    //             (vec![Ok(TU::i(1)), Ok(TU::i(3)), Ok(TU::b(false)), Ok(TU::i(7)), Ok(TU::i(9))], is_even_int),
    //             vec![Err(ErrorKind::NotNumeric)],
    //         ),
    //     ];

    //     for (inputs, expected) in inputs_and_expected {
    //         let (input_a, input_b) = inputs;
    //         let produced = Impl::filter(VP::raw(input_a), input_b).map(|e| e.map_err(Into::<ErrorKind>::into)).collect::<Vec<_>>();
    //         assert_eq!(expected, produced);
    //     }
    // }

    // #[test]
    // fn test_filter_s() {
    //     let inputs_and_expected: Vec<((_, fn(&MetaVal) -> Result<bool, Error>), _)> = vec![
    //         (
    //             (vec![], is_boolean),
    //             Ok(vec![]),
    //         ),
    //         (
    //             (TU::core_nested_sequence(), is_boolean),
    //             Ok(vec![TU::sample_boolean()]),
    //         ),
    //         (
    //             (vec![TU::b(false), MetaVal::Int(1)], is_boolean),
    //             Ok(vec![TU::b(false)]),
    //         ),
    //         (
    //             (vec![TU::b(false), MetaVal::Int(1)], is_boolean),
    //             Ok(vec![TU::b(false)]),
    //         ),
    //         (
    //             (vec![TU::i(0), TU::i(2), TU::i(4), TU::i(6), TU::i(8)], is_even_int),
    //             Ok(vec![TU::i(0), TU::i(2), TU::i(4), TU::i(6), TU::i(8)]),
    //         ),
    //         (
    //             (vec![TU::i(0), TU::i(2), TU::i(5), TU::i(6), TU::i(8)], is_even_int),
    //             Ok(vec![TU::i(0), TU::i(2), TU::i(6), TU::i(8)]),
    //         ),
    //         (
    //             (vec![TU::i(1), TU::i(3), TU::i(5), TU::i(7), TU::i(9)], is_even_int),
    //             Ok(vec![]),
    //         ),
    //         (
    //             (vec![TU::i(0), TU::i(2), TU::b(false), TU::i(6), TU::i(8)], is_even_int),
    //             Err(ErrorKind::NotNumeric),
    //         ),
    //         (
    //             (vec![TU::i(1), TU::i(3), TU::b(false), TU::i(7), TU::i(9)], is_even_int),
    //             Err(ErrorKind::NotNumeric),
    //         ),
    //     ];

    //     for (inputs, expected) in inputs_and_expected {
    //         let (input_a, input_b) = inputs;
    //         let produced = Impl::filter_s(input_a, input_b).map_err(Into::<ErrorKind>::into);
    //         assert_eq!(expected, produced);
    //     }
    // }

    // #[test]
    // fn test_map() {
    //     let inputs_and_expected: Vec<((_, fn(MetaVal) -> Result<MetaVal, Error>), _)> = vec![
    //         (
    //             (vec![], conv_repr),
    //             vec![],
    //         ),
    //         (
    //             (TU::core_nested_sequence().into_iter().map(Result::Ok).collect(), conv_repr),
    //             vec![
    //                 Ok(TU::s("string")), Ok(TU::s("integer")), Ok(TU::s("decimal")), Ok(TU::s("boolean")),
    //                 Ok(TU::s("null")), Ok(TU::s("sequence")), Ok(TU::s("mapping")),
    //             ],
    //         ),
    //         (
    //             (vec![Ok(TU::b(false)), Ok(MetaVal::Int(1)), Err(Error::Sentinel)], conv_repr),
    //             vec![Ok(TU::s("boolean")), Ok(TU::s("integer")), Err(ErrorKind::Sentinel)],
    //         ),
    //         (
    //             (vec![Err(Error::Sentinel), Ok(TU::b(false)), Ok(MetaVal::Int(1))], conv_repr),
    //             vec![Err(ErrorKind::Sentinel), Ok(TU::s("boolean")), Ok(TU::s("integer"))],
    //         ),
    //         (
    //             (vec![Ok(TU::i(0)), Ok(TU::i(2)), Ok(TU::i(4)), Ok(TU::i(6)), Ok(TU::i(8))], conv_add_3),
    //             vec![Ok(TU::i(0+3)), Ok(TU::i(2+3)), Ok(TU::i(4+3)), Ok(TU::i(6+3)), Ok(TU::i(8+3))],
    //         ),
    //         (
    //             (vec![Ok(TU::i(0)), Ok(TU::i(2)), Ok(TU::b(false)), Ok(TU::i(6)), Ok(TU::i(8))], conv_add_3),
    //             vec![Ok(TU::i(0+3)), Ok(TU::i(2+3)), Err(ErrorKind::NotNumeric), Ok(TU::i(6+3)), Ok(TU::i(8+3))],
    //         ),
    //     ];

    //     for (inputs, expected) in inputs_and_expected {
    //         let (input_a, input_b) = inputs;
    //         let produced = Impl::map(VP::raw(input_a), input_b).map(|e| e.map_err(Into::<ErrorKind>::into)).collect::<Vec<_>>();
    //         assert_eq!(expected, produced);
    //     }
    // }

    // #[test]
    // fn test_map_s() {
    //     let inputs_and_expected: Vec<((_, fn(MetaVal) -> Result<MetaVal, Error>), _)> = vec![
    //         (
    //             (vec![], conv_repr),
    //             Ok(vec![]),
    //         ),
    //         (
    //             (TU::core_nested_sequence(), conv_repr),
    //             Ok(vec![
    //                 TU::s("string"), TU::s("integer"), TU::s("decimal"), TU::s("boolean"),
    //                 TU::s("null"), TU::s("sequence"), TU::s("mapping"),
    //             ]),
    //         ),
    //         (
    //             (vec![TU::b(false), MetaVal::Int(1)], conv_repr),
    //             Ok(vec![TU::s("boolean"), TU::s("integer")]),
    //         ),
    //         (
    //             (vec![TU::i(0), TU::i(2), TU::i(4), TU::i(6), TU::i(8)], conv_add_3),
    //             Ok(vec![TU::i(0+3), TU::i(2+3), TU::i(4+3), TU::i(6+3), TU::i(8+3)]),
    //         ),
    //         (
    //             (vec![TU::i(0), TU::i(2), TU::b(false), TU::i(6), TU::i(8)], conv_add_3),
    //             Err(ErrorKind::NotNumeric),
    //         ),
    //     ];

    //     for (inputs, expected) in inputs_and_expected {
    //         let (input_a, input_b) = inputs;
    //         let produced = Impl::map_s(input_a, input_b).map_err(Into::<ErrorKind>::into);
    //         assert_eq!(expected, produced);
    //     }
    // }

    // #[test]
    // fn test_step_by() {
    //     let inputs_and_expected = vec![
    //         (
    //             (vec![], 1),
    //             Ok(vec![]),
    //         ),
    //         (
    //             (vec![], 2),
    //             Ok(vec![]),
    //         ),
    //         (
    //             (vec![], 0),
    //             Err(ErrorKind::ZeroStepSize),
    //         ),
    //         (
    //             (TU::core_nested_sequence().into_iter().map(Result::Ok).collect(), 1),
    //             Ok(TU::core_nested_sequence().into_iter().map(Result::Ok).collect()),
    //         ),
    //         (
    //             (TU::core_nested_sequence().into_iter().map(Result::Ok).collect(), 2),
    //             Ok(TU::core_nested_sequence().into_iter().step_by(2).map(Result::Ok).collect()),
    //         ),
    //         (
    //             (vec![Ok(TU::b(false)), Ok(MetaVal::Int(1)), Err(Error::Sentinel)], 10),
    //             Ok(vec![Ok(TU::b(false)), Err(ErrorKind::Sentinel)]),
    //         ),
    //         (
    //             // TODO: Does this case make sense?
    //             //       Emitting leading errors, but not counting them as "stepped", and then emitting the first non-error item.
    //             (vec![Err(Error::Sentinel), Ok(TU::b(false)), Ok(MetaVal::Int(1))], 10),
    //             Ok(vec![Err(ErrorKind::Sentinel), Ok(TU::b(false))]),
    //         ),
    //         (
    //             ((0i64..=100).into_iter().map(TU::i).map(Result::Ok).collect(), 0),
    //             Err(ErrorKind::ZeroStepSize),
    //         ),
    //         (
    //             ((0i64..=100).into_iter().map(TU::i).map(Result::Ok).collect(), 1),
    //             Ok((0i64..=100).into_iter().step_by(1).map(TU::i).map(Result::Ok).collect()),
    //         ),
    //         (
    //             ((0i64..=100).into_iter().map(TU::i).map(Result::Ok).collect(), 2),
    //             Ok((0i64..=100).into_iter().step_by(2).map(TU::i).map(Result::Ok).collect()),
    //         ),
    //         (
    //             ((0i64..=100).into_iter().map(TU::i).map(Result::Ok).collect(), 4),
    //             Ok((0i64..=100).into_iter().step_by(4).map(TU::i).map(Result::Ok).collect()),
    //         ),
    //     ];

    //     for (inputs, expected) in inputs_and_expected {
    //         let (input_a, input_b) = inputs;
    //         let produced = Impl::step_by(VP::raw(input_a), input_b).map_err(Into::<ErrorKind>::into).map(|it| it.map(|r| r.map_err(Into::<ErrorKind>::into)).collect::<Vec<_>>());
    //         assert_eq!(expected, produced);
    //     }
    // }

    // #[test]
    // fn test_step_by_s() {
    //     let inputs_and_expected = vec![
    //         (
    //             (vec![], 1),
    //             Ok(vec![]),
    //         ),
    //         (
    //             (vec![], 2),
    //             Ok(vec![]),
    //         ),
    //         (
    //             (vec![], 0),
    //             Err(ErrorKind::ZeroStepSize),
    //         ),
    //         (
    //             (TU::core_nested_sequence(), 1),
    //             Ok(TU::core_nested_sequence()),
    //         ),
    //         (
    //             (TU::core_nested_sequence(), 2),
    //             Ok(TU::core_nested_sequence().into_iter().step_by(2).collect()),
    //         ),
    //         (
    //             (vec![TU::b(false), MetaVal::Int(1)], 10),
    //             Ok(vec![TU::b(false)]),
    //         ),
    //         (
    //             ((0i64..=100).into_iter().map(TU::i).collect(), 0),
    //             Err(ErrorKind::ZeroStepSize),
    //         ),
    //         (
    //             ((0i64..=100).into_iter().map(TU::i).collect(), 1),
    //             Ok((0i64..=100).into_iter().step_by(1).map(TU::i).collect()),
    //         ),
    //         (
    //             ((0i64..=100).into_iter().map(TU::i).collect(), 2),
    //             Ok((0i64..=100).into_iter().step_by(2).map(TU::i).collect()),
    //         ),
    //         (
    //             ((0i64..=100).into_iter().map(TU::i).collect(), 4),
    //             Ok((0i64..=100).into_iter().step_by(4).map(TU::i).collect()),
    //         ),
    //     ];

    //     for (inputs, expected) in inputs_and_expected {
    //         let (input_a, input_b) = inputs;
    //         let produced = Impl::step_by_s(input_a, input_b).map_err(Into::<ErrorKind>::into);
    //         assert_eq!(expected, produced);
    //     }
    // }

    // #[test]
    // fn test_chain() {
    //     let inputs_and_expected = vec![
    //         (
    //             (vec![], vec![]),
    //             vec![],
    //         ),
    //         (
    //             (TU::core_nested_sequence().into_iter().map(Result::Ok).collect(), TU::core_flat_sequence().into_iter().map(Result::Ok).collect()),
    //             TU::core_nested_sequence().into_iter().chain(TU::core_flat_sequence()).map(Result::Ok).collect(),
    //         ),
    //         (
    //             (vec![Ok(TU::b(false)), Err(Error::Sentinel)], vec![Err(Error::Sentinel), Ok(TU::b(true))]),
    //             vec![Ok(TU::b(false)), Err(ErrorKind::Sentinel), Err(ErrorKind::Sentinel), Ok(TU::b(true))],
    //         ),
    //         (
    //             (TU::core_nested_sequence().into_iter().map(Result::Ok).collect(), vec![]),
    //             TU::core_nested_sequence().into_iter().map(Result::Ok).collect(),
    //         ),
    //         (
    //             (vec![], TU::core_nested_sequence().into_iter().map(Result::Ok).collect()),
    //             TU::core_nested_sequence().into_iter().map(Result::Ok).collect(),
    //         ),
    //     ];

    //     for (inputs, expected) in inputs_and_expected {
    //         let (input_a, input_b) = inputs;
    //         let produced = Impl::chain(VP::raw(input_a), VP::raw(input_b)).map(|e| e.map_err(Into::<ErrorKind>::into)).collect::<Vec<_>>();
    //         assert_eq!(expected, produced);
    //     }
    // }

    // #[test]
    // fn test_chain_s() {
    //     let inputs_and_expected = vec![
    //         (
    //             (vec![], vec![]),
    //             vec![],
    //         ),
    //         (
    //             (TU::core_nested_sequence(), TU::core_flat_sequence()),
    //             TU::core_nested_sequence().into_iter().chain(TU::core_flat_sequence()).collect(),
    //         ),
    //         (
    //             (vec![TU::b(false), TU::i(1)], vec![TU::i(1), TU::b(true)]),
    //             vec![TU::b(false), TU::i(1), TU::i(1), TU::b(true)],
    //         ),
    //         (
    //             (TU::core_nested_sequence(), vec![]),
    //             TU::core_nested_sequence(),
    //         ),
    //         (
    //             (vec![], TU::core_nested_sequence()),
    //             TU::core_nested_sequence(),
    //         ),
    //     ];

    //     for (inputs, expected) in inputs_and_expected {
    //         let (input_a, input_b) = inputs;
    //         let produced = Impl::chain_s(input_a, input_b);
    //         assert_eq!(expected, produced);
    //     }
    // }

    // #[test]
    // fn test_zip() {
    //     let inputs_and_expected = vec![
    //         (
    //             (vec![], vec![]),
    //             vec![],
    //         ),
    //         (
    //             (
    //                 vec![Ok(TU::i(1)), Ok(TU::i(2)), Ok(TU::i(3)), Ok(TU::i(4))],
    //                 vec![Ok(TU::i(4)), Ok(TU::i(3)), Ok(TU::i(2)), Ok(TU::i(1))],
    //             ),
    //             vec![
    //                 Ok(MetaVal::Seq(vec![TU::i(1), TU::i(4)])),
    //                 Ok(MetaVal::Seq(vec![TU::i(2), TU::i(3)])),
    //                 Ok(MetaVal::Seq(vec![TU::i(3), TU::i(2)])),
    //                 Ok(MetaVal::Seq(vec![TU::i(4), TU::i(1)])),
    //             ],
    //         ),
    //         (
    //             (
    //                 vec![Ok(TU::i(1)), Ok(TU::i(2)), Ok(TU::i(3))],
    //                 vec![Ok(TU::i(4)), Ok(TU::i(3)), Ok(TU::i(2)), Ok(TU::i(1))],
    //             ),
    //             vec![
    //                 Ok(MetaVal::Seq(vec![TU::i(1), TU::i(4)])),
    //                 Ok(MetaVal::Seq(vec![TU::i(2), TU::i(3)])),
    //                 Ok(MetaVal::Seq(vec![TU::i(3), TU::i(2)])),
    //             ],
    //         ),
    //         (
    //             (
    //                 vec![Ok(TU::i(1)), Ok(TU::i(2)), Ok(TU::i(3)), Ok(TU::i(4))],
    //                 vec![Ok(TU::i(4)), Ok(TU::i(3)), Ok(TU::i(2))],
    //             ),
    //             vec![
    //                 Ok(MetaVal::Seq(vec![TU::i(1), TU::i(4)])),
    //                 Ok(MetaVal::Seq(vec![TU::i(2), TU::i(3)])),
    //                 Ok(MetaVal::Seq(vec![TU::i(3), TU::i(2)])),
    //             ],
    //         ),
    //         (
    //             (
    //                 vec![Ok(TU::i(1)), Err(Error::Sentinel), Ok(TU::i(3))],
    //                 vec![Err(Error::Sentinel), Ok(TU::i(2)), Ok(TU::i(3))],
    //             ),
    //             vec![
    //                 Err(ErrorKind::Sentinel),
    //                 Err(ErrorKind::Sentinel),
    //                 Ok(MetaVal::Seq(vec![TU::i(3), TU::i(3)])),
    //             ],
    //         ),
    //         (
    //             (
    //                 vec![Ok(TU::i(1)), Ok(TU::i(2)), Ok(TU::i(3))],
    //                 vec![Err(Error::Sentinel), Err(Error::Sentinel)],
    //             ),
    //             vec![
    //                 Err(ErrorKind::Sentinel),
    //                 Err(ErrorKind::Sentinel),
    //             ],
    //         ),
    //         (
    //             (
    //                 vec![],
    //                 vec![Ok(TU::i(4)), Ok(TU::i(3)), Ok(TU::i(2)), Ok(TU::i(1))],
    //             ),
    //             vec![],
    //         ),
    //         (
    //             (
    //                 vec![Ok(TU::i(1)), Ok(TU::i(2)), Ok(TU::i(3)), Ok(TU::i(4))],
    //                 vec![],
    //             ),
    //             vec![],
    //         ),
    //     ];

    //     for (inputs, expected) in inputs_and_expected {
    //         let (input_a, input_b) = inputs;
    //         let produced = Impl::zip(VP::raw(input_a), VP::raw(input_b)).map(|e| e.map_err(Into::<ErrorKind>::into)).collect::<Vec<_>>();
    //         assert_eq!(expected, produced);
    //     }
    // }

    // #[test]
    // fn test_zip_s() {
    //     let inputs_and_expected = vec![
    //         (
    //             (vec![], vec![]),
    //             vec![],
    //         ),
    //         (
    //             (
    //                 vec![TU::i(1), TU::i(2), TU::i(3), TU::i(4)],
    //                 vec![TU::i(4), TU::i(3), TU::i(2), TU::i(1)],
    //             ),
    //             vec![
    //                 MetaVal::Seq(vec![TU::i(1), TU::i(4)]),
    //                 MetaVal::Seq(vec![TU::i(2), TU::i(3)]),
    //                 MetaVal::Seq(vec![TU::i(3), TU::i(2)]),
    //                 MetaVal::Seq(vec![TU::i(4), TU::i(1)]),
    //             ],
    //         ),
    //         (
    //             (
    //                 vec![TU::i(1), TU::i(2), TU::i(3)],
    //                 vec![TU::i(4), TU::i(3), TU::i(2), TU::i(1)],
    //             ),
    //             vec![
    //                 MetaVal::Seq(vec![TU::i(1), TU::i(4)]),
    //                 MetaVal::Seq(vec![TU::i(2), TU::i(3)]),
    //                 MetaVal::Seq(vec![TU::i(3), TU::i(2)]),
    //             ],
    //         ),
    //         (
    //             (
    //                 vec![TU::i(1), TU::i(2), TU::i(3), TU::i(4)],
    //                 vec![TU::i(4), TU::i(3), TU::i(2)],
    //             ),
    //             vec![
    //                 MetaVal::Seq(vec![TU::i(1), TU::i(4)]),
    //                 MetaVal::Seq(vec![TU::i(2), TU::i(3)]),
    //                 MetaVal::Seq(vec![TU::i(3), TU::i(2)]),
    //             ],
    //         ),
    //         (
    //             (
    //                 vec![],
    //                 vec![TU::i(4), TU::i(3), TU::i(2), TU::i(1)],
    //             ),
    //             vec![],
    //         ),
    //         (
    //             (
    //                 vec![TU::i(1), TU::i(2), TU::i(3), TU::i(4)],
    //                 vec![],
    //             ),
    //             vec![],
    //         ),
    //     ];

    //     for (inputs, expected) in inputs_and_expected {
    //         let (input_a, input_b) = inputs;
    //         let produced = Impl::zip_s(input_a, input_b);
    //         assert_eq!(expected, produced);
    //     }
    // }

    // #[test]
    // fn test_skip() {
    //     let inputs_and_expected = vec![
    //         (
    //             (vec![], 0),
    //             vec![],
    //         ),
    //         (
    //             (vec![], 1),
    //             vec![],
    //         ),
    //         (
    //             (vec![Ok(TU::i(1)), Ok(TU::i(2)), Ok(TU::i(3)), Ok(TU::i(4)), Ok(TU::i(5))], 0),
    //             vec![Ok(TU::i(1)), Ok(TU::i(2)), Ok(TU::i(3)), Ok(TU::i(4)), Ok(TU::i(5))],
    //         ),
    //         (
    //             (vec![Ok(TU::i(1)), Ok(TU::i(2)), Ok(TU::i(3)), Ok(TU::i(4)), Ok(TU::i(5))], 1),
    //             vec![Ok(TU::i(2)), Ok(TU::i(3)), Ok(TU::i(4)), Ok(TU::i(5))],
    //         ),
    //         (
    //             (vec![Ok(TU::i(1)), Ok(TU::i(2)), Ok(TU::i(3)), Ok(TU::i(4)), Ok(TU::i(5))], 2),
    //             vec![Ok(TU::i(3)), Ok(TU::i(4)), Ok(TU::i(5))],
    //         ),
    //         (
    //             (vec![Ok(TU::i(1)), Ok(TU::i(2)), Ok(TU::i(3)), Ok(TU::i(4)), Ok(TU::i(5))], 4),
    //             vec![Ok(TU::i(5))],
    //         ),
    //         (
    //             (vec![Ok(TU::i(1)), Ok(TU::i(2)), Ok(TU::i(3)), Ok(TU::i(4)), Ok(TU::i(5))], 8),
    //             vec![],
    //         ),
    //         (
    //             (vec![Err(Error::Sentinel), Err(Error::Sentinel), Ok(TU::i(3)), Ok(TU::i(4)), Ok(TU::i(5))], 0),
    //             vec![Err(ErrorKind::Sentinel), Err(ErrorKind::Sentinel), Ok(TU::i(3)), Ok(TU::i(4)), Ok(TU::i(5))],
    //         ),
    //         (
    //             (vec![Err(Error::Sentinel), Err(Error::Sentinel), Ok(TU::i(3)), Ok(TU::i(4)), Ok(TU::i(5))], 1),
    //             vec![Err(ErrorKind::Sentinel), Err(ErrorKind::Sentinel), Ok(TU::i(3)), Ok(TU::i(4)), Ok(TU::i(5))],
    //         ),
    //         (
    //             (vec![Err(Error::Sentinel), Err(Error::Sentinel), Ok(TU::i(3)), Ok(TU::i(4)), Ok(TU::i(5))], 2),
    //             vec![Err(ErrorKind::Sentinel), Err(ErrorKind::Sentinel), Ok(TU::i(3)), Ok(TU::i(4)), Ok(TU::i(5))],
    //         ),
    //         (
    //             (vec![Err(Error::Sentinel), Err(Error::Sentinel), Ok(TU::i(3)), Ok(TU::i(4)), Ok(TU::i(5))], 3),
    //             vec![Err(ErrorKind::Sentinel), Err(ErrorKind::Sentinel), Ok(TU::i(4)), Ok(TU::i(5))],
    //         ),
    //         (
    //             (vec![Err(Error::Sentinel), Err(Error::Sentinel), Ok(TU::i(3)), Ok(TU::i(4)), Ok(TU::i(5))], 4),
    //             vec![Err(ErrorKind::Sentinel), Err(ErrorKind::Sentinel), Ok(TU::i(5))],
    //         ),
    //         (
    //             (vec![Err(Error::Sentinel), Err(Error::Sentinel), Ok(TU::i(3)), Ok(TU::i(4)), Ok(TU::i(5))], 6),
    //             vec![Err(ErrorKind::Sentinel), Err(ErrorKind::Sentinel)],
    //         ),
    //     ];

    //     for (inputs, expected) in inputs_and_expected {
    //         let (input_a, input_b) = inputs;
    //         let produced = Impl::skip(VP::raw(input_a), input_b).map(|e| e.map_err(Into::<ErrorKind>::into)).collect::<Vec<_>>();
    //         assert_eq!(expected, produced);
    //     }
    // }

    // #[test]
    // fn test_skip_s() {
    //     let inputs_and_expected = vec![
    //         (
    //             (vec![], 0),
    //             vec![],
    //         ),
    //         (
    //             (vec![], 1),
    //             vec![],
    //         ),
    //         (
    //             (vec![TU::i(1), TU::i(2), TU::i(3), TU::i(4), TU::i(5)], 0),
    //             vec![TU::i(1), TU::i(2), TU::i(3), TU::i(4), TU::i(5)],
    //         ),
    //         (
    //             (vec![TU::i(1), TU::i(2), TU::i(3), TU::i(4), TU::i(5)], 1),
    //             vec![TU::i(2), TU::i(3), TU::i(4), TU::i(5)],
    //         ),
    //         (
    //             (vec![TU::i(1), TU::i(2), TU::i(3), TU::i(4), TU::i(5)], 2),
    //             vec![TU::i(3), TU::i(4), TU::i(5)],
    //         ),
    //         (
    //             (vec![TU::i(1), TU::i(2), TU::i(3), TU::i(4), TU::i(5)], 4),
    //             vec![TU::i(5)],
    //         ),
    //         (
    //             (vec![TU::i(1), TU::i(2), TU::i(3), TU::i(4), TU::i(5)], 8),
    //             vec![],
    //         ),
    //     ];

    //     for (inputs, expected) in inputs_and_expected {
    //         let (input_a, input_b) = inputs;
    //         let produced = Impl::skip_s(input_a, input_b);
    //         assert_eq!(expected, produced);
    //     }
    // }

    // #[test]
    // fn test_take() {
    //     let inputs_and_expected = vec![
    //         (
    //             (vec![], 0),
    //             vec![],
    //         ),
    //         (
    //             (vec![], 1),
    //             vec![],
    //         ),
    //         (
    //             (vec![Ok(TU::i(1)), Ok(TU::i(2)), Ok(TU::i(3)), Ok(TU::i(4)), Ok(TU::i(5))], 0),
    //             vec![],
    //         ),
    //         (
    //             (vec![Ok(TU::i(1)), Ok(TU::i(2)), Ok(TU::i(3)), Ok(TU::i(4)), Ok(TU::i(5))], 1),
    //             vec![Ok(TU::i(1))],
    //         ),
    //         (
    //             (vec![Ok(TU::i(1)), Ok(TU::i(2)), Ok(TU::i(3)), Ok(TU::i(4)), Ok(TU::i(5))], 2),
    //             vec![Ok(TU::i(1)), Ok(TU::i(2))],
    //         ),
    //         (
    //             (vec![Ok(TU::i(1)), Ok(TU::i(2)), Ok(TU::i(3)), Ok(TU::i(4)), Ok(TU::i(5))], 4),
    //             vec![Ok(TU::i(1)), Ok(TU::i(2)), Ok(TU::i(3)), Ok(TU::i(4))],
    //         ),
    //         (
    //             (vec![Ok(TU::i(1)), Ok(TU::i(2)), Ok(TU::i(3)), Ok(TU::i(4)), Ok(TU::i(5))], 8),
    //             vec![Ok(TU::i(1)), Ok(TU::i(2)), Ok(TU::i(3)), Ok(TU::i(4)), Ok(TU::i(5))],
    //         ),
    //         (
    //             (vec![Err(Error::Sentinel), Err(Error::Sentinel), Ok(TU::i(3)), Ok(TU::i(4)), Ok(TU::i(5))], 0),
    //             vec![],
    //         ),
    //         (
    //             (vec![Err(Error::Sentinel), Err(Error::Sentinel), Ok(TU::i(3)), Ok(TU::i(4)), Ok(TU::i(5))], 1),
    //             vec![Err(ErrorKind::Sentinel)],
    //         ),
    //         (
    //             (vec![Err(Error::Sentinel), Err(Error::Sentinel), Ok(TU::i(3)), Ok(TU::i(4)), Ok(TU::i(5))], 2),
    //             vec![Err(ErrorKind::Sentinel), Err(ErrorKind::Sentinel)],
    //         ),
    //         (
    //             (vec![Err(Error::Sentinel), Err(Error::Sentinel), Ok(TU::i(3)), Ok(TU::i(4)), Ok(TU::i(5))], 3),
    //             vec![Err(ErrorKind::Sentinel), Err(ErrorKind::Sentinel), Ok(TU::i(3))],
    //         ),
    //         (
    //             (vec![Err(Error::Sentinel), Err(Error::Sentinel), Ok(TU::i(3)), Ok(TU::i(4)), Ok(TU::i(5))], 4),
    //             vec![Err(ErrorKind::Sentinel), Err(ErrorKind::Sentinel), Ok(TU::i(3)), Ok(TU::i(4))],
    //         ),
    //         (
    //             (vec![Err(Error::Sentinel), Err(Error::Sentinel), Ok(TU::i(3)), Ok(TU::i(4)), Ok(TU::i(5))], 6),
    //             vec![Err(ErrorKind::Sentinel), Err(ErrorKind::Sentinel), Ok(TU::i(3)), Ok(TU::i(4)), Ok(TU::i(5))],
    //         ),
    //     ];

    //     for (inputs, expected) in inputs_and_expected {
    //         let (input_a, input_b) = inputs;
    //         let produced = Impl::take(VP::raw(input_a), input_b).map(|e| e.map_err(Into::<ErrorKind>::into)).collect::<Vec<_>>();
    //         assert_eq!(expected, produced);
    //     }
    // }

    // #[test]
    // fn test_take_s() {
    //     let inputs_and_expected = vec![
    //         (
    //             (vec![], 0),
    //             vec![],
    //         ),
    //         (
    //             (vec![], 1),
    //             vec![],
    //         ),
    //         (
    //             (vec![TU::i(1), TU::i(2), TU::i(3), TU::i(4), TU::i(5)], 0),
    //             vec![],
    //         ),
    //         (
    //             (vec![TU::i(1), TU::i(2), TU::i(3), TU::i(4), TU::i(5)], 1),
    //             vec![TU::i(1)],
    //         ),
    //         (
    //             (vec![TU::i(1), TU::i(2), TU::i(3), TU::i(4), TU::i(5)], 2),
    //             vec![TU::i(1), TU::i(2)],
    //         ),
    //         (
    //             (vec![TU::i(1), TU::i(2), TU::i(3), TU::i(4), TU::i(5)], 4),
    //             vec![TU::i(1), TU::i(2), TU::i(3), TU::i(4)],
    //         ),
    //         (
    //             (vec![TU::i(1), TU::i(2), TU::i(3), TU::i(4), TU::i(5)], 8),
    //             vec![TU::i(1), TU::i(2), TU::i(3), TU::i(4), TU::i(5)],
    //         ),
    //     ];

    //     for (inputs, expected) in inputs_and_expected {
    //         let (input_a, input_b) = inputs;
    //         let produced = Impl::take_s(input_a, input_b);
    //         assert_eq!(expected, produced);
    //     }
    // }

    // #[test]
    // fn test_skip_while() {
    //     let inputs_and_expected: Vec<((_, fn(&MetaVal) -> Result<bool, Error>), _)> = vec![
    //         (
    //             (vec![], is_lt_4_int),
    //             vec![],
    //         ),
    //         (
    //             (vec![Ok(TU::i(1)), Ok(TU::i(2)), Ok(TU::i(3))], is_lt_4_int),
    //             vec![],
    //         ),
    //         (
    //             (vec![Ok(TU::i(4)), Ok(TU::i(5)), Ok(TU::i(6))], is_lt_4_int),
    //             vec![Ok(TU::i(4)), Ok(TU::i(5)), Ok(TU::i(6))],
    //         ),
    //         (
    //             (vec![Ok(TU::i(1)), Ok(TU::i(2)), Ok(TU::i(3)), Ok(TU::i(4)), Ok(TU::i(5)), Ok(TU::i(6))], is_lt_4_int),
    //             vec![Ok(TU::i(4)), Ok(TU::i(5)), Ok(TU::i(6))],
    //         ),
    //         (
    //             (vec![Ok(TU::i(1)), Err(Error::Sentinel), Ok(TU::i(2)), Ok(TU::i(3)), Ok(TU::i(4)), Ok(TU::i(5))], is_lt_4_int),
    //             vec![Err(ErrorKind::Sentinel), Ok(TU::i(4)), Ok(TU::i(5))],
    //         ),
    //         (
    //             (vec![Ok(TU::i(1)), Ok(TU::i(2)), Ok(TU::i(3)), Ok(TU::i(4)), Ok(TU::i(5)), Err(Error::Sentinel)], is_lt_4_int),
    //             vec![Ok(TU::i(4)), Ok(TU::i(5)), Err(ErrorKind::Sentinel)],
    //         ),
    //         (
    //             (vec![Ok(TU::s("a")), Ok(TU::i(1)), Ok(TU::i(2)), Ok(TU::i(3)), Ok(TU::i(4)), Ok(TU::i(5))], is_lt_4_int),
    //             vec![Err(ErrorKind::NotNumeric), Ok(TU::i(4)), Ok(TU::i(5))],
    //         ),
    //         (
    //             (vec![Ok(TU::i(1)), Ok(TU::i(2)), Ok(TU::i(3)), Ok(TU::i(4)), Ok(TU::i(5)), Ok(TU::s("a"))], is_lt_4_int),
    //             vec![Ok(TU::i(4)), Ok(TU::i(5)), Ok(TU::s("a"))],
    //         ),
    //     ];

    //     for (inputs, expected) in inputs_and_expected {
    //         let (input_a, input_b) = inputs;
    //         let produced = Impl::skip_while(VP::raw(input_a), input_b).map(|e| e.map_err(Into::<ErrorKind>::into)).collect::<Vec<_>>();
    //         assert_eq!(expected, produced);
    //     }
    // }

    // #[test]
    // fn test_skip_while_s() {
    //     let inputs_and_expected: Vec<((_, fn(&MetaVal) -> Result<bool, Error>), _)> = vec![
    //         (
    //             (vec![], is_lt_4_int),
    //             Ok(vec![]),
    //         ),
    //         (
    //             (vec![TU::i(1), TU::i(2), TU::i(3)], is_lt_4_int),
    //             Ok(vec![]),
    //         ),
    //         (
    //             (vec![TU::i(4), TU::i(5), TU::i(6)], is_lt_4_int),
    //             Ok(vec![TU::i(4), TU::i(5), TU::i(6)]),
    //         ),
    //         (
    //             (vec![TU::i(1), TU::i(2), TU::i(3), TU::i(4), TU::i(5), TU::i(6)], is_lt_4_int),
    //             Ok(vec![TU::i(4), TU::i(5), TU::i(6)]),
    //         ),
    //         (
    //             (vec![TU::s("a"), TU::i(1), TU::i(2), TU::i(3), TU::i(4), TU::i(5)], is_lt_4_int),
    //             Err(ErrorKind::NotNumeric),
    //         ),
    //         (
    //             (vec![TU::i(1), TU::i(2), TU::i(3), TU::i(4), TU::i(5), TU::s("a")], is_lt_4_int),
    //             Ok(vec![TU::i(4), TU::i(5), TU::s("a")]),
    //         ),
    //     ];

    //     for (inputs, expected) in inputs_and_expected {
    //         let (input_a, input_b) = inputs;
    //         let produced = Impl::skip_while_s(input_a, input_b).map_err(Into::<ErrorKind>::into);
    //         assert_eq!(expected, produced);
    //     }
    // }

    // #[test]
    // fn test_take_while() {
    //     let inputs_and_expected: Vec<((_, fn(&MetaVal) -> Result<bool, Error>), _)> = vec![
    //         (
    //             (vec![], is_lt_4_int),
    //             vec![],
    //         ),
    //         (
    //             (vec![Ok(TU::i(1)), Ok(TU::i(2)), Ok(TU::i(3))], is_lt_4_int),
    //             vec![Ok(TU::i(1)), Ok(TU::i(2)), Ok(TU::i(3))],
    //         ),
    //         (
    //             (vec![Ok(TU::i(4)), Ok(TU::i(5)), Ok(TU::i(6))], is_lt_4_int),
    //             vec![],
    //         ),
    //         (
    //             (vec![Ok(TU::i(1)), Ok(TU::i(2)), Ok(TU::i(3)), Ok(TU::i(4)), Ok(TU::i(5)), Ok(TU::i(6))], is_lt_4_int),
    //             vec![Ok(TU::i(1)), Ok(TU::i(2)), Ok(TU::i(3))],
    //         ),
    //         (
    //             (vec![Ok(TU::i(1)), Err(Error::Sentinel), Ok(TU::i(2)), Ok(TU::i(3)), Ok(TU::i(4)), Ok(TU::i(5))], is_lt_4_int),
    //             vec![Ok(TU::i(1)), Err(ErrorKind::Sentinel), Ok(TU::i(2)), Ok(TU::i(3))],
    //         ),
    //         (
    //             (vec![Ok(TU::i(1)), Ok(TU::i(2)), Ok(TU::i(3)), Ok(TU::i(4)), Ok(TU::i(5)), Err(Error::Sentinel)], is_lt_4_int),
    //             vec![Ok(TU::i(1)), Ok(TU::i(2)), Ok(TU::i(3))],
    //         ),
    //         (
    //             (vec![Ok(TU::s("a")), Ok(TU::i(1)), Ok(TU::i(2)), Ok(TU::i(3)), Ok(TU::i(4)), Ok(TU::i(5))], is_lt_4_int),
    //             vec![Err(ErrorKind::NotNumeric), Ok(TU::i(1)), Ok(TU::i(2)), Ok(TU::i(3))],
    //         ),
    //         (
    //             (vec![Ok(TU::i(1)), Ok(TU::i(2)), Ok(TU::i(3)), Ok(TU::i(4)), Ok(TU::i(5)), Ok(TU::s("a"))], is_lt_4_int),
    //             vec![Ok(TU::i(1)), Ok(TU::i(2)), Ok(TU::i(3))],
    //         ),
    //     ];

    //     for (inputs, expected) in inputs_and_expected {
    //         let (input_a, input_b) = inputs;
    //         let produced = Impl::take_while(VP::raw(input_a), input_b).map(|e| e.map_err(Into::<ErrorKind>::into)).collect::<Vec<_>>();
    //         assert_eq!(expected, produced);
    //     }
    // }

    // #[test]
    // fn test_take_while_s() {
    //     let inputs_and_expected: Vec<((_, fn(&MetaVal) -> Result<bool, Error>), _)> = vec![
    //         (
    //             (vec![], is_lt_4_int),
    //             Ok(vec![]),
    //         ),
    //         (
    //             (vec![TU::i(1), TU::i(2), TU::i(3)], is_lt_4_int),
    //             Ok(vec![TU::i(1), TU::i(2), TU::i(3)]),
    //         ),
    //         (
    //             (vec![TU::i(4), TU::i(5), TU::i(6)], is_lt_4_int),
    //             Ok(vec![]),
    //         ),
    //         (
    //             (vec![TU::i(1), TU::i(2), TU::i(3), TU::i(4), TU::i(5), TU::i(6)], is_lt_4_int),
    //             Ok(vec![TU::i(1), TU::i(2), TU::i(3)]),
    //         ),
    //         (
    //             (vec![TU::s("a"), TU::i(1), TU::i(2), TU::i(3), TU::i(4), TU::i(5)], is_lt_4_int),
    //             Err(ErrorKind::NotNumeric),
    //         ),
    //         (
    //             (vec![TU::i(1), TU::i(2), TU::i(3), TU::i(4), TU::i(5), TU::s("a")], is_lt_4_int),
    //             Ok(vec![TU::i(1), TU::i(2), TU::i(3)]),
    //         ),
    //     ];

    //     for (inputs, expected) in inputs_and_expected {
    //         let (input_a, input_b) = inputs;
    //         let produced = Impl::take_while_s(input_a, input_b).map_err(Into::<ErrorKind>::into);
    //         assert_eq!(expected, produced);
    //     }
    // }
}
