use crate::{
    Interface,
    primitives::{enumeration, object},
};

pub type Result<T, I> = std::result::Result<T, Error<I>>;

pub struct Error<I: Interface = ()> {
    pub object: object<I>,
    pub err: u32,
    pub msg: &'static str,
}

impl<I: Interface> Error<I> {
    pub fn new(object: object<I>, err: I::Error, msg: &'static str) -> Self {
        Self {
            object,
            err: err.to_u32(),
            msg,
        }
    }

    pub fn err(&self) -> Option<I::Error> {
        <I::Error as enumeration>::from_u32(self.err)
    }

    pub fn cast<To: Interface>(self) -> Error<To> {
        let Error { object, err, msg } = self;

        Error {
            object: object.cast(),
            err,
            msg,
        }
    }
}
