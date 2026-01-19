use crate::{
    helpers::{bitmask_range, wrapping_add},
    position::Pos,
};
use std::ops::RangeInclusive;

pub struct ChunkIter<const LEN: usize> {
    state: State,
    start: Pos,
    end: Pos,
}

impl<const LEN: usize> ChunkIter<LEN> {
    pub fn new(range: RangeInclusive<Pos>) -> Self {
        let (start, end) = range.into_inner();
        Self { state: State::Start, start, end }
    }

    fn wrapping_add(&self, lhs: usize, rhs: usize) -> usize {
        wrapping_add!(lhs + rhs; 0..LEN)
    }
}

enum State {
    Start,
    Middle { next_chunk: usize },
    End,
    None,
}

impl<const LEN: usize> Iterator for ChunkIter<LEN> {
    type Item = ChunkInfo;

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
                        let next_chunk = self.wrapping_add(chunk, 1);
                        self.state = match next_chunk == self.end.chunk {
                            true => State::Middle { next_chunk },
                            false => State::End,
                        };

                        Some(ChunkInfo { chunk, lower, upper: 63 })
                    }
                }
            }
            State::Middle { next_chunk } => {
                let chunk = next_chunk;

                let next_chunk = self.wrapping_add(chunk, 1);
                self.state = match next_chunk == self.end.chunk {
                    true => State::Middle { next_chunk },
                    false => State::End,
                };

                Some(ChunkInfo { chunk, lower: 0, upper: 63 })
            }
            State::End => {
                let Pos { chunk, index: upper } = self.end;
                self.state = State::None;

                Some(ChunkInfo { chunk, lower: 0, upper })
            }
            State::None => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ChunkInfo {
    pub chunk: usize,
    pub lower: u8,
    pub upper: u8,
}

impl ChunkInfo {
    pub fn mask(&self) -> u64 {
        bitmask_range(self.lower, self.upper)
    }
}
