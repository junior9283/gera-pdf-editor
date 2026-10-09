use std::collections::HashMap;
use std::hash::Hash;

/// Peso (em bytes) de um valor no cache. Deve ser constante enquanto o valor estiver guardado:
/// o cache soma o peso na inserção e o subtrai na remoção.
pub trait Weighed {
    fn weight(&self) -> usize;
}

/// Cache LRU limitado por bytes. O item recém-inserido nunca é removido, mesmo acima do orçamento.
pub struct LruCache<K, V> {
    map: HashMap<K, (V, u64)>,
    tick: u64,
    bytes: usize,
    budget: usize,
}

impl<K: Hash + Eq + Clone, V: Weighed> LruCache<K, V> {
    pub fn new(budget_bytes: usize) -> Self {
        Self {
            map: HashMap::new(),
            tick: 0,
            bytes: 0,
            budget: budget_bytes,
        }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
    pub fn bytes(&self) -> usize {
        self.bytes
    }
    pub fn contains(&self, k: &K) -> bool {
        self.map.contains_key(k)
    }
    /// Lê sem alterar a recência.
    pub fn peek(&self, k: &K) -> Option<&V> {
        self.map.get(k).map(|e| &e.0)
    }
    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.map.keys()
    }

    pub fn get(&mut self, k: &K) -> Option<&V> {
        self.tick += 1;
        let tick = self.tick;
        self.map.get_mut(k).map(|e| {
            e.1 = tick;
            &e.0
        })
    }

    pub fn insert(&mut self, k: K, v: V) {
        self.tick += 1;
        self.bytes += v.weight();
        if let Some((old, _)) = self.map.insert(k, (v, self.tick)) {
            self.bytes -= old.weight();
        }
        while self.bytes > self.budget && self.map.len() > 1 {
            let Some(oldest) = self
                .map
                .iter()
                .min_by_key(|(_, e)| e.1)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            if let Some((v, _)) = self.map.remove(&oldest) {
                self.bytes -= v.weight();
            }
        }
    }

    /// Mantém só as entradas cuja chave satisfaz `keep`.
    pub fn retain(&mut self, mut keep: impl FnMut(&K) -> bool) {
        let bytes = &mut self.bytes;
        self.map.retain(|k, (v, _)| {
            let ok = keep(k);
            if !ok {
                *bytes -= v.weight();
            }
            ok
        });
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.bytes = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Blob(usize);
    impl Weighed for Blob {
        fn weight(&self) -> usize {
            self.0
        }
    }

    #[test]
    fn evicts_least_recently_used_over_budget() {
        let mut c = LruCache::new(100);
        c.insert("a", Blob(40));
        c.insert("b", Blob(40));
        assert!(c.get(&"a").is_some()); // "a" fica mais recente que "b"
        c.insert("c", Blob(40)); // 120 > 100 => remove "b"
        assert!(c.contains(&"a") && c.contains(&"c") && !c.contains(&"b"));
        assert_eq!(c.bytes(), 80);
    }

    #[test]
    fn newest_item_is_kept_even_if_larger_than_budget() {
        let mut c = LruCache::new(10);
        c.insert("a", Blob(5));
        c.insert("big", Blob(50));
        assert!(c.contains(&"big") && !c.contains(&"a"));
        assert_eq!(c.len(), 1);
    }

    #[test]
    fn replacing_a_key_updates_byte_accounting() {
        let mut c = LruCache::new(100);
        c.insert("a", Blob(30));
        c.insert("a", Blob(70));
        assert_eq!((c.len(), c.bytes()), (1, 70));
    }

    #[test]
    fn peek_does_not_refresh_and_clear_empties() {
        let mut c = LruCache::new(80);
        c.insert("a", Blob(40));
        c.insert("b", Blob(40));
        assert!(c.peek(&"a").is_some()); // não protege "a"
        c.insert("c", Blob(40)); // remove "a" (o mais antigo)
        assert!(!c.contains(&"a"));
        assert_eq!(c.keys().count(), 2);
        c.clear();
        assert!(c.is_empty() && c.bytes() == 0);
    }

    #[test]
    fn retain_drops_entries_and_their_bytes() {
        let mut c = LruCache::new(100);
        c.insert(1, Blob(30));
        c.insert(2, Blob(20));
        c.retain(|k| *k != 1);
        assert!(!c.contains(&1) && c.contains(&2));
        assert_eq!(c.bytes(), 20);
    }
}
