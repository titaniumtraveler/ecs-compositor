pub struct IterMapFn<Iter, Acc, F> {
    iter: Iter,
    acc: Acc,
    f: F,
}

pub trait IteratorExt: Iterator + Sized {
    fn iter_flat_map<Init, Acc, F, Item>(mut self, init: Init, f: F) -> IterMapFn<Self, Acc, F>
    where
        Init: FnOnce(&mut Self) -> Acc,
        F: FnMut(&mut Self, &mut Acc) -> Option<Item>,
    {
        IterMapFn {
            acc: init(&mut self),
            iter: self,
            f,
        }
    }
}
impl<I: Iterator> IteratorExt for I {}

impl<Iter, Acc, F, Item> Iterator for IterMapFn<Iter, Acc, F>
where
    Iter: Iterator,
    F: FnMut(&mut Iter, &mut Acc) -> Option<Item>,
{
    type Item = Item;

    fn next(&mut self) -> Option<Self::Item> {
        let Self { iter, acc, f } = self;
        f(iter, acc)
    }
}
