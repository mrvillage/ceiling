use std::{collections::BinaryHeap, sync::Mutex};

use dashmap::DashMap;
use sero::{LockGuard, LockStore};

#[derive(Debug)]
pub struct DefaultStore {
    map: DashMap<String, (u32, u64)>,
    locks: LockStore<String>,
    expiring: Mutex<BinaryHeap<Expiry>>,
}

#[derive(Debug, PartialEq, Eq)]
struct Expiry(pub u64, pub String);

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

pub trait SyncStore: std::fmt::Debug + Send + Sync {
    type Lock: StoreLock;

    fn new() -> Self
    where
        Self: Sized;
    fn get(&self, key: &str) -> Self::Lock;
    // Sets the value at key to value, if reset_updated is true then the u64 reset value was updated. This may be helpful for internal implementations of Self::prune.
    fn set(&self, key: &str, value: (u32, u64), reset_updated: bool);
    fn remove(&self, key: &str);
    // Prunes the store of any expired values.
    fn prune(&self, now: u64);
}

#[cfg(feature = "async")]
#[async_trait::async_trait]
pub trait AsyncStore: std::fmt::Debug + Send + Sync {
    type Lock: StoreLock;

    fn new() -> Self
    where
        Self: Sized;
    async fn get(&self, key: &str) -> Self::Lock;
    // Sets the value at key to value, if reset_updated is true then the u64 reset value was updated. This may be helpful for internal implementations of Self::prune.
    async fn set(&self, key: &str, value: (u32, u64), reset_updated: bool);
    async fn remove(&self, key: &str);
    // Prunes the store of any expired values.
    async fn prune(&self, now: u64);
}

/// The lock should be released when the implementor is dropped
pub trait StoreLock:
    std::ops::Deref<Target = Option<(u32, u64)>> + std::fmt::Debug + Send + Sync
{
}

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
    pub fn new(value: Option<(u32, u64)>, guard: LockGuard<String>) -> Self {
        Self {
            value,
            _guard: guard,
        }
    }
}
