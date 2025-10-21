#![allow(dead_code)]

use std::{
    pin::Pin,
    task::{Context, Poll, Waker},
};

struct PollWaker<T = ()>(T);

impl Future for PollWaker<()> {
    type Output = Waker;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Ready(cx.waker().to_owned())
    }
}

impl Future for PollWaker<Option<Waker>> {
    type Output = Waker;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut old_waker = self.0.take().expect("Waker has to be set");
        old_waker.clone_from(cx.waker());
        Poll::Ready(old_waker)
    }
}

impl Future for PollWaker<&mut Waker> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.0.clone_from(cx.waker());
        Poll::Ready(())
    }
}
