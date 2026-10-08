// SPDX-License-Identifier: Apache-2.0
//! Typed admission for the record and schema code shared by decode and writers.
//!
//! Decode passes its `&DecodeContext`, whose operations charge the session
//! budget before the work they admit. Writers pass [`StandardAdmission`],
//! which performs the same operations with standard allocation and never
//! refuses: its failure type is [`Infallible`]. A format ceiling, such as the
//! counted-value recovery limit, is a property of the bytes and refuses under
//! both admissions.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::convert::Infallible;
use std::fmt;
use std::io::{Cursor, Read};

use cadmpeg_container::ArchiveSnapshot;
use cadmpeg_core::decode::iter_source::IterSource;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, DepthGuard, ScopedReservation, View};
use cadmpeg_core::CodecError;

use crate::MAX_SCHEMA_BYTES;

/// Storage and work admission for Protein schema and record decoding.
pub trait ProteinAdmission: Copy {
    /// Refusal produced by the admission policy.
    type Error;
    /// Owner of temporary storage that lives until it is dropped.
    type Scope;
    /// One admitted nesting level, released when dropped.
    type Depth;
    /// The nested Protein archive this admission reads schemas from.
    type Archive<'bytes>;

    /// Opens an empty temporary storage owner.
    fn scope(self, operation: &'static str) -> Result<Self::Scope, Self::Error>;

    /// Runs `build` with the storage it allocates held by `scope`.
    fn scoped<T>(
        self,
        scope: &mut Self::Scope,
        build: impl FnOnce() -> Result<T, CodecError>,
    ) -> Result<T, CodecError>;

    /// Admits a fixed number of work steps.
    fn work(self, units: usize, operation: &'static str) -> Result<(), Self::Error>;

    /// Admits one step of a fixed-step source, including the end probe.
    fn next<I: Iterator>(
        self,
        values: &mut I,
        operation: &'static str,
    ) -> Result<Option<I::Item>, Self::Error>;

    /// Admits a complete traversal of a counted source before its first visit.
    fn traverse<S: IterSource>(
        self,
        values: S,
        operation: &'static str,
    ) -> Result<impl Iterator<Item = <S::Iter as Iterator>::Item>, Self::Error>;

    /// Validates UTF-8 after admitting every byte.
    fn validate_utf8<'bytes>(
        self,
        bytes: &'bytes [u8],
        operation: &'static str,
    ) -> Result<Result<&'bytes str, std::str::Utf8Error>, Self::Error>;

    /// Copies text into owned storage.
    fn copy_text(self, text: &str, operation: &'static str) -> Result<String, Self::Error>;

    /// Formats text into owned storage.
    fn format_text(
        self,
        args: fmt::Arguments<'_>,
        operation: &'static str,
    ) -> Result<String, Self::Error>;

    /// Searches text for a pattern.
    fn contains_text(
        self,
        text: &str,
        pattern: &str,
        operation: &'static str,
    ) -> Result<bool, Self::Error>;

    /// The refusal for a count or size above a Protein format ceiling.
    fn format_ceiling(self, operation: &'static str, limit: u64, requested: u64) -> CodecError;

    /// Builds `count` values, admitting their slots before the first one.
    fn collect_indexed<T>(
        self,
        count: usize,
        operation: &'static str,
        value_at: impl FnMut(usize) -> Result<T, CodecError>,
    ) -> Result<Vec<T>, CodecError>;

    /// Appends one value.
    fn push<T>(
        self,
        values: &mut Vec<T>,
        value: T,
        operation: &'static str,
    ) -> Result<(), Self::Error>;

    /// Appends copied bytes.
    fn extend_bytes(
        self,
        values: &mut Vec<u8>,
        source: &[u8],
        operation: &'static str,
    ) -> Result<(), Self::Error>;

    /// Inserts into a name-keyed B-tree map, returning a replaced value.
    fn insert_btree_map<V>(
        self,
        values: &mut BTreeMap<String, V>,
        key: String,
        value: V,
        operation: &'static str,
    ) -> Result<Option<V>, Self::Error>;

    /// Looks up a name-keyed B-tree map for mutation.
    fn get_mut_btree_map<'values, V>(
        self,
        values: &'values mut BTreeMap<String, V>,
        key: &str,
        operation: &'static str,
    ) -> Result<Option<&'values mut V>, Self::Error>;

    /// Inserts into a name-keyed hash map, returning a replaced value.
    fn insert_hash_map<V>(
        self,
        values: &mut HashMap<String, V>,
        key: String,
        value: V,
        operation: &'static str,
    ) -> Result<Option<V>, Self::Error>;

    /// Looks up a name-keyed hash map.
    fn get_hash_map<'values, V>(
        self,
        values: &'values HashMap<String, V>,
        key: &str,
        operation: &'static str,
    ) -> Result<Option<&'values V>, Self::Error>;

    /// Tests a name-keyed hash map for a key.
    fn contains_key_hash_map<V>(
        self,
        values: &HashMap<String, V>,
        key: &str,
        operation: &'static str,
    ) -> Result<bool, Self::Error>;

    /// Inserts a borrowed name into a set, returning whether it was absent.
    fn insert_name<'name>(
        self,
        names: &mut BTreeSet<&'name str>,
        name: &'name str,
        operation: &'static str,
    ) -> Result<bool, Self::Error>;

    /// Tests a set of borrowed names.
    fn contains_name(
        self,
        names: &BTreeSet<&str>,
        name: &str,
        operation: &'static str,
    ) -> Result<bool, Self::Error>;

    /// Enters one nesting level.
    fn enter_nested(self, operation: &'static str) -> Result<Self::Depth, Self::Error>;

    /// Parses XML and runs `read` over the document. The outer error is the
    /// parse failure or refusal; the inner result is `read`'s.
    fn with_xml<T>(
        self,
        text: &str,
        operation: &'static str,
        read: impl FnOnce(&roxmltree::Document<'_>) -> Result<T, CodecError>,
    ) -> Result<Result<T, CodecError>, CodecError>;

    /// The first element child of the document root.
    fn xml_root<'node, 'input>(
        self,
        document: &'node roxmltree::Document<'input>,
        operation: &'static str,
    ) -> Result<Option<roxmltree::Node<'node, 'input>>, Self::Error>;

    /// The value of the first attribute whose local name is `name`.
    fn xml_attribute<'node>(
        self,
        node: roxmltree::Node<'node, '_>,
        name: &str,
        operation: &'static str,
    ) -> Result<Option<&'node str>, Self::Error>;

    /// Visits every schema document in the archive, in archive order, with
    /// its entry name and XML bytes.
    fn read_schemas(
        self,
        protein: Self::Archive<'_>,
        visit: impl FnMut(&str, &[u8]) -> Result<(), CodecError>,
    ) -> Result<(), CodecError>;
}

/// Whether an archive entry name is a packaged schema document. The fixed
/// suffix test runs first, so only schema-named entries pay for the search.
pub(crate) fn is_schema_entry<A: ProteinAdmission>(
    admission: A,
    name: &str,
) -> Result<bool, A::Error> {
    Ok(name.ends_with("Schema.xml")
        && (name.starts_with("Schemas/")
            || admission.contains_text(name, "/Schemas/", "Protein schema entry path search")?))
}

impl<'ctx, 'input> ProteinAdmission for &'ctx DecodeContext<'input> {
    type Error = CodecError;
    type Scope = ScopedReservation<'ctx>;
    type Depth = DepthGuard<'ctx>;
    type Archive<'bytes> = View<'input>;

    fn scope(self, operation: &'static str) -> Result<Self::Scope, CodecError> {
        self.reserve_scoped(0, operation)
    }

    fn scoped<T>(
        self,
        scope: &mut Self::Scope,
        build: impl FnOnce() -> Result<T, CodecError>,
    ) -> Result<T, CodecError> {
        scope.with_storage(build)
    }

    fn work(self, units: usize, operation: &'static str) -> Result<(), CodecError> {
        self.charge_work(u64_from_index(units), operation)
    }

    fn next<I: Iterator>(
        self,
        values: &mut I,
        operation: &'static str,
    ) -> Result<Option<I::Item>, CodecError> {
        self.next_charged(values, operation)
    }

    fn traverse<S: IterSource>(
        self,
        values: S,
        operation: &'static str,
    ) -> Result<impl Iterator<Item = <S::Iter as Iterator>::Item>, CodecError> {
        Ok(self.admit_iter(values, operation)?)
    }

    fn validate_utf8<'bytes>(
        self,
        bytes: &'bytes [u8],
        operation: &'static str,
    ) -> Result<Result<&'bytes str, std::str::Utf8Error>, CodecError> {
        DecodeContext::validate_utf8(self, bytes, operation)
    }

    fn copy_text(self, text: &str, operation: &'static str) -> Result<String, CodecError> {
        self.copy_retained_text(text, operation)
    }

    fn format_text(
        self,
        args: fmt::Arguments<'_>,
        operation: &'static str,
    ) -> Result<String, CodecError> {
        self.format_retained(args, operation)
    }

    fn contains_text(
        self,
        text: &str,
        pattern: &str,
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        DecodeContext::contains_text(self, text, pattern, operation)
    }

    fn format_ceiling(self, operation: &'static str, limit: u64, requested: u64) -> CodecError {
        self.refuse_codec_limit(operation, limit, requested)
    }

    fn collect_indexed<T>(
        self,
        count: usize,
        operation: &'static str,
        value_at: impl FnMut(usize) -> Result<T, CodecError>,
    ) -> Result<Vec<T>, CodecError> {
        self.collect_indexed_vec(count, operation, value_at)
    }

    fn push<T>(
        self,
        values: &mut Vec<T>,
        value: T,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.push_vec(values, value, operation)
    }

    fn extend_bytes(
        self,
        values: &mut Vec<u8>,
        source: &[u8],
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.extend_from_slice(values, source, operation)
    }

    fn insert_btree_map<V>(
        self,
        values: &mut BTreeMap<String, V>,
        key: String,
        value: V,
        operation: &'static str,
    ) -> Result<Option<V>, CodecError> {
        DecodeContext::insert_btree_map(self, values, key, value, operation)
    }

    fn get_mut_btree_map<'values, V>(
        self,
        values: &'values mut BTreeMap<String, V>,
        key: &str,
        operation: &'static str,
    ) -> Result<Option<&'values mut V>, CodecError> {
        DecodeContext::get_mut_btree_map(self, values, key, operation)
    }

    fn insert_hash_map<V>(
        self,
        values: &mut HashMap<String, V>,
        key: String,
        value: V,
        operation: &'static str,
    ) -> Result<Option<V>, CodecError> {
        DecodeContext::insert_hash_map(self, values, key, value, operation)
    }

    fn get_hash_map<'values, V>(
        self,
        values: &'values HashMap<String, V>,
        key: &str,
        operation: &'static str,
    ) -> Result<Option<&'values V>, CodecError> {
        DecodeContext::get_hash_map(self, values, key, operation)
    }

    fn contains_key_hash_map<V>(
        self,
        values: &HashMap<String, V>,
        key: &str,
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        DecodeContext::contains_key_hash_map(self, values, key, operation)
    }

    fn insert_name<'name>(
        self,
        names: &mut BTreeSet<&'name str>,
        name: &'name str,
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        self.insert_btree_set(names, name, operation)
    }

    fn contains_name(
        self,
        names: &BTreeSet<&str>,
        name: &str,
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        self.contains_btree_set(names, name, operation)
    }

    fn enter_nested(self, operation: &'static str) -> Result<Self::Depth, CodecError> {
        DecodeContext::enter_nested(self, operation)
    }

    fn with_xml<T>(
        self,
        text: &str,
        operation: &'static str,
        read: impl FnOnce(&roxmltree::Document<'_>) -> Result<T, CodecError>,
    ) -> Result<Result<T, CodecError>, CodecError> {
        let admitted = self.parse_xml(text, operation)?;
        Ok(read(admitted.document()))
    }

    fn xml_root<'node, 'doc>(
        self,
        document: &'node roxmltree::Document<'doc>,
        operation: &'static str,
    ) -> Result<Option<roxmltree::Node<'node, 'doc>>, CodecError> {
        match self.xml_root_element(document, operation) {
            Ok(root) => Ok(Some(root)),
            Err(CodecError::Malformed(_)) => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn xml_attribute<'node>(
        self,
        node: roxmltree::Node<'node, '_>,
        name: &str,
        operation: &'static str,
    ) -> Result<Option<&'node str>, CodecError> {
        DecodeContext::xml_attribute(self, node, name, operation)
    }

    fn read_schemas(
        self,
        protein: View<'input>,
        mut visit: impl FnMut(&str, &[u8]) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        let archive = ArchiveSnapshot::new(self, protein)?;
        for entry in self.admit_iter(archive.entries(), "Protein schema entries")? {
            if !is_schema_entry(self, &entry.name)? {
                continue;
            }
            if entry.uncompressed_size > MAX_SCHEMA_BYTES {
                return Err(self.refuse_codec_limit(
                    "Protein schema bytes",
                    MAX_SCHEMA_BYTES,
                    entry.uncompressed_size,
                ));
            }
            let xml = archive.open(self, &entry.name)?;
            visit(&entry.name, xml.window())?;
        }
        Ok(())
    }
}

/// Standard allocation for writers: the same operations, never refused.
#[derive(Clone, Copy, Debug, Default)]
pub struct StandardAdmission;

impl ProteinAdmission for StandardAdmission {
    type Error = Infallible;
    type Scope = ();
    type Depth = ();
    type Archive<'bytes> = &'bytes [u8];

    fn scope(self, _operation: &'static str) -> Result<(), Infallible> {
        Ok(())
    }

    fn scoped<T>(
        self,
        _scope: &mut (),
        build: impl FnOnce() -> Result<T, CodecError>,
    ) -> Result<T, CodecError> {
        build()
    }

    fn work(self, _units: usize, _operation: &'static str) -> Result<(), Infallible> {
        Ok(())
    }

    fn next<I: Iterator>(
        self,
        values: &mut I,
        _operation: &'static str,
    ) -> Result<Option<I::Item>, Infallible> {
        Ok(values.next())
    }

    fn traverse<S: IterSource>(
        self,
        values: S,
        _operation: &'static str,
    ) -> Result<impl Iterator<Item = <S::Iter as Iterator>::Item>, Infallible> {
        Ok(values.source_iter())
    }

    fn validate_utf8<'bytes>(
        self,
        bytes: &'bytes [u8],
        _operation: &'static str,
    ) -> Result<Result<&'bytes str, std::str::Utf8Error>, Infallible> {
        Ok(std::str::from_utf8(bytes))
    }

    fn copy_text(self, text: &str, _operation: &'static str) -> Result<String, Infallible> {
        Ok(text.to_owned())
    }

    fn format_text(
        self,
        args: fmt::Arguments<'_>,
        _operation: &'static str,
    ) -> Result<String, Infallible> {
        Ok(args.to_string())
    }

    fn contains_text(
        self,
        text: &str,
        pattern: &str,
        _operation: &'static str,
    ) -> Result<bool, Infallible> {
        Ok(text.contains(pattern))
    }

    fn format_ceiling(self, operation: &'static str, limit: u64, requested: u64) -> CodecError {
        CodecError::malformed(format_args!(
            "{operation} requests {requested}, above the format limit {limit}"
        ))
    }

    fn collect_indexed<T>(
        self,
        count: usize,
        _operation: &'static str,
        value_at: impl FnMut(usize) -> Result<T, CodecError>,
    ) -> Result<Vec<T>, CodecError> {
        (0..count).map(value_at).collect()
    }

    fn push<T>(
        self,
        values: &mut Vec<T>,
        value: T,
        _operation: &'static str,
    ) -> Result<(), Infallible> {
        values.push(value);
        Ok(())
    }

    fn extend_bytes(
        self,
        values: &mut Vec<u8>,
        source: &[u8],
        _operation: &'static str,
    ) -> Result<(), Infallible> {
        values.extend_from_slice(source);
        Ok(())
    }

    fn insert_btree_map<V>(
        self,
        values: &mut BTreeMap<String, V>,
        key: String,
        value: V,
        _operation: &'static str,
    ) -> Result<Option<V>, Infallible> {
        Ok(values.insert(key, value))
    }

    fn get_mut_btree_map<'values, V>(
        self,
        values: &'values mut BTreeMap<String, V>,
        key: &str,
        _operation: &'static str,
    ) -> Result<Option<&'values mut V>, Infallible> {
        Ok(values.get_mut(key))
    }

    fn insert_hash_map<V>(
        self,
        values: &mut HashMap<String, V>,
        key: String,
        value: V,
        _operation: &'static str,
    ) -> Result<Option<V>, Infallible> {
        Ok(values.insert(key, value))
    }

    fn get_hash_map<'values, V>(
        self,
        values: &'values HashMap<String, V>,
        key: &str,
        _operation: &'static str,
    ) -> Result<Option<&'values V>, Infallible> {
        Ok(values.get(key))
    }

    fn contains_key_hash_map<V>(
        self,
        values: &HashMap<String, V>,
        key: &str,
        _operation: &'static str,
    ) -> Result<bool, Infallible> {
        Ok(values.contains_key(key))
    }

    fn insert_name<'name>(
        self,
        names: &mut BTreeSet<&'name str>,
        name: &'name str,
        _operation: &'static str,
    ) -> Result<bool, Infallible> {
        Ok(names.insert(name))
    }

    fn contains_name(
        self,
        names: &BTreeSet<&str>,
        name: &str,
        _operation: &'static str,
    ) -> Result<bool, Infallible> {
        Ok(names.contains(name))
    }

    fn enter_nested(self, _operation: &'static str) -> Result<(), Infallible> {
        Ok(())
    }

    fn with_xml<T>(
        self,
        text: &str,
        _operation: &'static str,
        read: impl FnOnce(&roxmltree::Document<'_>) -> Result<T, CodecError>,
    ) -> Result<Result<T, CodecError>, CodecError> {
        let document = roxmltree::Document::parse(text).map_err(CodecError::malformed)?;
        Ok(read(&document))
    }

    fn xml_root<'node, 'doc>(
        self,
        document: &'node roxmltree::Document<'doc>,
        _operation: &'static str,
    ) -> Result<Option<roxmltree::Node<'node, 'doc>>, Infallible> {
        Ok(document.root().children().find(roxmltree::Node::is_element))
    }

    fn xml_attribute<'node>(
        self,
        node: roxmltree::Node<'node, '_>,
        name: &str,
        _operation: &'static str,
    ) -> Result<Option<&'node str>, Infallible> {
        Ok(node
            .attributes()
            .find(|attribute| attribute.name() == name)
            .map(|attribute| attribute.value()))
    }

    fn read_schemas(
        self,
        protein: &[u8],
        mut visit: impl FnMut(&str, &[u8]) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        let mut archive = zip::ZipArchive::new(Cursor::new(protein)).map_err(|error| {
            CodecError::malformed(format_args!("cannot open nested Protein ZIP: {error}"))
        })?;
        let mut names = BTreeSet::new();
        for index in 0..archive.len() {
            let entry = archive.by_index(index).map_err(|error| {
                CodecError::malformed(format_args!("cannot read nested Protein entry: {error}"))
            })?;
            let name = entry.name().to_owned();
            if !names.insert(name.clone()) {
                return Err(CodecError::malformed(format_args!(
                    "duplicate ZIP entry name {name}"
                )));
            }
            if !is_schema_entry(self, &name).unwrap_or_else(|never| match never {}) {
                continue;
            }
            if entry.size() > MAX_SCHEMA_BYTES {
                return Err(self.format_ceiling(
                    "Protein schema bytes",
                    MAX_SCHEMA_BYTES,
                    entry.size(),
                ));
            }
            let mut xml = Vec::new();
            entry.take(MAX_SCHEMA_BYTES).read_to_end(&mut xml)?;
            visit(&name, &xml)?;
        }
        Ok(())
    }
}
