use std::future::Future;
use std::sync::Arc;
use std::task::Wake;

use crate::builtin::{Signal, Variant};
use crate::meta::FromGodot;
use impl_trait_for_tuples::impl_for_tuples;

use future::{GodotWaker, SignalFuture};
use wrapper::{Local, Send, Wrapper};

pub fn godot_task_local(future: impl Future<Output = ()> + 'static) {
    godot_task_send(Local::new(future))
}

pub fn godot_task_send(future: impl Future<Output = ()> + 'static + std::marker::Send) {
    let waker = Arc::new(GodotWaker::new_send(future));
    waker.wake();
}

pub trait FromSignalArgs: 'static + Sized {
    fn from_args(args: &[&Variant]) -> Self;
}

#[impl_for_tuples(2)]
#[tuple_types_custom_trait_bound(FromGodot + 'static)]
impl FromSignalArgs for Tuple {
    fn from_args(args: &[&Variant]) -> Self {
        let mut iter = args.iter();
        #[allow(clippy::unused_unit)]
        (for_tuples!(#(iter.next().unwrap().to()),*))
    }
}

pub type LocalSignalFuture<R> = SignalFuture<R, Local<R>>;
pub type SendSignalFuture<R> = SignalFuture<R, Send<R>>;

// Signal should implement IntoFuture for convenience. Keeping ToSignalFuture around might still be desirable, though. It allows to reuse i
// the same signal instance multiple times.
pub trait ToSignalFuture {
    fn to_local_future<R: FromSignalArgs>(&self) -> LocalSignalFuture<R>;

    fn to_send_future<R: FromSignalArgs + std::marker::Send>(&self) -> SendSignalFuture<R>;
}

impl ToSignalFuture for Signal {
    fn to_local_future<R: FromSignalArgs>(&self) -> LocalSignalFuture<R> {
        SignalFuture::new(format!("Signal::{}", self), self.clone())
    }

    fn to_send_future<R: FromSignalArgs + std::marker::Send>(&self) -> SendSignalFuture<R> {
        SignalFuture::new(format!("Signal::{}", self), self.clone())
    }
}

mod future {
    use std::future::Future;
    use std::marker::PhantomData;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Wake, Waker};

    use crate::builtin::{Callable, GString, Signal, Variant};
    use crate::classes::object::ConnectFlags;
    use crate::obj::EngineEnum;

    use super::wrapper::{Send, Wrapper};
    use super::FromSignalArgs;

    pub struct SignalFuture<R, W>
    where
        R: FromSignalArgs,
        W: Wrapper<R>,
    {
        state: Arc<Mutex<(Option<W>, Option<Waker>)>>,
        _phantom: PhantomData<(*mut (), R)>,
    }

    impl<R, W> SignalFuture<R, W>
    where
        R: FromSignalArgs,
        W: Wrapper<R>,
    {
        pub(super) fn new(name: impl Into<GString>, signal: Signal) -> Self {
            let state = Arc::new(Mutex::new((None, Option::<Waker>::None)));
            let callback_state = state.clone();

            signal.connect(
                Callable::from_fn(name, move |args: &[&Variant]| {
                    let mut lock = callback_state.lock().unwrap();
                    let waker = lock.1.take();

                    lock.0.replace(W::new(R::from_args(args)));
                    drop(lock);

                    if let Some(waker) = waker {
                        waker.wake();
                    }

                    Ok(Variant::nil())
                }),
                ConnectFlags::ONE_SHOT.ord() as i64,
            );

            Self {
                state,
                _phantom: PhantomData,
            }
        }
    }

    impl<R, W> Future for SignalFuture<R, W>
    where
        R: FromSignalArgs,
        W: Wrapper<R>,
    {
        type Output = R;

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let mut lock = self.state.lock().unwrap();

            if let Some(result) = lock.0.take() {
                return Poll::Ready(result.into_inner());
            }

            lock.1.replace(cx.waker().clone());

            Poll::Pending
        }
    }

    unsafe impl<R: FromSignalArgs + std::marker::Send> std::marker::Send for SignalFuture<R, Send<R>> {}
    unsafe impl<R: FromSignalArgs + std::marker::Send> std::marker::Sync for SignalFuture<R, Send<R>> {}

    pub struct GodotWaker {
        future: Mutex<Pin<Box<dyn Future<Output = ()> + 'static + std::marker::Send>>>,
    }

    impl GodotWaker {
        pub(super) fn new_send(
            future: impl Future<Output = ()> + 'static + std::marker::Send,
        ) -> Self {
            Self {
                future: Mutex::new(Box::pin(future)),
            }
        }
    }

    impl Wake for GodotWaker {
        fn wake(self: Arc<Self>) {
            let waker: Waker = self.clone().into();
            let mut ctx = Context::from_waker(&waker);

            let mut future = self.future.lock().unwrap();
            let _ = future.as_mut().poll(&mut ctx);
        }
    }
}

mod wrapper {
    use std::future::Future;
    use std::mem::ManuallyDrop;
    use std::ops::DerefMut;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use std::thread::ThreadId;

    use crate::builtin::Variant;
    use crate::global::godot_error;

    use super::FromSignalArgs;

    pub trait Wrapper<T>: Sized + std::marker::Send + 'static {
        fn new(inner: T) -> Self;
        fn into_inner(self) -> T;
    }
    pub struct Local<T> {
        inner: ManuallyDrop<T>,
        thread: ThreadId,
    }

    impl<T: 'static> Wrapper<T> for Local<T> {
        fn new(inner: T) -> Self {
            Self {
                inner: ManuallyDrop::new(inner),
                thread: std::thread::current().id(),
            }
        }

        fn into_inner(self) -> T {
            let mut this = ManuallyDrop::new(self);
            unsafe { ManuallyDrop::take(&mut this.inner) }
        }
    }

    impl<T: FromSignalArgs + 'static> FromSignalArgs for Local<T> {
        fn from_args(args: &[&Variant]) -> Self {
            Local::new(T::from_args(args))
        }
    }

    impl<F: Future + 'static> Future for Local<F> {
        type Output = F::Output;

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            assert_eq!(self.thread, std::thread::current().id());
            unsafe { self.map_unchecked_mut(|s| s.inner.deref_mut()) }.poll(cx)
        }
    }

    impl<T> Drop for Local<T> {
        fn drop(&mut self) {
            if self.thread == std::thread::current().id() {
                unsafe { ManuallyDrop::drop(&mut self.inner) };
            } else if std::thread::panicking() {
                godot_error!(
                "Local is dropped on another thread while panicking. Leaking inner Future to avoid abort."
            );
            } else {
                panic!("Local is dropped on another thread.");
            }
        }
    }

    // Verified at runtime by checking the current thread id.
    unsafe impl<T> std::marker::Send for Local<T> {}

    pub struct Send<T> {
        inner: T,
    }

    impl<T: std::marker::Send + 'static> Wrapper<T> for Send<T> {
        fn new(inner: T) -> Self {
            Self { inner }
        }

        fn into_inner(self) -> T {
            self.inner
        }
    }
}
