use std::ptr::{NonNull, slice_from_raw_parts, slice_from_raw_parts_mut};

#[allow(clippy::len_without_is_empty)]
pub trait RawSliceExt: Sized {
    /// helper function exclusively to be able to reuse `split_at()`
    fn len(&self) -> usize;

    /// Remove `count` elements from `&mut self`, move pointer forwards and return slice pointing
    /// to the range of items we just removed.
    ///
    /// If `self.len()` is too short, returns `None` and doesn't change the pointer.
    ///
    /// # Safety
    ///
    /// See the Safety requirements in `Self::add()`.
    ///
    /// Note that because this method operates **exclusively** *inside* the range
    /// pointed to by the slice; When the slice points to a valid allocation, this
    /// method will be **always** safe!
    unsafe fn split_at(&mut self, len: usize) -> Option<Self> {
        if self.len() < len {
            None
        } else {
            unsafe { Some(Self::split_at_unchecked(self, len)) }
        }
    }

    /// # Safety
    ///
    /// Same as [`Self::split_at()`], see its safety requirements, but doesn't bounds check.
    /// So running this with `self.len() < len` is undefined behavior!
    unsafe fn split_at_unchecked(&mut self, len: usize) -> Self;
}

impl<T> RawSliceExt for *const [T] {
    fn len(&self) -> usize {
        <*const [T]>::len(*self)
    }

    unsafe fn split_at_unchecked(&mut self, len: usize) -> Self {
        unsafe {
            let split_off = slice_from_raw_parts(self.cast(), len);
            *self = slice_from_raw_parts(self.cast::<T>().add(len), self.len().unchecked_sub(len));
            split_off
        }
    }
}

impl<T> RawSliceExt for *mut [T] {
    fn len(&self) -> usize {
        <*mut [T]>::len(*self)
    }

    unsafe fn split_at_unchecked(&mut self, len: usize) -> Self {
        unsafe {
            let split_off = slice_from_raw_parts_mut(self.cast(), len);
            *self =
                slice_from_raw_parts_mut(self.cast::<T>().add(len), self.len().unchecked_sub(len));
            split_off
        }
    }
}

impl<T> RawSliceExt for NonNull<[T]> {
    fn len(&self) -> usize {
        <NonNull<[T]>>::len(*self)
    }

    unsafe fn split_at_unchecked(&mut self, count: usize) -> Self {
        // SAFETY:
        // Self::new_unchecked() is safe because it operates on pointers that are already `NonNull`.
        // `ptr.add()` is safe because it stays inside the already existing allocation, which the
        // caller guaranties to be valid.
        unsafe {
            let split_off =
                Self::new_unchecked(slice_from_raw_parts_mut(self.as_ptr().cast(), count));
            *self = Self::new_unchecked(slice_from_raw_parts_mut(
                self.as_ptr().cast::<T>().add(count),
                self.len().unchecked_sub(count),
            ));
            split_off
        }
    }
}

#[test]
fn test_raw_slice_split() {
    unsafe {
        let mut main = slice_from_raw_parts(0x1000 as *const u8, 0x2000);
        let split1 = main.split_at(0x1000).unwrap();
        let split2 = main.split_at(0x1000).unwrap();

        assert_eq!(main, slice_from_raw_parts(0x3000 as *const u8, 0));
        assert_eq!(split1, slice_from_raw_parts(0x1000 as *const u8, 0x1000));
        assert_eq!(split2, slice_from_raw_parts(0x2000 as *const u8, 0x1000));
    }
}
