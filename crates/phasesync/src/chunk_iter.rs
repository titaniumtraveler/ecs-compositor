use crate::{
    helpers::bitmask_range,
    position::{CarryingAdd, Pos, WrappingU6, WrappingUsize},
};
use std::ops::RangeInclusive;

pub struct ChunkIter<const MAX: usize> {
    state: State,
    start: Pos<MAX>,
    end: Pos<MAX>,
}

impl<const MAX: usize> ChunkIter<MAX> {
    pub fn new(range: RangeInclusive<Pos<MAX>>) -> Self {
        let (start, end) = range.into_inner();
        Self { state: State::Start, start, end }
    }

    fn wrapping_add(&self, lhs: usize, rhs: usize) -> usize {
        *(WrappingUsize::<MAX>::new(lhs) + WrappingUsize::<MAX>::new(rhs))
    }
}

enum State {
    Start,
    Middle { next_chunk: usize },
    End,
    None,
}

impl<const MAX: usize> Iterator for ChunkIter<MAX> {
    type Item = ChunkInfo<MAX>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.state {
            State::Start => {
                let Pos { chunk, index: lower } = self.start;
                match chunk == self.end.chunk {
                    true => {
                        self.state = State::None;
                        Some(ChunkInfo { chunk, lower, upper: self.end.index })
                    }
                    false => {
                        let next_chunk = self.wrapping_add(*chunk, 1);
                        self.state = match next_chunk == *self.end.chunk {
                            true => State::Middle { next_chunk },
                            false => State::End,
                        };

                        Some(ChunkInfo { chunk, lower, upper: WrappingU6::MAX })
                    }
                }
            }
            State::Middle { next_chunk } => {
                let chunk = next_chunk;

                let next_chunk = self.wrapping_add(chunk, 1);
                self.state = match next_chunk == *self.end.chunk {
                    true => State::Middle { next_chunk },
                    false => State::End,
                };

                Some(ChunkInfo {
                    chunk: WrappingUsize::new(chunk),
                    lower: WrappingU6::ZERO,
                    upper: WrappingU6::MAX,
                })
            }
            State::End => {
                let Pos { chunk, index: upper } = self.end;
                self.state = State::None;

                Some(ChunkInfo { chunk, lower: WrappingU6::ZERO, upper })
            }
            State::None => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ChunkInfo<const MAX: usize> {
    pub chunk: WrappingUsize<MAX>,
    pub lower: WrappingU6,
    pub upper: WrappingU6,
}

impl<const MAX: usize> ChunkInfo<MAX> {
    pub fn mask(&self) -> u64 {
        bitmask_range(self.lower.inner(), self.upper.inner())
    }
}
