// SPDX-License-Identifier: Apache-2.0
//! Source offsets of a native record as the `record_layout` pointers of its
//! capture.
//!
//! A decoded record holds the source offsets of its fields in memory. The
//! document wire does not carry them: they are the pointers of the record's
//! capture in the decode sidecar. One walk names every offset by its pointer
//! path. Collecting writes the decoded offsets to a pointer map; restoring
//! reads a record read from the wire back to the offsets its sidecar states.
//! An offset whose pointer is absent stays absent. A restore also names
//! each pointer that a located record must have and the map lacks: every
//! pointer except that of an optional offset.

use std::collections::BTreeMap;

use super::identity::{ReferenceRun, Sourced};

/// A native record whose fields carry source offsets.
pub(crate) trait SourceOffsets {
    /// Visit every source offset of the record under its pointer name.
    fn visit_source_offsets(&mut self, offsets: &mut OffsetVisitor<'_>);
}

enum Mode<'a> {
    Collect(&'a mut BTreeMap<String, u64>),
    Restore(&'a BTreeMap<String, u64>, Vec<String>),
}

/// The walk over the source offsets of one record.
pub(crate) struct OffsetVisitor<'a> {
    path: String,
    mode: Mode<'a>,
}

impl<'a> OffsetVisitor<'a> {
    /// A walk that writes every present offset to `pointers`.
    pub(crate) fn collect(pointers: &'a mut BTreeMap<String, u64>) -> Self {
        Self {
            path: String::new(),
            mode: Mode::Collect(pointers),
        }
    }

    /// A walk that sets every offset to its pointer in `pointers`, or to
    /// absent when the pointer is absent.
    pub(crate) fn restore(pointers: &'a BTreeMap<String, u64>) -> Self {
        Self {
            path: String::new(),
            mode: Mode::Restore(pointers, Vec::new()),
        }
    }

    fn require(&mut self, key: String) {
        if let Mode::Restore(pointers, missing) = &mut self.mode {
            if !pointers.contains_key(&key) {
                missing.push(key);
            }
        }
    }

    fn key(&self, name: &str) -> String {
        if self.path.is_empty() {
            name.to_owned()
        } else {
            format!("{}.{name}", self.path)
        }
    }

    /// One offset that a located record holds.
    pub(crate) fn offset(&mut self, name: &str, slot: &mut Option<u64>) {
        self.optional_offset(name, slot);
        let key = self.key(name);
        self.require(key);
    }

    /// One offset that a located record holds only with the value it
    /// locates.
    pub(crate) fn optional_offset(&mut self, name: &str, slot: &mut Option<u64>) {
        let key = self.key(name);
        match &mut self.mode {
            Mode::Collect(pointers) => {
                if let Some(offset) = slot {
                    pointers.insert(key, *offset);
                }
            }
            Mode::Restore(pointers, _) => *slot = pointers.get(&key).copied(),
        }
    }

    /// A fixed run of offsets that a located record holds, named `name.0`
    /// to `name.N-1`. A run restores only when every pointer of it is
    /// present.
    pub(crate) fn offsets<const N: usize>(&mut self, name: &str, slot: &mut Option<[u64; N]>) {
        let keys: [String; N] = std::array::from_fn(|index| self.key(&format!("{name}.{index}")));
        match &mut self.mode {
            Mode::Collect(pointers) => {
                if let Some(offsets) = slot {
                    for (key, offset) in keys.into_iter().zip(*offsets) {
                        pointers.insert(key, offset);
                    }
                }
            }
            Mode::Restore(pointers, missing) => {
                let mut offsets = [0; N];
                let mut absent = Vec::new();
                for (offset, key) in offsets.iter_mut().zip(keys) {
                    match pointers.get(&key) {
                        Some(pointer) => *offset = *pointer,
                        None => absent.push(key),
                    }
                }
                *slot = absent.is_empty().then_some(offsets);
                missing.extend(absent);
            }
        }
    }

    /// The offset of one sourced value.
    pub(crate) fn sourced<T>(&mut self, name: &str, value: &mut Sourced<T>) {
        self.offset(name, &mut value.offset);
    }

    /// The offsets of a run of sourced values, named `name.0` onward.
    pub(crate) fn sourced_each<T>(&mut self, name: &str, values: &mut [Sourced<T>]) {
        for (index, value) in values.iter_mut().enumerate() {
            self.sourced(&format!("{name}.{index}"), value);
        }
    }

    /// The encoding locations of an ordered reference run, named `name.0`
    /// onward. A run restores its locations only when every pointer of it is
    /// present.
    pub(crate) fn reference_run<T>(&mut self, name: &str, run: &mut ReferenceRun<T>) {
        match &mut self.mode {
            Mode::Collect(_) => {
                let keys: Vec<(String, u64)> = run
                    .offsets()
                    .enumerate()
                    .map(|(index, offset)| (self.key(&format!("{name}.{index}")), *offset))
                    .collect();
                if let Mode::Collect(pointers) = &mut self.mode {
                    pointers.extend(keys);
                }
            }
            Mode::Restore(..) => {
                let keys: Vec<String> = (0..run.len())
                    .map(|index| self.key(&format!("{name}.{index}")))
                    .collect();
                if let Mode::Restore(pointers, _) = &self.mode {
                    let restored: Option<Vec<u64>> =
                        keys.iter().map(|key| pointers.get(key).copied()).collect();
                    run.set_offsets(restored);
                }
                for key in keys {
                    self.require(key);
                }
            }
        }
    }

    /// A nested record under `name`.
    pub(crate) fn nested(&mut self, name: &str, visit: impl FnOnce(&mut Self)) {
        let parent = std::mem::replace(&mut self.path, String::new());
        self.path = if parent.is_empty() {
            name.to_owned()
        } else {
            format!("{parent}.{name}")
        };
        visit(self);
        self.path = parent;
    }

    /// A nested record under `name`.
    pub(crate) fn record<T: SourceOffsets + ?Sized>(&mut self, name: &str, value: &mut T) {
        self.nested(name, |offsets| value.visit_source_offsets(offsets));
    }

    /// A nested record under `name`, when present.
    pub(crate) fn optional<T: SourceOffsets>(&mut self, name: &str, value: &mut Option<T>) {
        if let Some(value) = value {
            self.record(name, value);
        }
    }

    /// A run of nested records, named `name.0` onward.
    pub(crate) fn each<T: SourceOffsets>(&mut self, name: &str, values: &mut [T]) {
        for (index, value) in values.iter_mut().enumerate() {
            self.record(&format!("{name}.{index}"), value);
        }
    }
}

impl<T: SourceOffsets + ?Sized> SourceOffsets for Box<T> {
    fn visit_source_offsets(&mut self, offsets: &mut OffsetVisitor<'_>) {
        (**self).visit_source_offsets(offsets);
    }
}

/// The source offsets of `record`, by pointer name.
pub(crate) fn collect<T: SourceOffsets + ?Sized>(record: &mut T) -> BTreeMap<String, u64> {
    let mut pointers = BTreeMap::new();
    record.visit_source_offsets(&mut OffsetVisitor::collect(&mut pointers));
    pointers
}

/// Set every source offset of `record` to its pointer in `pointers`, and
/// return the pointers that the located record must have and `pointers`
/// lacks.
pub(crate) fn restore<T: SourceOffsets + ?Sized>(
    record: &mut T,
    pointers: &BTreeMap<String, u64>,
) -> Vec<String> {
    let mut visitor = OffsetVisitor::restore(pointers);
    record.visit_source_offsets(&mut visitor);
    match visitor.mode {
        Mode::Restore(_, missing) => missing,
        Mode::Collect(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests;
