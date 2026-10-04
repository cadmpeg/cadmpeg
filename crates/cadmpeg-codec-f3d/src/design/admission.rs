// SPDX-License-Identifier: Apache-2.0
//! Typed admission for Design projections shared with writer validation.

use cadmpeg_core::decode::cost::DecodeCost;
use cadmpeg_core::decode::iter_source::IterSource;
use cadmpeg_core::decode::scan::AdmittedIter;
use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::index::StandardIndex;
use std::borrow::Borrow;
use std::collections::{btree_map::Entry, BTreeMap};
use std::convert::Infallible;
use std::fmt;

pub(crate) trait DesignStorage {
    fn with_storage<T>(&mut self, build: impl FnOnce() -> Result<T, CodecError>)
        -> Result<T, CodecError>;
}

impl DesignStorage for ScopedReservation<'_> {
    fn with_storage<T>(&mut self, build: impl FnOnce() -> Result<T, CodecError>)
        -> Result<T, CodecError> {
        ScopedReservation::with_storage(self, build)
    }
}

impl DesignStorage for () {
    fn with_storage<T>(&mut self, build: impl FnOnce() -> Result<T, CodecError>)
        -> Result<T, CodecError> {
        build()
    }
}

pub(crate) trait DesignAdmission {
    type Error;
    type Iter<'values, S: IterSource + ?Sized + 'values>:
        Iterator<Item = <S::Iter<'values> as Iterator>::Item> + 'values;
    type Storage<'ctx>: DesignStorage where Self: 'ctx;
    fn into_error(error: Self::Error) -> CodecError;
    fn reserve_scoped(&self, length: u64, operation: &'static str)
        -> Result<Self::Storage<'_>, Self::Error>;
    fn admit_iter<'values, S: IterSource + ?Sized>(&self, values: &'values S,
        operation: &'static str)
        -> Result<Self::Iter<'values, S>, Self::Error>;
    fn equal<T: DecodeCost + PartialEq + ?Sized>(&self, left: &T, right: &T,
        operation: &'static str) -> Result<bool, Self::Error>;
    fn entry_btree_map<'values, K: DecodeCost + Ord, V>(&self,
        values: &'values mut BTreeMap<K, V>, key: K, operation: &'static str)
        -> Result<Entry<'values, K, V>, Self::Error>;
    fn get_btree_map<'values, K, Q, V>(&self, values: &'values BTreeMap<K, V>, key: &Q,
        operation: &'static str) -> Result<Option<&'values V>, Self::Error>
        where K: Borrow<Q> + Ord, Q: DecodeCost + Ord + ?Sized;
    fn contains_key_btree_map<K, Q, V>(&self, values: &BTreeMap<K, V>, key: &Q,
        operation: &'static str) -> Result<bool, Self::Error>
        where K: Borrow<Q> + Ord, Q: DecodeCost + Ord + ?Sized;
    fn insert_btree_map<K: DecodeCost + Ord, V>(&self, values: &mut BTreeMap<K, V>, key: K,
        value: V, operation: &'static str) -> Result<Option<V>, Self::Error>;
    fn collect_vec<T>(&self, values: impl IntoIterator<Item = T>, operation: &'static str)
        -> Result<Vec<T>, Self::Error>;
    fn push_vec<T>(&self, values: &mut Vec<T>, value: T, operation: &'static str)
        -> Result<(), Self::Error>;
    fn copy_retained_text(&self, text: &str, operation: &'static str)
        -> Result<String, Self::Error>;
    fn copy_scoped_text<'ctx>(&self, text: &str, storage: &mut Self::Storage<'ctx>,
        operation: &'static str) -> Result<String, Self::Error> where Self: 'ctx;
    fn retained_string(&self, length: usize, operation: &'static str)
        -> Result<String, Self::Error>;
    fn push_retained_char(&self, text: &mut String, value: char, operation: &'static str)
        -> Result<(), Self::Error>;
    fn make_ascii_lowercase(&self, text: &mut str, operation: &'static str)
        -> Result<(), Self::Error>;
    fn format_retained(&self, arguments: fmt::Arguments<'_>, operation: &'static str)
        -> Result<String, Self::Error>;
    fn validate_nonblank_text(&self, text: String, operation: &'static str)
        -> Result<Option<NonBlankString>, Self::Error>;
    fn identifier_extent_error(&self, operation: &'static str) -> CodecError;
}

impl DesignAdmission for DecodeContext<'_> {
    type Error = CodecError;
    type Iter<'values, S: IterSource + ?Sized + 'values> = AdmittedIter<S::Iter<'values>>;
    type Storage<'ctx> = ScopedReservation<'ctx> where Self: 'ctx;
    fn into_error(error: CodecError) -> CodecError { error }
    fn reserve_scoped(&self, length: u64, operation: &'static str)
        -> Result<Self::Storage<'_>, CodecError> {
        DecodeContext::reserve_scoped(self, length, operation)
    }
    fn admit_iter<'values, S: IterSource + ?Sized>(&self, values: &'values S,
        operation: &'static str)
        -> Result<Self::Iter<'values, S>, CodecError> {
        Ok(DecodeContext::admit_iter(self, values, operation)?)
    }
    fn equal<T: DecodeCost + PartialEq + ?Sized>(&self, left: &T, right: &T,
        operation: &'static str) -> Result<bool, CodecError> {
        DecodeContext::equal(self, left, right, operation)
    }
    fn entry_btree_map<'values, K: DecodeCost + Ord, V>(&self,
        values: &'values mut BTreeMap<K, V>, key: K, operation: &'static str)
        -> Result<Entry<'values, K, V>, CodecError> {
        DecodeContext::entry_btree_map(self, values, key, operation)
    }
    fn get_btree_map<'values, K, Q, V>(&self, values: &'values BTreeMap<K, V>, key: &Q,
        operation: &'static str) -> Result<Option<&'values V>, CodecError>
        where K: Borrow<Q> + Ord, Q: DecodeCost + Ord + ?Sized {
        DecodeContext::get_btree_map(self, values, key, operation)
    }
    fn contains_key_btree_map<K, Q, V>(&self, values: &BTreeMap<K, V>, key: &Q,
        operation: &'static str) -> Result<bool, CodecError>
        where K: Borrow<Q> + Ord, Q: DecodeCost + Ord + ?Sized {
        DecodeContext::contains_key_btree_map(self, values, key, operation)
    }
    fn insert_btree_map<K: DecodeCost + Ord, V>(&self, values: &mut BTreeMap<K, V>, key: K,
        value: V, operation: &'static str) -> Result<Option<V>, CodecError> {
        DecodeContext::insert_btree_map(self, values, key, value, operation)
    }
    fn collect_vec<T>(&self, values: impl IntoIterator<Item = T>, operation: &'static str)
        -> Result<Vec<T>, CodecError> {
        DecodeContext::collect_vec(self, values, operation)
    }
    fn push_vec<T>(&self, values: &mut Vec<T>, value: T, operation: &'static str)
        -> Result<(), CodecError> {
        DecodeContext::push_vec(self, values, value, operation)
    }
    fn copy_retained_text(&self, text: &str, operation: &'static str) -> Result<String, CodecError> {
        DecodeContext::copy_retained_text(self, text, operation)
    }
    fn copy_scoped_text<'ctx>(&self, text: &str, storage: &mut Self::Storage<'ctx>,
        operation: &'static str) -> Result<String, CodecError> where Self: 'ctx {
        DecodeContext::copy_scoped_text(self, text, storage, operation)
    }
    fn retained_string(&self, length: usize, operation: &'static str) -> Result<String, CodecError> {
        DecodeContext::retained_string(self, length, operation)
    }
    fn push_retained_char(&self, text: &mut String, value: char, operation: &'static str)
        -> Result<(), CodecError> {
        DecodeContext::push_retained_char(self, text, value, operation)
    }
    fn make_ascii_lowercase(&self, text: &mut str, operation: &'static str)
        -> Result<(), CodecError> {
        DecodeContext::make_ascii_lowercase(self, text, operation)
    }
    fn format_retained(&self, arguments: fmt::Arguments<'_>, operation: &'static str)
        -> Result<String, CodecError> {
        DecodeContext::format_retained(self, arguments, operation)
    }
    fn validate_nonblank_text(&self, text: String, operation: &'static str)
        -> Result<Option<NonBlankString>, CodecError> {
        Ok(NonBlankString::for_decode(self, text, operation)?)
    }
    fn identifier_extent_error(&self, operation: &'static str) -> CodecError {
        self.refuse_codec_limit(operation, 0, 1)
    }
}

impl DesignAdmission for StandardIndex {
    type Error = Infallible;
    type Iter<'values, S: IterSource + ?Sized + 'values> = S::Iter<'values>;
    type Storage<'ctx> = ();
    fn into_error(error: Infallible) -> CodecError { match error {} }
    fn reserve_scoped(&self, _length: u64, _operation: &'static str)
        -> Result<(), Infallible> { Ok(()) }
    fn admit_iter<'values, S: IterSource + ?Sized>(&self, values: &'values S,
        _operation: &'static str)
        -> Result<Self::Iter<'values, S>, Infallible> {
        Ok(values.source_iter())
    }
    fn equal<T: DecodeCost + PartialEq + ?Sized>(&self, left: &T, right: &T,
        _operation: &'static str) -> Result<bool, Infallible> { Ok(left == right) }
    fn entry_btree_map<'values, K: DecodeCost + Ord, V>(&self,
        values: &'values mut BTreeMap<K, V>, key: K, _operation: &'static str)
        -> Result<Entry<'values, K, V>, Infallible> { Ok(values.entry(key)) }
    fn get_btree_map<'values, K, Q, V>(&self, values: &'values BTreeMap<K, V>, key: &Q,
        _operation: &'static str) -> Result<Option<&'values V>, Infallible>
        where K: Borrow<Q> + Ord, Q: DecodeCost + Ord + ?Sized { Ok(values.get(key)) }
    fn contains_key_btree_map<K, Q, V>(&self, values: &BTreeMap<K, V>, key: &Q,
        _operation: &'static str) -> Result<bool, Infallible>
        where K: Borrow<Q> + Ord, Q: DecodeCost + Ord + ?Sized { Ok(values.contains_key(key)) }
    fn insert_btree_map<K: DecodeCost + Ord, V>(&self, values: &mut BTreeMap<K, V>, key: K,
        value: V, _operation: &'static str) -> Result<Option<V>, Infallible> {
        Ok(values.insert(key, value))
    }
    fn collect_vec<T>(&self, values: impl IntoIterator<Item = T>, _operation: &'static str)
        -> Result<Vec<T>, Infallible> { Ok(values.into_iter().collect()) }
    fn push_vec<T>(&self, values: &mut Vec<T>, value: T, _operation: &'static str)
        -> Result<(), Infallible> { values.push(value); Ok(()) }
    fn copy_retained_text(&self, text: &str, _operation: &'static str)
        -> Result<String, Infallible> { Ok(text.to_owned()) }
    fn copy_scoped_text<'ctx>(&self, text: &str, _storage: &mut (), _operation: &'static str)
        -> Result<String, Infallible> where Self: 'ctx { Ok(text.to_owned()) }
    fn retained_string(&self, length: usize, _operation: &'static str)
        -> Result<String, Infallible> { Ok(String::with_capacity(length)) }
    fn push_retained_char(&self, text: &mut String, value: char, _operation: &'static str)
        -> Result<(), Infallible> { text.push(value); Ok(()) }
    fn make_ascii_lowercase(&self, text: &mut str, _operation: &'static str)
        -> Result<(), Infallible> { text.make_ascii_lowercase(); Ok(()) }
    fn format_retained(&self, arguments: fmt::Arguments<'_>, _operation: &'static str)
        -> Result<String, Infallible> { Ok(format!("{arguments}")) }
    fn validate_nonblank_text(&self, text: String, _operation: &'static str)
        -> Result<Option<NonBlankString>, Infallible> { Ok(text.try_into().ok()) }
    fn identifier_extent_error(&self, _operation: &'static str) -> CodecError {
        CodecError::malformed("Design identifier extent exceeds the addressable length")
    }
}
