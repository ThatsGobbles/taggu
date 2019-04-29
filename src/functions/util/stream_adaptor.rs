use std::collections::VecDeque;
use std::collections::HashSet;

use crate::functions::Error;
use crate::functions::op::operator::Predicate;
use crate::functions::op::operand::Operand;
use crate::metadata::stream::value::MetaValueStream;
use crate::metadata::types::MetaVal;

#[derive(Debug)]
pub enum StreamAdaptor<'s> {
    Raw(MetaValueStream<'s>),
    Fixed(std::vec::IntoIter<MetaVal<'s>>),

    // Flatten(FlattenStream<'s>),
    // Dedup(DedupStream<'s>),
    // Unique(UniqueStream<'s>),
    // StepBy(StepByStream<'s>),
    // Chain(ChainStream<'s>),
    // Zip(ZipStream<'s>),
    // Map(MapStream<'s>),
}

impl<'s> Iterator for StreamAdaptor<'s> {
    type Item = Result<MetaVal<'s>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            &mut Self::Raw(ref mut it) => it.next().map(|res| res.map(|(_, mv)| mv).map_err(Error::ValueStream)),
            &mut Self::Fixed(ref mut it) => it.next().map(Result::Ok),

            // &mut Self::Flatten(ref mut it) => it.next(),
            // &mut Self::Dedup(ref mut it) => it.next(),
            // &mut Self::Unique(ref mut it) => it.next(),
            // &mut Self::StepBy(ref mut it) => it.next(),
            // &mut Self::Chain(ref mut it) => it.next(),
            // &mut Self::Zip(ref mut it) => it.next(),
            // &mut Self::Map(ref mut it) => it.next(),
        }
    }
}

#[derive(Debug)]
pub struct FilterAdaptor<'s>(Box<StreamAdaptor<'s>>, Predicate);

impl<'s> FilterAdaptor<'s> {
    pub fn new(s: StreamAdaptor<'s>, pred: Predicate) -> Self {
        Self(Box::new(s), pred)
    }
}

impl<'s> Iterator for FilterAdaptor<'s> {
    type Item = Result<MetaVal<'s>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.0.next()? {
            Ok(mv) => {
                match self.1.process(&mv) {
                    Err(err) => Some(Err(err)),
                    Ok(b) => {
                        if b { Some(Ok(mv)) }
                        else { self.next() }
                    },
                }
            },
            Err(err) => Some(Err(err)),
        }
    }
}