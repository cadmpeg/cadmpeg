// SPDX-License-Identifier: Apache-2.0
//! A readable table may be empty; an unread table has no value.
use std::ops::{Deref, DerefMut};

#[derive(Debug, Clone, PartialEq)]
pub(super) struct ReadTable<T>(Option<Vec<T>>);
impl<T> Default for ReadTable<T> {
    fn default() -> Self {
        Self(None)
    }
}
impl<T> From<Vec<T>> for ReadTable<T> {
    fn from(rows: Vec<T>) -> Self {
        Self(Some(rows))
    }
}
impl<T> From<Option<Vec<T>>> for ReadTable<T> {
    fn from(rows: Option<Vec<T>>) -> Self {
        Self(rows)
    }
}
impl<T> std::iter::FromIterator<T> for ReadTable<T> {
    fn from_iter<I: IntoIterator<Item = T>>(rows: I) -> Self {
        Self(Some(rows.into_iter().collect()))
    }
}
impl<T> Deref for ReadTable<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        self.0.as_deref().unwrap_or(&[])
    }
}
impl<T> ReadTable<T> {
    pub(super) fn as_read(&self) -> Option<&[T]> {
        self.0.as_deref()
    }
    pub(super) fn admit_empty(&mut self) {
        self.0.get_or_insert_with(Vec::new);
    }
    pub(super) fn push(&mut self, row: T) {
        self.0.get_or_insert_with(Vec::new).push(row);
    }
    pub(super) fn pop(&mut self) -> Option<T> {
        self.0.as_mut()?.pop()
    }
    pub(super) fn extend(&mut self, rows: impl IntoIterator<Item = T>) {
        self.0.get_or_insert_with(Vec::new).extend(rows);
    }
    pub(super) fn iter_mut(&mut self) -> std::slice::IterMut<'_, T> {
        self.0.as_deref_mut().unwrap_or(&mut []).iter_mut()
    }
}
impl<'a, T> IntoIterator for &'a ReadTable<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
impl<'a, T> IntoIterator for &'a mut ReadTable<T> {
    type Item = &'a mut T;
    type IntoIter = std::slice::IterMut<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

impl<T> DerefMut for ReadTable<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        self.0.as_deref_mut().unwrap_or(&mut [])
    }
}
