use ipnetwork::IpNetwork;
use prefix_trie::joint::JointPrefixMap;
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

type Prefix = IpNetwork;
type PrefixMap<T> = JointPrefixMap<Prefix, Arc<T>>;

pub struct RoutingTable<T>(RwLock<PrefixMap<T>>);

impl<T> RoutingTable<T> {
    pub fn new() -> RoutingTable<T> {
        RoutingTable(RwLock::new(PrefixMap::new()))
    }

    pub fn read<'a>(&'a self) -> RwLockReadGuard<'a, PrefixMap<T>> {
        self.0.read().unwrap()
    }

    pub fn write<'a>(&'a self) -> RwLockWriteGuard<'a, PrefixMap<T>> {
        self.0.write().unwrap()
    }

    // clones a value that matches longest prefix
    pub fn get(&self, prefix: &Prefix) -> Option<Arc<T>> {
        self.read().get_lpm(prefix).map(|(_, v)| v.clone())
    }

    // checks if given prefix exists in the routing table
    pub fn contains(&self, prefix: &Prefix) -> bool {
        self.read().contains_key(prefix)
    }

    // inserts given value under given prefix and returns previous value
    pub fn insert(&self, prefix: Prefix, value: Arc<T>) -> Option<Arc<T>> {
        self.write().insert(prefix, value)
    }

    // removes a prefix with value from the routing table
    pub fn remove(&self, prefix: &Prefix) -> Option<Arc<T>> {
        self.write().remove(prefix)
    }
}
