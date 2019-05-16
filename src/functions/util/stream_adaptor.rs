use std::collections::VecDeque;
use std::collections::HashSet;
use std::iter::FusedIterator;

use crate::functions::Error;
use crate::functions::operator::UnaryPredicate;
use crate::functions::operator::UnaryConverter;
use crate::metadata::stream::value::MetaValueStream;
use crate::metadata::types::MetaVal;

#[derive(Debug)]
pub enum StreamAdaptor<'s> {
    Raw(MetaValueStream<'s>),
    Fixed(std::vec::IntoIter<MetaVal<'s>>),

    Flatten(FlattenAdaptor<'s>),
    Dedup(DedupAdaptor<'s>),
    Unique(UniqueAdaptor<'s>),

    Filter(FilterAdaptor<'s>),
    Map(MapAdaptor<'s>),
    StepBy(StepByAdaptor<'s>),
    Chain(ChainAdaptor<'s>),
    Zip(ZipAdaptor<'s>),
    Skip(SkipAdaptor<'s>),
    Take(TakeAdaptor<'s>),
    SkipWhile(SkipWhileAdaptor<'s>),
    TakeWhile(TakeWhileAdaptor<'s>),
    Intersperse(IntersperseAdaptor<'s>),
    Interleave(InterleaveAdaptor<'s>),
    // Chunks,
    // Windows,
}

impl<'s> Iterator for StreamAdaptor<'s> {
    type Item = Result<MetaVal<'s>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            &mut Self::Raw(ref mut it) => it.next().map(|res| res.map(|(_, mv)| mv).map_err(Error::ValueStream)),
            &mut Self::Fixed(ref mut it) => it.next().map(Result::Ok),

            &mut Self::Flatten(ref mut it) => it.next(),
            &mut Self::Dedup(ref mut it) => it.next(),
            &mut Self::Unique(ref mut it) => it.next(),

            &mut Self::Filter(ref mut it) => it.next(),
            &mut Self::Map(ref mut it) => it.next(),
            &mut Self::StepBy(ref mut it) => it.next(),
            &mut Self::Chain(ref mut it) => it.next(),
            &mut Self::Zip(ref mut it) => it.next(),
            &mut Self::Skip(ref mut it) => it.next(),
            &mut Self::Take(ref mut it) => it.next(),
            &mut Self::SkipWhile(ref mut it) => it.next(),
            &mut Self::TakeWhile(ref mut it) => it.next(),
            &mut Self::Intersperse(ref mut it) => it.next(),
            &mut Self::Interleave(ref mut it) => it.next(),
        }
    }
}

#[derive(Debug)]
pub struct FlattenAdaptor<'s>(Box<StreamAdaptor<'s>>, VecDeque<MetaVal<'s>>);

impl<'s> FlattenAdaptor<'s> {
    pub fn new(s: StreamAdaptor<'s>) -> Self {
        Self(Box::new(s), VecDeque::new())
    }
}

impl<'s> Iterator for FlattenAdaptor<'s> {
    type Item = Result<MetaVal<'s>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.1.pop_front() {
            Some(mv) => Some(Ok(mv)),
            None => {
                // Try to get the next item from the stream.
                match self.0.next()? {
                    Ok(MetaVal::Seq(seq)) => {
                        // Move all elements in the sequence into the queue.
                        self.1.extend(seq);
                        self.next()
                    },
                    o => Some(o),
                }
            },
        }
    }
}#[derive(Debug)]
pub struct DedupAdaptor<'s>(Box<StreamAdaptor<'s>>, Option<MetaVal<'s>>);

impl<'s> DedupAdaptor<'s> {
    pub fn new(s: StreamAdaptor<'s>) -> Self {
        Self(Box::new(s), None)
    }
}

impl<'s> Iterator for DedupAdaptor<'s> {
    type Item = Result<MetaVal<'s>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        let res = self.0.next()?;

        match res {
            Err(err) => Some(Err(err)),
            Ok(curr_val) => {
                if Some(&curr_val) != self.1.as_ref() {
                    // A non-duplicate was found.
                    self.1 = Some(curr_val.clone());
                    Some(Ok(curr_val))
                }
                else {
                    // Delegate to the next call.
                    self.next()
                }
            },
        }
    }
}

#[derive(Debug)]
pub struct UniqueAdaptor<'s>(Box<StreamAdaptor<'s>>, HashSet<MetaVal<'s>>);

impl<'s> UniqueAdaptor<'s> {
    pub fn new(s: StreamAdaptor<'s>) -> Self {
        Self(Box::new(s), HashSet::new())
    }
}

impl<'s> Iterator for UniqueAdaptor<'s> {
    type Item = Result<MetaVal<'s>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        let res = self.0.next()?;

        match res {
            Err(err) => Some(Err(err)),
            Ok(curr_val) => {
                if self.1.contains(&curr_val) {
                    // Skip and delegate to the next call.
                    self.next()
                }
                else {
                    self.1.insert(curr_val.clone());
                    Some(Ok(curr_val))
                }
            },
        }
    }
}

#[derive(Debug)]
pub struct FilterAdaptor<'s>(Box<StreamAdaptor<'s>>, UnaryPredicate);

impl<'s> FilterAdaptor<'s> {
    pub fn new(s: StreamAdaptor<'s>, pred: UnaryPredicate) -> Self {
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

#[derive(Debug)]
pub struct MapAdaptor<'s>(Box<StreamAdaptor<'s>>, UnaryConverter);

impl<'s> MapAdaptor<'s> {
    pub fn new(s: StreamAdaptor<'s>, conv: UnaryConverter) -> Self {
        Self(Box::new(s), conv)
    }
}

impl<'s> Iterator for MapAdaptor<'s> {
    type Item = Result<MetaVal<'s>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.0.next()? {
            Ok(mv) => Some(self.1.process(mv)),
            Err(err) => Some(Err(err)),
        }
    }
}

#[derive(Debug)]
pub struct StepByAdaptor<'s> {
    stream: Box<StreamAdaptor<'s>>,
    curr: usize,
    n: usize,
}

impl<'s> StepByAdaptor<'s> {
    // Can fail if step size is zero.
    pub fn new(s: StreamAdaptor<'s>, n: usize) -> Result<Self, Error> {
        if n == 0 { Err(Error::ZeroStepSize) }
        else {
            Ok(Self {
                stream: Box::new(s),
                curr: n,
                n,
            })
        }
    }
}

impl<'s> Iterator for StepByAdaptor<'s> {
    type Item = Result<MetaVal<'s>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.stream.next()? {
            // Always report errors, even if they would not normally be "hit".
            Err(err) => Some(Err(err)),
            Ok(mv) => {
                // Output the meta value if currently at a step point.
                if self.curr >= self.n {
                    self.curr = 1;
                    Some(Ok(mv))
                }
                else {
                    self.curr += 1;
                    self.next()
                }
            }
        }
    }
}

#[derive(Debug)]
pub struct ChainAdaptor<'s>(Box<StreamAdaptor<'s>>, Box<StreamAdaptor<'s>>, bool);

impl<'s> ChainAdaptor<'s> {
    pub fn new(sa_a: StreamAdaptor<'s>, sa_b: StreamAdaptor<'s>) -> Self {
        Self(Box::new(sa_a), Box::new(sa_b), false)
    }
}

impl<'s> Iterator for ChainAdaptor<'s> {
    type Item = Result<MetaVal<'s>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        // Iterate the first stream.
        if !self.2 {
            match self.0.next() {
                None => {
                    self.2 = true;
                    self.next()
                }
                Some(res) => Some(res),
            }
        }
        // Iterate the second stream.
        else {
            self.1.next()
        }
    }
}

#[derive(Debug)]
pub struct ZipAdaptor<'s>(Box<StreamAdaptor<'s>>, Box<StreamAdaptor<'s>>);

impl<'s> ZipAdaptor<'s> {
    pub fn new(s_a: StreamAdaptor<'s>, s_b: StreamAdaptor<'s>) -> Self {
        Self(Box::new(s_a), Box::new(s_b))
    }
}

impl<'s> Iterator for ZipAdaptor<'s> {
    type Item = Result<MetaVal<'s>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        let res_a = self.0.next()?;
        let res_b = self.1.next()?;

        match (res_a, res_b) {
            (Err(e_a), _) => Some(Err(e_a)),
            (_, Err(e_b)) => Some(Err(e_b)),
            (Ok(a), Ok(b)) => Some(Ok(MetaVal::Seq(vec![a, b]))),
        }
    }
}

#[derive(Debug)]
pub struct SkipAdaptor<'s> {
    it: Box<StreamAdaptor<'s>>,
    curr: usize,
    n: usize,
}

impl<'s> SkipAdaptor<'s> {
    pub fn new(s: StreamAdaptor<'s>, n: usize) -> Self {
        Self {
            it: Box::new(s),
            curr: 0,
            n,
        }
    }
}

impl<'s> Iterator for SkipAdaptor<'s> {
    type Item = Result<MetaVal<'s>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        while self.curr < self.n {
            self.curr += 1;
            let res_mv = self.it.next()?;

            if let Err(e) = res_mv { return Some(Err(e)) }
        }

        self.it.next()
    }
}

#[derive(Debug)]
pub struct TakeAdaptor<'s> {
    it: Box<StreamAdaptor<'s>>,
    curr: usize,
    n: usize,
}

impl<'s> TakeAdaptor<'s> {
    pub fn new(s: StreamAdaptor<'s>, n: usize) -> Self {
        Self {
            it: Box::new(s),
            curr: 0,
            n,
        }
    }
}

impl<'s> Iterator for TakeAdaptor<'s> {
    type Item = Result<MetaVal<'s>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.curr < self.n {
            self.curr += 1;
            self.it.next()
        }
        else {
            None
        }
    }
}

#[derive(Debug)]
pub struct SkipWhileAdaptor<'s>(Box<StreamAdaptor<'s>>, UnaryPredicate, bool);

impl<'s> SkipWhileAdaptor<'s> {
    pub fn new(s: StreamAdaptor<'s>, u_pred: UnaryPredicate) -> Self {
        Self(Box::new(s), u_pred, true)
    }
}

impl<'s> Iterator for SkipWhileAdaptor<'s> {
    type Item = Result<MetaVal<'s>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.2 {
            loop {
                match self.0.next()? {
                    Err(e) => return Some(Err(e)),
                    Ok(mv) => {
                        match self.1.process(&mv) {
                            Err(e) => return Some(Err(e)),
                            Ok(true) => continue,
                            Ok(false) => {
                                self.2 = false;
                                return Some(Ok(mv))
                            }
                        }
                    },
                }
            }
        }

        self.0.next()
    }
}

#[derive(Debug)]
pub struct TakeWhileAdaptor<'s>(Box<StreamAdaptor<'s>>, UnaryPredicate, bool);

impl<'s> TakeWhileAdaptor<'s> {
    pub fn new(s: StreamAdaptor<'s>, u_pred: UnaryPredicate) -> Self {
        Self(Box::new(s), u_pred, true)
    }
}

impl<'s> Iterator for TakeWhileAdaptor<'s> {
    type Item = Result<MetaVal<'s>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.2 {
            match self.0.next()? {
                Ok(mv) => {
                    match self.1.process(&mv) {
                        Ok(true) => Some(Ok(mv)),
                        Ok(false) => {
                            self.2 = false;
                            return None
                        },
                        Err(e) => Some(Err(e)),
                    }
                },
                Err(e) => Some(Err(e)),
            }
        }
        else { None }
    }
}

#[derive(Debug)]
pub struct IntersperseAdaptor<'s>(Box<StreamAdaptor<'s>>, MetaVal<'s>, bool);

impl<'s> IntersperseAdaptor<'s> {
    pub fn new(s: StreamAdaptor<'s>, mv: MetaVal<'s>) -> Self {
        Self(Box::new(s), mv, false)
    }
}

impl<'s> Iterator for IntersperseAdaptor<'s> {
    type Item = Result<MetaVal<'s>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        self.2 = !self.2;

        if self.2 { self.0.next() }
        else { Some(Ok(self.1.clone())) }
    }
}

impl<'s> FusedIterator for IntersperseAdaptor<'s> {}

#[derive(Debug)]
pub struct InterleaveAdaptor<'s>(Box<StreamAdaptor<'s>>, Box<StreamAdaptor<'s>>, bool);

impl<'s> InterleaveAdaptor<'s> {
    pub fn new(s_a: StreamAdaptor<'s>, s_b: StreamAdaptor<'s>) -> Self {
        Self(Box::new(s_a), Box::new(s_b), false)
    }
}

impl<'s> Iterator for InterleaveAdaptor<'s> {
    type Item = Result<MetaVal<'s>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        self.2 = !self.2;

        if self.2 { self.0.next() }
        else { self.1.next() }
    }
}

impl<'s> FusedIterator for InterleaveAdaptor<'s> {}
