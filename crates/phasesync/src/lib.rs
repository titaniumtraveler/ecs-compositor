use crate::{
    chunk_iter::{ChunkInfo, ChunkIter},
    helpers::{bitmask_range, lowest_one, try_while, try_while_mut},
};
use std::{
    ops::RangeInclusive,
    sync::atomic::{AtomicU64, Ordering::*},
};

pub use crate::position::Pos;

pub mod chunk_iter;
pub mod helpers;
mod position;

/// Synchronization primitive to allow the coordination of ring-buffer shaped resources between
/// multiple threads.
///
/// See [`examples/`](./examples/) for examples on how to use this.
#[repr(transparent)]
pub struct Phasesync<const LEN: usize> {
    pub chunks: [AtomicU64; LEN],
}

impl<const LEN: usize> Phasesync<LEN> {
    /// Main function of [`Phasesync`].
    /// Frees the range of slots, and then searches for the next active slot in this phase as
    /// set by `until`.
    ///
    /// See [`FreeReturn`] and more importantly [`FreeReturn::AllSlotsDead`] for details.
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
                while let Some(index) = lowest_one(val & mask) {
                    let slot = Pos { chunk: info.chunk, index };

                    if let Some(prev_index) = index
                        .checked_sub(1)
                        .filter(|prev_index| lower < *prev_index)
                    {
                        match try_while_mut(
                            chunk,
                            &mut val,
                            |val| val & (1 << index) == 1,
                            |val| val | bitmask_range(lower, prev_index),
                        ) {
                            true => mask = bitmask_range(prev_index, info.upper),
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

impl<const LEN: usize> Phasesync<LEN> {
    /// Iterator over the range of bits of each chunk described by `slots`.
    /// Note if `end.chunk < start.chunk`, this *will* correctly wrap around `const LEN`
    pub fn chunk_iter(slots: RangeInclusive<Pos>) -> ChunkIter<LEN> {
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

#[derive(Debug, Clone, Copy)]
#[must_use = "Make sure to handle the case of [`Self::AllSlotsDead`]"]
pub enum FreeReturn {
    /// The fast path was successful, so the resources associated with the slot(s) where not freed,
    /// but will be when the oldest slot in this phase was let go.
    Successful,
    /// The resources associated with the slot were successfully freed and [`Pos`] was selected as the next slot
    /// responsible for freeing resources of this phase.
    Selected { slot: Pos },
    /// The resources associated with the slot were successfully freed, but no slot in this phase
    /// are active anymore.
    ///
    /// That means all remaining resources of this phase can be freed.
    /// At least from the point of time where the [`Phasesync::free_slots()`] `until` parameter was
    /// loaded.
    ///
    /// That also means the caller of [`Phasesync::free_slots()`] is now responsible to in some way make
    /// sure the next time a slot is created in this phase, it knows, it is responsible for doing
    /// the resource freeing when it is destroyed again.
    AllSlotsDead,
}
