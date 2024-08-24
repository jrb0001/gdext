use std::future::Future;
use std::mem::ManuallyDrop;
use std::ops::DerefMut;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};
use std::thread::ThreadId;

use crate::builtin::{Callable, GString, Signal, Variant};
use crate::classes::object::ConnectFlags;
use crate::global::godot_error;
use crate::meta::FromGodot;
use crate::obj::EngineEnum;
use impl_trait_for_tuples::impl_for_tuples;

pub fn godot_task_local(future: impl Future<Output = ()> + 'static) {
    godot_task_sync(LocalFuture::new(future))
}

pub fn godot_task_sync(future: impl Future<Output = ()> + 'static + Send + Sync) {
    let waker = Arc::new(GodotWaker::new_sync(future));
    waker.wake();
}

struct GodotWaker {
    future: Mutex<Pin<Box<dyn Future<Output = ()> + 'static + Send + Sync>>>,
    again: AtomicBool,
}

impl GodotWaker {
    fn new_sync(future: impl Future<Output = ()> + 'static + Send + Sync) -> Self {
        Self {
            future: Mutex::new(Box::pin(future)),
            again: AtomicBool::new(false),
        }
    }
}

impl Wake for GodotWaker {
    fn wake(self: Arc<Self>) {
        let waker: Waker = self.clone().into();
        let mut ctx = Context::from_waker(&waker);

        // Flag must be set before locking to avoid race condition.
        self.again.store(true, Ordering::SeqCst);
        if let Ok(mut future) = self.future.try_lock() {
            while self.again.swap(false, Ordering::SeqCst) {
                let _ = future.as_mut().poll(&mut ctx);
            }
        }
    }
}

struct LocalFuture<F: Future + 'static> {
    future: ManuallyDrop<F>,
    thread: ThreadId,
}

impl<F: Future + 'static> LocalFuture<F> {
    fn new(future: F) -> Self {
        Self {
            future: ManuallyDrop::new(future),
            thread: std::thread::current().id(),
        }
    }
}

impl<F: Future + 'static> Future for LocalFuture<F> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        assert_eq!(self.thread, std::thread::current().id());
        unsafe { self.map_unchecked_mut(|s| s.future.deref_mut()) }.poll(cx)
    }
}

impl<F: Future + 'static> Drop for LocalFuture<F> {
    fn drop(&mut self) {
        if self.thread == std::thread::current().id() {
            unsafe { ManuallyDrop::drop(&mut self.future) };
        } else if std::thread::panicking() {
            godot_error!(
                "LocalFuture is dropped on another thread while panicking. Leaking inner Future to avoid abort."
            );
        } else {
            panic!("LocalFuture is dropped on another thread.");
        }
    }
}

// Verified at runtime by checking the current thread id.
unsafe impl<F: Future + 'static> Send for LocalFuture<F> {}
unsafe impl<F: Future + 'static> Sync for LocalFuture<F> {}

pub struct SignalFuture<R: FromSignalArgs> {
    state: Arc<Mutex<(Option<R>, Option<Waker>)>>,
}

impl<R: FromSignalArgs> SignalFuture<R> {
    fn new(name: impl Into<GString>, signal: Signal) -> Self {
        let state = Arc::new(Mutex::new((None, Option::<Waker>::None)));
        let callback_state = state.clone();

        // the callable currently requires that the return value is Sync + Send
        signal.connect(
            Callable::from_fn(name, move |args: &[&Variant]| {
                let mut lock = callback_state.lock().unwrap();
                let waker = lock.1.take();

                lock.0.replace(R::from_args(args));
                drop(lock);

                if let Some(waker) = waker {
                    waker.wake();
                }

                Ok(Variant::nil())
            }),
            ConnectFlags::ONE_SHOT.ord() as i64,
        );

        Self { state }
    }
}

impl<R: FromSignalArgs> Future for SignalFuture<R> {
    type Output = R;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut lock = self.state.lock().unwrap();

        if let Some(result) = lock.0.take() {
            return Poll::Ready(result);
        }

        lock.1.replace(cx.waker().clone());

        Poll::Pending
    }
}

pub trait FromSignalArgs: Sync + Send + 'static {
    fn from_args(args: &[&Variant]) -> Self;
}

#[impl_for_tuples(12)]
impl FromSignalArgs for Tuple {
    for_tuples!( where #(Tuple: FromGodot + Sync + Send + 'static),* );
    fn from_args(args: &[&Variant]) -> Self {
        let mut iter = args.iter();
        #[allow(clippy::unused_unit)]
        (for_tuples!(#(iter.next().unwrap().to()),*))
    }
}

// Signal should implement IntoFuture for convenience. Keeping ToSignalFuture around might still be desirable, though. It allows to reuse i
// the same signal instance multiple times.
pub trait ToSignalFuture<R: FromSignalArgs> {
    fn to_future(&self) -> SignalFuture<R>;
}

impl<R: FromSignalArgs> ToSignalFuture<R> for Signal {
    fn to_future(&self) -> SignalFuture<R> {
        SignalFuture::new(format!("Signal::{}", self), self.clone())
    }
}
