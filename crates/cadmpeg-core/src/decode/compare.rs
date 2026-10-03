// SPDX-License-Identifier: Apache-2.0
//! Charged equality, ordering, and collection lookup.

use super::cost::DecodeCost;
use super::DecodeContext;
use crate::CodecError;
use std::borrow::Borrow;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::hash::{BuildHasher, Hash};

impl DecodeContext<'_> {
    /// Compares values after admitting both operands and their owned children.
    pub fn equal<T: DecodeCost + PartialEq + ?Sized>(
        &self,
        left: &T,
        right: &T,
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        self.charge_key(left, 1, operation)?;
        self.charge_key(right, 1, operation)?;
        Ok(left == right)
    }
    /// Orders values after admitting both operands and their owned children.
    pub fn compare<T: DecodeCost + Ord + ?Sized>(
        &self,
        left: &T,
        right: &T,
        operation: &'static str,
    ) -> Result<std::cmp::Ordering, CodecError> {
        self.charge_key(left, 1, operation)?;
        self.charge_key(right, 1, operation)?;
        Ok(left.cmp(right))
    }
    /// Tests slice membership through the single position-search implementation.
    pub fn contains<T: DecodeCost + PartialEq>(
        &self,
        values: &[T],
        value: &T,
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        self.any_by(
            values,
            |candidate| self.equal(candidate, value, operation),
            operation,
        )
    }
    /// Admits key work before `HashMap::get`.
    pub fn get_hash_map<'values, K, Q, V, S>(
        &self,
        values: &'values HashMap<K, V, S>,
        key: &Q,
        operation: &'static str,
    ) -> Result<Option<&'values V>, CodecError>
    where
        K: Borrow<Q> + Eq + Hash,
        Q: DecodeCost + Eq + Hash + ?Sized,
        S: BuildHasher,
    {
        self.charge_key(key, 1, operation)?;
        Ok(values.get(key))
    }
    /// Admits key work before `HashMap::get_mut`.
    pub fn get_mut_hash_map<'values, K, Q, V, S>(
        &self,
        values: &'values mut HashMap<K, V, S>,
        key: &Q,
        operation: &'static str,
    ) -> Result<Option<&'values mut V>, CodecError>
    where
        K: Borrow<Q> + Eq + Hash,
        Q: DecodeCost + Eq + Hash + ?Sized,
        S: BuildHasher,
    {
        self.charge_key(key, 1, operation)?;
        Ok(values.get_mut(key))
    }
    /// Admits key work before `HashMap::contains_key`.
    pub fn contains_key_hash_map<K, Q, V, S>(
        &self,
        values: &HashMap<K, V, S>,
        key: &Q,
        operation: &'static str,
    ) -> Result<bool, CodecError>
    where
        K: Borrow<Q> + Eq + Hash,
        Q: DecodeCost + Eq + Hash + ?Sized,
        S: BuildHasher,
    {
        self.charge_key(key, 1, operation)?;
        Ok(values.contains_key(key))
    }
    /// Admits key work before `HashMap::remove`.
    pub fn remove_hash_map<K, Q, V, S>(
        &self,
        values: &mut HashMap<K, V, S>,
        key: &Q,
        operation: &'static str,
    ) -> Result<Option<V>, CodecError>
    where
        K: Borrow<Q> + Eq + Hash,
        Q: DecodeCost + Eq + Hash + ?Sized,
        S: BuildHasher,
    {
        self.charge_key(key, 1, operation)?;
        Ok(values.remove(key))
    }
    /// Admits key work before `HashSet::contains`.
    pub fn contains_hash_set<K, Q, S>(
        &self,
        values: &HashSet<K, S>,
        key: &Q,
        operation: &'static str,
    ) -> Result<bool, CodecError>
    where
        K: Borrow<Q> + Eq + Hash,
        Q: DecodeCost + Eq + Hash + ?Sized,
        S: BuildHasher,
    {
        self.charge_key(key, 1, operation)?;
        Ok(values.contains(key))
    }
    /// Admits key work before borrowing a hash set's stored value.
    pub fn get_hash_set<'values, K, Q, S>(
        &self,
        values: &'values HashSet<K, S>,
        key: &Q,
        operation: &'static str,
    ) -> Result<Option<&'values K>, CodecError>
    where
        K: Borrow<Q> + Eq + Hash,
        Q: DecodeCost + Eq + Hash + ?Sized,
        S: BuildHasher,
    {
        self.charge_key(key, 1, operation)?;
        Ok(values.get(key))
    }
    /// Admits key work before `HashSet::remove`.
    pub fn remove_hash_set<K, Q, S>(
        &self,
        values: &mut HashSet<K, S>,
        key: &Q,
        operation: &'static str,
    ) -> Result<bool, CodecError>
    where
        K: Borrow<Q> + Eq + Hash,
        Q: DecodeCost + Eq + Hash + ?Sized,
        S: BuildHasher,
    {
        self.charge_key(key, 1, operation)?;
        Ok(values.remove(key))
    }
    /// Admits key work before `BTreeMap::get`.
    pub fn get_btree_map<'values, K, Q, V>(
        &self,
        values: &'values BTreeMap<K, V>,
        key: &Q,
        operation: &'static str,
    ) -> Result<Option<&'values V>, CodecError>
    where
        K: Borrow<Q> + Ord,
        Q: DecodeCost + Ord + ?Sized,
    {
        self.charge_key(key, Self::tree_comparisons(values.len()), operation)?;
        Ok(values.get(key))
    }
    /// Admits key work before `BTreeMap::get_mut`.
    pub fn get_mut_btree_map<'values, K, Q, V>(
        &self,
        values: &'values mut BTreeMap<K, V>,
        key: &Q,
        operation: &'static str,
    ) -> Result<Option<&'values mut V>, CodecError>
    where
        K: Borrow<Q> + Ord,
        Q: DecodeCost + Ord + ?Sized,
    {
        self.charge_key(key, Self::tree_comparisons(values.len()), operation)?;
        Ok(values.get_mut(key))
    }
    /// Admits key work before `BTreeMap::contains_key`.
    pub fn contains_key_btree_map<K, Q, V>(
        &self,
        values: &BTreeMap<K, V>,
        key: &Q,
        operation: &'static str,
    ) -> Result<bool, CodecError>
    where
        K: Borrow<Q> + Ord,
        Q: DecodeCost + Ord + ?Sized,
    {
        self.charge_key(key, Self::tree_comparisons(values.len()), operation)?;
        Ok(values.contains_key(key))
    }
    /// Admits key work before `BTreeMap::remove`.
    pub fn remove_btree_map<K, Q, V>(
        &self,
        values: &mut BTreeMap<K, V>,
        key: &Q,
        operation: &'static str,
    ) -> Result<Option<V>, CodecError>
    where
        K: Borrow<Q> + Ord,
        Q: DecodeCost + Ord + ?Sized,
    {
        self.charge_key(key, Self::tree_comparisons(values.len()), operation)?;
        self.admit_tree_mutation::<K, V>(values.len(), operation)?;
        Ok(values.remove(key))
    }
    /// Admits key work before `BTreeSet::contains`.
    pub fn contains_btree_set<K, Q>(
        &self,
        values: &BTreeSet<K>,
        key: &Q,
        operation: &'static str,
    ) -> Result<bool, CodecError>
    where
        K: Borrow<Q> + Ord,
        Q: DecodeCost + Ord + ?Sized,
    {
        self.charge_key(key, Self::tree_comparisons(values.len()), operation)?;
        Ok(values.contains(key))
    }
    /// Admits key work before borrowing a tree set's stored value.
    pub fn get_btree_set<'values, K, Q>(
        &self,
        values: &'values BTreeSet<K>,
        key: &Q,
        operation: &'static str,
    ) -> Result<Option<&'values K>, CodecError>
    where
        K: Borrow<Q> + Ord,
        Q: DecodeCost + Ord + ?Sized,
    {
        self.charge_key(key, Self::tree_comparisons(values.len()), operation)?;
        Ok(values.get(key))
    }
    /// Hashes the complete value after admitting its bytes and child traversal.
    pub fn hash_value<T: DecodeCost + Hash + ?Sized>(
        &self,
        value: &T,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        use std::hash::Hasher;
        self.charge_key(value, 1, operation)?;
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        value.hash(&mut hash);
        Ok(hash.finish())
    }
    /// Admits key work before `BTreeSet::remove`.
    pub fn remove_btree_set<K, Q>(
        &self,
        values: &mut BTreeSet<K>,
        key: &Q,
        operation: &'static str,
    ) -> Result<bool, CodecError>
    where
        K: Borrow<Q> + Ord,
        Q: DecodeCost + Ord + ?Sized,
    {
        self.charge_key(key, Self::tree_comparisons(values.len()), operation)?;
        self.admit_tree_mutation::<K, ()>(values.len(), operation)?;
        Ok(values.remove(key))
    }
    /// Admits the query key before borrowing the stored key and value.
    pub fn get_key_value_hash_map<'values, K, Q, V, S>(
        &self,
        values: &'values HashMap<K, V, S>,
        key: &Q,
        operation: &'static str,
    ) -> Result<Option<(&'values K, &'values V)>, CodecError>
    where
        K: Borrow<Q> + Eq + Hash,
        Q: DecodeCost + Eq + Hash + ?Sized,
        S: BuildHasher,
    {
        self.charge_key(key, 1, operation)?;
        Ok(values.get_key_value(key))
    }
    /// Admits the query key before removing the stored key and value.
    pub fn remove_entry_hash_map<K, Q, V, S>(
        &self,
        values: &mut HashMap<K, V, S>,
        key: &Q,
        operation: &'static str,
    ) -> Result<Option<(K, V)>, CodecError>
    where
        K: Borrow<Q> + Eq + Hash,
        Q: DecodeCost + Eq + Hash + ?Sized,
        S: BuildHasher,
    {
        self.charge_key(key, 1, operation)?;
        Ok(values.remove_entry(key))
    }
    /// Tests set separation through admitted traversal and complete-key lookup.
    pub fn is_disjoint_hash_set<T: DecodeCost + Eq + Hash, S: BuildHasher>(
        &self,
        left: &HashSet<T, S>,
        right: &HashSet<T, S>,
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        let (source, target) = if left.len() <= right.len() {
            (left, right)
        } else {
            (right, left)
        };
        for value in self.admit_iter(source, operation)? {
            if self.contains_hash_set(target, value, operation)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
    /// Tests set containment through admitted traversal and complete-key lookup.
    pub fn is_subset_hash_set<T: DecodeCost + Eq + Hash, S: BuildHasher>(
        &self,
        left: &HashSet<T, S>,
        right: &HashSet<T, S>,
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        if left.len() > right.len() {
            return Ok(false);
        }
        let (source, target) = (left, right);
        for value in self.admit_iter(source, operation)? {
            if !self.contains_hash_set(target, value, operation)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
    /// Admits the query key before borrowing the stored key and value.
    pub fn get_key_value_btree_map<'values, K, Q, V>(
        &self,
        values: &'values BTreeMap<K, V>,
        key: &Q,
        operation: &'static str,
    ) -> Result<Option<(&'values K, &'values V)>, CodecError>
    where
        K: Borrow<Q> + Ord,
        Q: DecodeCost + Ord + ?Sized,
    {
        self.charge_key(key, Self::tree_comparisons(values.len()), operation)?;
        Ok(values.get_key_value(key))
    }
    /// Admits the query key before removing the stored key and value.
    pub fn remove_entry_btree_map<K, Q, V>(
        &self,
        values: &mut BTreeMap<K, V>,
        key: &Q,
        operation: &'static str,
    ) -> Result<Option<(K, V)>, CodecError>
    where
        K: Borrow<Q> + Ord,
        Q: DecodeCost + Ord + ?Sized,
    {
        self.charge_key(key, Self::tree_comparisons(values.len()), operation)?;
        self.admit_tree_mutation::<K, V>(values.len(), operation)?;
        Ok(values.remove_entry(key))
    }
    /// Tests set separation through admitted traversal and complete-key lookup.
    pub fn is_disjoint_btree_set<T: DecodeCost + Ord>(
        &self,
        left: &BTreeSet<T>,
        right: &BTreeSet<T>,
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        let (source, target) = if left.len() <= right.len() {
            (left, right)
        } else {
            (right, left)
        };
        for value in self.admit_iter(source, operation)? {
            if self.contains_btree_set(target, value, operation)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
    /// Tests set containment through admitted traversal and complete-key lookup.
    pub fn is_subset_btree_set<T: DecodeCost + Ord>(
        &self,
        left: &BTreeSet<T>,
        right: &BTreeSet<T>,
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        if left.len() > right.len() {
            return Ok(false);
        }
        let (source, target) = (left, right);
        for value in self.admit_iter(source, operation)? {
            if !self.contains_btree_set(target, value, operation)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
    /// Admits lookup and new entry storage before returning an entry.
    pub fn entry_hash_map<'values, K: DecodeCost + Eq + Hash, V>(
        &self,
        values: &'values mut std::collections::HashMap<K, V>,
        key: K,
        operation: &'static str,
    ) -> Result<std::collections::hash_map::Entry<'values, K, V>, CodecError> {
        self.admit_hash_map_entry(values, &key, operation)?;
        self.charge_key(&key, 1, operation)?;
        Ok(values.entry(key))
    }
    /// Admits lookup and new entry storage before returning an entry.
    pub fn entry_btree_map<'values, K: DecodeCost + Ord, V>(
        &self,
        values: &'values mut std::collections::BTreeMap<K, V>,
        key: K,
        operation: &'static str,
    ) -> Result<std::collections::btree_map::Entry<'values, K, V>, CodecError> {
        self.admit_btree_entry(values, &key, operation)?;
        self.charge_key(&key, Self::tree_comparisons(values.len()), operation)?;
        Ok(values.entry(key))
    }
}

#[cfg(test)]
mod tests;
