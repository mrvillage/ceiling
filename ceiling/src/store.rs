use std::{collections::BinaryHeap, sync::Mutex};

use dashmap::DashMap;
use sero::{LockGuard, LockStore};

/// The default store implementation if none is specified when creating a rate limiter.
/// The default implementation uses `dashmap::DashMap` to store buckets, `sero::LockStore` to store locks,
/// and a `std::collections::BinaryHeap` containing the expiry times for pruning expired buckets.
#[derive(Debug)]
pub struct DefaultStore {
    map: DashMap<String, (u32, u64)>,
    locks: LockStore<String>,
    expiring: Mutex<BinaryHeap<Expiry>>,
}

#[derive(Debug, PartialEq, Eq)]
struct Expiry(pub(crate) u64, pub(crate) String);

impl PartialOrd for Expiry {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        other.0.partial_cmp(&self.0)
    }

    #[inline]
    fn lt(&self, other: &Self) -> bool {
        other.0 < self.0
    }

    #[inline]
    fn le(&self, other: &Self) -> bool {
        other.0 <= self.0
    }

    #[inline]
    fn gt(&self, other: &Self) -> bool {
        other.0 > self.0
    }

    #[inline]
    fn ge(&self, other: &Self) -> bool {
        other.0 >= self.0
    }
}

impl Ord for Expiry {
    #[inline]
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.0.cmp(&self.0)
    }
}

impl SyncStore for DefaultStore {
    type Lock = DefaultStoreLock;

    fn new() -> Self
    where
        Self: Sized,
    {
        Self {
            map: DashMap::new(),
            locks: LockStore::new(),
            expiring: Mutex::new(BinaryHeap::new()),
        }
    }

    fn get(&self, key: &str) -> Self::Lock {
        let guard = self.locks.lock(key.into()).wait();
        let value = self.map.get(key).map(|v| *v);
        Self::Lock::new(value, guard)
    }

    fn set(&self, key: &str, value: (u32, u64), reset_updated: bool) {
        self.map.insert(key.to_string(), value);
        if reset_updated {
            let mut lock = self.expiring.lock().unwrap();
            lock.push(Expiry(value.1 + 1, key.to_string()));
        }
    }

    fn remove(&self, key: &str) {
        self.map.remove(key);
    }

    fn prune(&self, now: u64) {
        let mut expiring = self.expiring.lock().unwrap();
        loop {
            let peek = expiring.peek();
            if let Some(peek) = peek {
                if peek.0 < now {
                    break;
                }
                let key = &expiring.pop().unwrap().1;
                let lock = self.get(key);
                let item = *lock;
                if let Some(item) = item {
                    if item.1 < now {
                        continue;
                    }
                    self.remove(key);
                }
            }
        }
    }
}

/// The trait providing the required methods for a synchronous store of buckets.
pub trait SyncStore: std::fmt::Debug + Send + Sync {
    /// The type of the Lock returned from `SyncStore::get`, must implement `ceiling::StoreLock`.
    type Lock: StoreLock;

    /// Creates a new store
    fn new() -> Self
    where
        Self: Sized;
    /// Gets a bucket from the store, the return value must implement `ceiling::StoreLock`
    fn get(&self, key: &str) -> Self::Lock;
    /// Sets the value of a bucket in the store.
    /// If reset_updated is true then the u64 reset value was updated. This may be helpful for internal implementations of `SyncStore::prune`.
    fn set(&self, key: &str, value: (u32, u64), reset_updated: bool);
    /// Removes a bucket from the store.
    fn remove(&self, key: &str);
    /// Prunes the store of any expired values. Any bucket with a reset value less than the provided now value is considered expired.
    fn prune(&self, now: u64);
}
///
#[cfg(feature = "async")]
#[async_trait::async_trait]
pub trait AsyncStore: std::fmt::Debug + Send + Sync {
    /// The type of the Lock returned from `AsyncStore::get`, must implement `ceiling::StoreLock`.
    type Lock: StoreLock;

    /// Creates a new store
    fn new() -> Self
    where
        Self: Sized;
    /// Gets a bucket from the store, the return value must implement `ceiling::StoreLock`
    async fn get(&self, key: &str) -> Self::Lock;
    /// Sets the value of a bucket in the store.
    /// If reset_updated is true then the u64 reset value was updated. This may be helpful for internal implementations of `AsyncStore::prune`.
    async fn set(&self, key: &str, value: (u32, u64), reset_updated: bool);
    /// Removes a bucket from the store.
    async fn remove(&self, key: &str);
    /// Prunes the store of any expired values. Any bucket with a reset value less than the provided now value is considered expired.
    async fn prune(&self, now: u64);
}

/// The implementor of this trait is expected to dereference into an Option<(u32, u64)> with the items
/// in the tuple corresponding to the remaining requests and the reset time in seconds respectively.
/// While an instance of this trait is alive the corresponding rate limiting bucket is considered locked and
/// no changes should be made until the implementor is dropped, meaning the lock has been released.
pub trait StoreLock:
    std::ops::Deref<Target = Option<(u32, u64)>> + std::fmt::Debug + Send + Sync
{
}

/// The default implementation of `StoreLock` for use with `DefaultStore`.
#[derive(Debug)]
pub struct DefaultStoreLock {
    value: Option<(u32, u64)>,
    _guard: LockGuard<String>,
}

impl StoreLock for DefaultStoreLock {}

impl std::ops::Deref for DefaultStoreLock {
    type Target = Option<(u32, u64)>;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl DefaultStoreLock {
    /// Creates a new `DefaultStoreLock`
    pub fn new(value: Option<(u32, u64)>, guard: LockGuard<String>) -> Self {
        Self {
            value,
            _guard: guard,
        }
    }
}
