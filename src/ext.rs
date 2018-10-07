//! Extension traits for the standard `Stream` and `Future` traits.

use std::time::{Duration, Instant};
use std::io;
use std::pin::Pin;

use futures::prelude::*;
use futures::{Poll, task};

use crate::Delay;

/// An extension trait for futures which provides convenient accessors for
/// timing out execution and such.
pub trait FutureExt: TryFuture + Sized {

    /// Creates a new future which will take at most `dur` time to resolve from
    /// the point at which this method is called.
    ///
    /// This combinator creates a new future which wraps the receiving future
    /// in a timeout. The future returned will resolve in at most `dur` time
    /// specified (relative to when this function is called).
    ///
    /// If the future completes before `dur` elapses then the future will
    /// resolve with that item. Otherwise the future will resolve to an error
    /// once `dur` has elapsed.
    ///
    /// # Examples
    ///
    /// ```
    /// #![feature(futures_api)]
    /// extern crate futures;
    /// extern crate futures_timer;
    ///
    /// use std::time::Duration;
    /// use futures::prelude::*;
    /// use futures::executor::block_on;
    /// use futures_timer::FutureExt;
    ///
    /// # fn long_future() -> impl futures::Future<Output=Result<(), std::io::Error>> {
    /// #   futures::future::ok(())
    /// # }
    /// #
    /// fn main() {
    ///     let future = long_future();
    ///     let timed_out = future.timeout(Duration::from_secs(1));
    ///
    ///     match block_on(timed_out) {
    ///         Ok(item) => println!("got {:?} within enough time!", item),
    ///         Err(_) => println!("took too long to produce the item"),
    ///     }
    /// }
    /// ```
    fn timeout(self, dur: Duration) -> Timeout<Self>
        where Self::Error: From<io::Error>,
    {
        Timeout {
            delay: Delay::new(dur),
            future: self,
        }
    }

    /// Creates a new future which will resolve no later than `at` specified.
    ///
    /// This method is otherwise equivalent to the `timeout` method except that
    /// it tweaks the moment at when the timeout elapsed to being specified with
    /// an absolute value rather than a relative one. For more documentation see
    /// the `timeout` method.
    fn timeout_at(self, at: Instant) -> Timeout<Self>
        where Self::Error: From<io::Error>,
    {
        Timeout {
            delay: Delay::new_at(at),
            future: self,
        }
    }
}

impl<F: TryFuture> FutureExt for F {}

/// Future returned by the `FutureExt::timeout` method.
pub struct Timeout<F> {
    delay: Delay,
    future: F,
}

impl<F> Timeout<F>
    where F: TryFuture
{
    unsafe_pinned!(delay: Delay);
    unsafe_pinned!(future: F);
}

impl<F> Future for Timeout<F>
    where F: TryFuture,
          F::Error: From<io::Error>,
{
    type Output = Result<F::Ok, F::Error>;

   fn poll(mut self: Pin<&mut Self>, lw: &task::LocalWaker) -> Poll<Self::Output> {
        match self.future().try_poll(&lw) {
            Poll::Pending => {}
            other => return other,
        }

        if self.delay().poll(lw)?.is_ready() {
            Err(io::Error::new(io::ErrorKind::TimedOut, "future timed out").into()).into()
        } else {
            Poll::Pending
        }
    }
}

/// An extension trait for streams which provides convenient accessors for
/// timing out execution and such.
pub trait StreamExt: TryStream + Sized {

    /// Creates a new stream which will take at most `dur` time to yield each
    /// item of the stream.
    ///
    /// This combinator creates a new stream which wraps the receiving stream
    /// in a timeout-per-item. The stream returned will resolve in at most
    /// `dur` time for each item yielded from the stream. The first item's timer
    /// starts when this method is called.
    ///
    /// If a stream's item completes before `dur` elapses then the timer will be
    /// reset for the next item. If the timeout elapses, however, then an error
    /// will be yielded on the stream and the timer will be reset.
    fn timeout(self, dur: Duration) -> TimeoutStream<Self>
        where Self::Error: From<io::Error>,
    {
        TimeoutStream {
            delay: Delay::new(dur),
            dur,
            stream: self,
        }
    }
}

impl<S: TryStream> StreamExt for S {}

/// Stream returned by the `StreamExt::timeout` method.
pub struct TimeoutStream<S> {
    delay: Delay,
    dur: Duration,
    stream: S,
}

impl<S> TimeoutStream<S>
    where S: TryStream
{
    unsafe_pinned!(delay: Delay);
    unsafe_pinned!(stream: S);
}

impl<S> Stream for TimeoutStream<S>
    where S: TryStream,
          S::Error: From<io::Error>,
{
    type Item = Result<S::Ok, S::Error>;

    fn poll_next(mut self: Pin<&mut Self>, lw: &task::LocalWaker) -> Poll<Option<Self::Item>> {
        let dur = self.dur.clone();
        match self.stream().try_poll_next(lw) {
            Poll::Pending => {}
            other => {
                self.delay().reset(dur);
                return other
            }
        }

        if self.delay().poll(lw)?.is_ready() {
            self.delay().reset(dur);
            Some(Err(io::Error::new(io::ErrorKind::TimedOut, "stream item timed out").into())).into()
        } else {
            Poll::Pending
        }
    }
}
