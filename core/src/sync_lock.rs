//! A thin wrapper around `tokio::sync::RwLock` that offers the same blocking
//! `read()` / `write()` API provided by `parking_lot::RwLock`.  This allows the
//! existing synchronous call-sites to remain unchanged while migrating the
//! underlying implementation to Tokio – eliminating the risk of executor
//! starvation caused by `parking_lot`’s non-async aware locks.

use tokio::sync::{RwLock as TokioRwLock, RwLockReadGuard, RwLockWriteGuard};
use std::ops::{Deref, DerefMut};

/// Tokio-based read-write lock with blocking + async accessors.
pub struct RwLock<T>(TokioRwLock<T>);

impl<T> RwLock<T> {
    /// Create a new lock containing `value`.
    pub fn new(value: T) -> Self {
        Self(TokioRwLock::new(value))
    }

    /// Acquire a blocking read guard.  When already inside a Tokio runtime
    /// thread we first call `tokio::task::block_in_place` to avoid the runtime
    /// panic that occurs when using *blocking* APIs directly on a worker
    /// thread.
    pub fn read(&self) -> impl Deref<Target = T> + '_ {
        if tokio::runtime::Handle::try_current().is_ok() {
            tokio::task::block_in_place(|| self.0.blocking_read())
        } else {
            self.0.blocking_read()
        }
    }

    /// Acquire a blocking write guard.  See `read` for rationale.
    pub fn write(&self) -> impl DerefMut<Target = T> + '_ {
        if tokio::runtime::Handle::try_current().is_ok() {
            tokio::task::block_in_place(|| self.0.blocking_write())
        } else {
            self.0.blocking_write()
        }
    }

    /// Acquire an async read guard.  Prefer this inside async contexts.
    pub async fn read_async(&self) -> RwLockReadGuard<'_, T> {
        self.0.read().await
    }

    /// Acquire an async write guard.  Prefer this inside async contexts.
    pub async fn write_async(&self) -> RwLockWriteGuard<'_, T> {
        self.0.write().await
    }

    /// Consume the lock and return the underlying value.  Panics if there are
    /// outstanding guards (same semantics as `tokio::sync::RwLock::into_inner`).
    pub fn into_inner(self) -> T {
        self.0.into_inner()
    }
}

impl<T: Default> Default for RwLock<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}
