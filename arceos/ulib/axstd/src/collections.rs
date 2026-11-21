//! Collection types for allocation-enabled applications.

pub use alloc::collections::*;

use alloc::vec::Vec;
use core::{
    borrow::Borrow,
    hash::{Hash, Hasher},
};

/// A hash map for kernel applications. Its deterministic hasher is intended for trusted keys.
pub struct HashMap<K, V> {
    buckets: Vec<Vec<(K, V)>>,
    len: usize,
}

impl<K, V> HashMap<K, V> {
    pub const fn new() -> Self {
        Self {
            buckets: Vec::new(),
            len: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.buckets
            .iter()
            .flatten()
            .map(|(key, value)| (key, value))
    }
}

impl<K, V> Default for HashMap<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Eq + Hash, V> HashMap<K, V> {
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        if !self.buckets.is_empty() {
            let index = bucket_index(&key, self.buckets.len());
            if let Some((_, old)) = self.buckets[index].iter_mut().find(|(k, _)| *k == key) {
                return Some(core::mem::replace(old, value));
            }
        }
        if self.len == self.buckets.len() {
            let capacity = (self.buckets.len() * 2).max(8);
            let mut buckets = Vec::with_capacity(capacity);
            buckets.resize_with(capacity, Vec::new);
            for (key, value) in self.buckets.drain(..).flatten() {
                buckets[bucket_index(&key, capacity)].push((key, value));
            }
            self.buckets = buckets;
        }
        let index = bucket_index(&key, self.buckets.len());
        self.buckets[index].push((key, value));
        self.len += 1;
        None
    }

    pub fn get<Q: Eq + Hash + ?Sized>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
    {
        if self.buckets.is_empty() {
            return None;
        }
        self.buckets[bucket_index(key, self.buckets.len())]
            .iter()
            .find_map(|(k, value)| (k.borrow() == key).then_some(value))
    }

    pub fn remove<Q: Eq + Hash + ?Sized>(&mut self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
    {
        if self.buckets.is_empty() {
            return None;
        }
        let index = bucket_index(key, self.buckets.len());
        let bucket = &mut self.buckets[index];
        let entry = bucket.iter().position(|(k, _)| k.borrow() == key)?;
        self.len -= 1;
        Some(bucket.swap_remove(entry).1)
    }
}

fn bucket_index(key: &(impl Hash + ?Sized), capacity: usize) -> usize {
    let mut hasher = Fnv1a(0xcbf29ce484222325);
    key.hash(&mut hasher);
    hasher.finish() as usize % capacity
}

struct Fnv1a(u64);

impl Hasher for Fnv1a {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 = (self.0 ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        }
    }
}
