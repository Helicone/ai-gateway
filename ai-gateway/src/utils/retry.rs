//! This is a modified version of the `retry` struct from the `backon` crate,
//! modified so that the `retryable_fn` can be a function that takes a
//! `&Result<T, E>` instead of an `Err(E)`.
//!
//! Upstream PR: <https://github.com/Xuanwo/backon/pull/203>
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, ready},
    time::Duration,
};

use backon::{Backoff, DefaultSleeper, Sleeper};

pub struct RetryWithResult<
    B: Backoff,
    T,
    E,
    Fut: Future<Output = Result<T, E>>,
    FutureFn: FnMut() -> Fut,
    RF = fn(&Result<T, E>) -> bool,
    NF = fn(&Result<T, E>, Duration),
    AF = fn(&Result<T, E>, Option<Duration>) -> Option<Duration>,
> {
    backoff: B,
    future_fn: FutureFn,

    retryable_fn: RF,
    notify_fn: NF,
    sleep_fn: DefaultSleeper,
    adjust_fn: AF,

    state: State<T, E, Fut, <DefaultSleeper as Sleeper>::Sleep>,
}

impl<B, T, E, Fut, FutureFn> RetryWithResult<B, T, E, Fut, FutureFn>
where
    B: Backoff,
    Fut: Future<Output = Result<T, E>>,
    FutureFn: FnMut() -> Fut,
{
    pub fn new(future_fn: FutureFn, backoff: B) -> Self {
        RetryWithResult {
            backoff,
            future_fn,

            retryable_fn: |result: &Result<T, E>| result.is_err(),
            notify_fn: |_: &Result<T, E>, _: Duration| {},
            adjust_fn: |_: &Result<T, E>, dur: Option<Duration>| dur,
            sleep_fn: DefaultSleeper::default(),

            state: State::Idle,
        }
    }
}

impl<B, T, E, Fut, FutureFn, RF, NF, AF>
    RetryWithResult<B, T, E, Fut, FutureFn, RF, NF, AF>
where
    B: Backoff,
    Fut: Future<Output = Result<T, E>>,
    FutureFn: FnMut() -> Fut,
    RF: FnMut(&Result<T, E>) -> bool,
    NF: FnMut(&Result<T, E>, Duration),
    AF: FnMut(&Result<T, E>, Option<Duration>) -> Option<Duration>,
{
    pub fn when<RN: FnMut(&Result<T, E>) -> bool>(
        self,
        retryable: RN,
    ) -> RetryWithResult<B, T, E, Fut, FutureFn, RN, NF, AF> {
        RetryWithResult {
            backoff: self.backoff,
            retryable_fn: retryable,
            notify_fn: self.notify_fn,
            future_fn: self.future_fn,
            sleep_fn: self.sleep_fn,
            adjust_fn: self.adjust_fn,
            state: self.state,
        }
    }

    pub fn notify<NN: FnMut(&Result<T, E>, Duration)>(
        self,
        notify: NN,
    ) -> RetryWithResult<B, T, E, Fut, FutureFn, RF, NN, AF> {
        RetryWithResult {
            backoff: self.backoff,
            retryable_fn: self.retryable_fn,
            notify_fn: notify,
            sleep_fn: self.sleep_fn,
            future_fn: self.future_fn,
            adjust_fn: self.adjust_fn,
            state: self.state,
        }
    }

    pub fn adjust<
        NAF: FnMut(&Result<T, E>, Option<Duration>) -> Option<Duration>,
    >(
        self,
        adjust: NAF,
    ) -> RetryWithResult<B, T, E, Fut, FutureFn, RF, NF, NAF> {
        RetryWithResult {
            backoff: self.backoff,
            retryable_fn: self.retryable_fn,
            notify_fn: self.notify_fn,
            sleep_fn: self.sleep_fn,
            future_fn: self.future_fn,
            adjust_fn: adjust,
            state: self.state,
        }
    }
}

#[derive(Default)]
enum State<
    T,
    E,
    Fut: Future<Output = Result<T, E>>,
    SleepFut: Future<Output = ()>,
> {
    #[default]
    Idle,
    Polling(Fut),
    Sleeping(SleepFut),
}

impl<B, T, E, Fut, FutureFn, RF, NF, AF> Future
    for RetryWithResult<B, T, E, Fut, FutureFn, RF, NF, AF>
where
    B: Backoff,
    Fut: Future<Output = Result<T, E>>,
    FutureFn: FnMut() -> Fut,
    RF: FnMut(&Result<T, E>) -> bool,
    NF: FnMut(&Result<T, E>, Duration),
    AF: FnMut(&Result<T, E>, Option<Duration>) -> Option<Duration>,
{
    type Output = Result<T, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // Safety: This is safe because we don't move the `RetryWithResult`
        // struct itself, only its internal state.
        //
        // We do the exactly same thing like `pin_project` but without depending
        // on it directly.
        let this = unsafe { self.get_unchecked_mut() };

        loop {
            match &mut this.state {
                State::Idle => {
                    let fut = (this.future_fn)();
                    this.state = State::Polling(fut);
                }
                State::Polling(fut) => {
                    // Safety: This is safe because we don't move the
                    // `RetryWithResult` struct and this fut,
                    // only its internal state.
                    //
                    // We do the exactly same thing like `pin_project` but
                    // without depending on it directly.
                    let mut fut = unsafe { Pin::new_unchecked(fut) };

                    let result: Result<T, E> = ready!(fut.as_mut().poll(cx));

                    if !(this.retryable_fn)(&result) {
                        return Poll::Ready(result);
                    }
                    let adjusted_backoff =
                        (this.adjust_fn)(&result, this.backoff.next());
                    match adjusted_backoff {
                        None => return Poll::Ready(result),
                        Some(dur) => {
                            (this.notify_fn)(&result, dur);
                            this.state =
                                State::Sleeping(this.sleep_fn.sleep(dur));
                        }
                    }
                }
                State::Sleeping(sl) => {
                    // Safety: This is safe because we don't move the
                    // `RetryWithResult` struct and this fut,
                    // only its internal state.
                    //
                    // We do the exactly same thing like `pin_project` but
                    // without depending on it directly.
                    let mut sl = unsafe { Pin::new_unchecked(sl) };

                    let _: () = ready!(sl.as_mut().poll(cx));
                    this.state = State::Idle;
                }
            }
        }
    }
}
