use crate::{
    helpers::{bit_mask_range, find_first_one, wrapping_add},
    sync_point::iter::{ChunkInfo, ChunkIter},
};
use std::{
    ops::RangeInclusive,
    sync::atomic::{AtomicU64, Ordering::*},
};

mod iter;

#[derive(Debug, Clone, Copy)]
pub struct Pos {
    /// Index into the `&[AtomicU64;LEN]` this position is refering to.
    chunk: usize,
    /// Index into the specific bit of the chunk pointed to by [`Self::chunk`].
    ///
    /// # Invariants
    /// - The first two bits are **always** `0`.
    ///   Or phrased differently: `index & 0b1100_0000 == 0` always holds.
    ///   If rust allowed it, this could be an `u6`
    index: u8,
}

impl Pos {
    #[allow(dead_code)]
    fn wrapping_add<const LEN: usize>(self, rhs: u8) -> Self {
        let Self { chunk, index } = self;
        wrapping_add! (
            index + rhs; 0..64u8;
            no_wrap => |WrapArgs { rhs,  .. }| Pos { chunk, index: index + rhs},
            do_wrap => |WrapArgs { rhs, diff, .. }| Pos { chunk: wrapping_add!(chunk + 1; 0..LEN), index: rhs - diff },
        )
    }
}

struct SyncPoint<const LEN: usize> {
    chunks: [AtomicU64; LEN],
}

impl<const LEN: usize> SyncPoint<LEN> {
    fn chunk_iter(slots: RangeInclusive<Pos>) -> ChunkIter<LEN> {
        ChunkIter::new(slots)
    }

    fn get_chunk(&self, info: ChunkInfo) -> &AtomicU64 {
        let ChunkInfo { chunk, .. } = info;
        &self.chunks[chunk]
    }

    fn load_chunk<'chunk>(&'chunk self, info: ChunkInfo) -> LoadedChunk<'chunk> {
        let chunk = self.get_chunk(info);
        let mask = info.mask();
        let val = chunk.load(Acquire);

        LoadedChunk { chunk, mask, val, info }
    }

    fn load_chunk_fn<'chunk>(&'chunk self) -> impl FnMut(ChunkInfo) -> LoadedChunk<'chunk> {
        move |info| self.load_chunk(info)
    }
}

struct LoadedChunk<'chunk> {
    chunk: &'chunk AtomicU64,
    mask: u64,
    val: u64,
    info: ChunkInfo,
}

impl<const LEN: usize> SyncPoint<LEN> {
    #[allow(dead_code)]
    pub fn free_slots(
        &self,
        slots: RangeInclusive<Pos>,
        until: Pos,
        commit: impl FnMut(Pos),
    ) -> FreeReturn {
        if self.fast_path(slots.clone()) {
            return FreeReturn::Successful;
        }

        self.slow_path(slots, until, commit)
    }

    fn fast_path(&self, slots: RangeInclusive<Pos>) -> bool {
        Self::chunk_iter(slots).map(self.load_chunk_fn()).all(
            |LoadedChunk { chunk, mask, val, .. }| {
                try_while(chunk, val, |val| val & mask == mask, |val| val & !mask)
            },
        )
    }

    fn slow_path(
        &self,
        slots: RangeInclusive<Pos>,
        until: Pos,
        mut commit: impl FnMut(Pos),
    ) -> FreeReturn {
        // re-set all slots to `1u1`
        Self::chunk_iter(slots.clone())
            .map(self.load_chunk_fn())
            .for_each(|LoadedChunk { chunk, mask, val, .. }| {
                assert!(try_while(chunk, val, |_| true, |val| val | mask))
            });

        let search_range = {
            let upper = slots.into_inner().1;
            upper.wrapping_add::<LEN>(1)..=until
        };
        Self::chunk_iter(search_range)
            .map(self.load_chunk_fn())
            .find_map(|LoadedChunk { chunk, mut mask, mut val, info }| {
                let mut lower = info.lower;
                while let Some(index) = find_first_one(val & mask) {
                    let slot = Pos { chunk: info.chunk, index };

                    if let Some(prev_index) = index
                        .checked_sub(1)
                        .filter(|prev_index| lower < *prev_index)
                    {
                        match try_while_mut(
                            chunk,
                            &mut val,
                            |val| val & (1 << index) == 1,
                            |val| val | bit_mask_range(lower, prev_index),
                        ) {
                            true => mask = bit_mask_range(prev_index, info.upper),
                            false => continue,
                        }
                    }

                    commit(slot);

                    match try_while_mut(
                        chunk,
                        &mut val,
                        |val| val & (1 << index) == 1,
                        |val| val & !(1 << index),
                    ) {
                        true => return Some(FreeReturn::Selected { slot }),
                        false => lower = index,
                    }
                }
                None
            })
            .unwrap_or(FreeReturn::AllSlotsDead)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum FreeReturn {
    /// We made it someone elses problem
    Successful,
    Selected {
        slot: Pos,
    },
    AllSlotsDead,
}

fn try_while(
    chunk: &AtomicU64,
    mut val: u64,
    mut cond: impl FnMut(u64) -> bool,
    mut f: impl FnMut(u64) -> u64,
) -> bool {
    while cond(val) {
        match chunk.compare_exchange(val, f(val), Release, Acquire) {
            Ok(_old) => return true,
            Err(actual) => val = actual,
        }
    }

    false
}

fn try_while_mut(
    chunk: &AtomicU64,
    val: &mut u64,
    mut cond: impl FnMut(u64) -> bool,
    mut f: impl FnMut(u64) -> u64,
) -> bool {
    while cond(*val) {
        match chunk.compare_exchange(*val, f(*val), Release, Acquire) {
            Ok(_old) => return true,
            Err(actual) => *val = actual,
        }
    }

    false
}
