// SPDX-License-Identifier: Apache-2.0
//! Retained-document serialization: the ZIP repack and the `Document.xml`
//! patch.
//!
//! What to write is decided in [`target`], which is the one resolution gate this
//! codec has. This module carries out what that gate settled. An edit the patch
//! cannot hold stays as the source stores it, and a retained record the archive
//! cannot hold is left out; the report names each (§3.4, Paths item 4).

pub(crate) mod target;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::{Seek, SeekFrom, Write};

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{edit_dropped, evaluation_stale, NativeDispositions};
use cadmpeg_ir::native::NativeNamespace;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::schema::EntityKind;
use serde::de::DeserializeOwned;
use zip::write::SimpleFileOptions;

use crate::native::{
    ArchiveSpan, ArchiveSpanRole, EntryRecord, ExtensionRecord, ObjectRecord, PropertyBody,
    PropertyRecord, ValueRecord,
};
use crate::persistence::{Graph, Vocabulary};
use target::Resolution;

trait WriteSeek: Write + Seek {}
impl<T: Write + Seek> WriteSeek for T {}

fn write(output: &mut dyn Write, resolution: &Resolution<'_>) -> Result<WriteOutcome, CodecError> {
    let mut staged = tempfile::tempfile()?;
    let report = write_seekable(&mut staged, resolution)?;
    staged.seek(SeekFrom::Start(0))?;
    std::io::copy(&mut staged, output)?;
    Ok(report)
}

const UNREADABLE_ENTRY: &str = "whose archive entry record does not read";
const DUPLICATE_ENTRY: &str = "whose archive entry name an earlier entry record takes";
const UNREAD_PAYLOAD: &str = "whose ZIP member has no archive entry record";
const UNHELD_PAYLOAD: &str =
    "as the payload entries of these properties, which the archive does not hold; each written \
     property names no entry";
const UNREADABLE_SPAN: &str =
    "whose physical ledger span does not read; the archive member order it states is not kept";

/// Native records the archive leaves out, by the reason the write omits them.
#[derive(Debug, Default, Clone)]
struct Omissions(BTreeMap<&'static str, Vec<String>>);

impl Omissions {
    fn push(&mut self, reason: &'static str, id: impl Into<String>) {
        self.0.entry(reason).or_default().push(id.into());
    }

    /// Declare each omitted record dropped, so the write wrapper charges it.
    fn declare(&self, native: &mut NativeDispositions) {
        for (reason, ids) in &self.0 {
            native.drop_records("fcstd", reason, ids);
        }
    }
}

/// Pass on a refusal §3.4 admits (a resource ceiling, destination I/O); any
/// other failure is the caller's to degrade.
fn recover(error: CodecError) -> Result<(), CodecError> {
    match error {
        CodecError::ResourceLimit(_) | CodecError::Io(_) => Err(error),
        _ => Ok(()),
    }
}

/// The records of `arena` that read as `T`, and the ids of those that do not.
fn read_arena<T: DeserializeOwned>(
    namespace: &NativeNamespace,
    arena: &str,
) -> Result<(Vec<T>, Vec<String>), CodecError> {
    let ids = namespace
        .arenas()
        .get(arena)
        .into_iter()
        .flatten()
        .map(cadmpeg_ir::native::NativeRecord::id);
    let mut read = Vec::new();
    let mut unreadable = Vec::new();
    for (id, record) in ids.zip(namespace.arena_iter_as::<T>(arena)) {
        match record {
            Ok(record) => read.push(record),
            Err(error) => {
                recover(error.into())?;
                unreadable.push(id.to_owned());
            }
        }
    }
    Ok((read, unreadable))
}

/// Repack the retained entry set with a patched `Document.xml`.
///
/// The replay law is already settled when this runs. A [`Resolution`] comes only
/// from [`target::retained_baseline`], so the dialect written here is the one
/// the retained `Document.xml` declares.
fn write_seekable(
    output: &mut dyn WriteSeek,
    resolution: &Resolution<'_>,
) -> Result<WriteOutcome, CodecError> {
    let namespace = resolution.namespace();
    let entries = resolution.entries();
    let mut omitted = resolution.omitted().clone();
    let (physical_ledger, unreadable_spans) =
        read_arena::<ArchiveSpan>(namespace, "physical_ledger")?;
    for id in unreadable_spans {
        omitted.push(UNREADABLE_SPAN, id);
    }
    let (objects, unreadable_objects) = read_arena::<ObjectRecord>(namespace, "objects")?;
    let (extensions, unreadable_extensions) =
        read_arena::<ExtensionRecord>(namespace, "extensions")?;
    let (properties, unreadable_properties) =
        read_arena::<PropertyRecord>(namespace, "properties")?;
    let mut omitted_members = resolution.omitted_members().clone();
    for span in &physical_ledger {
        if let ArchiveSpanRole::CompressedPayload(name) = &span.role {
            if name != "Document.xml"
                && !entries.iter().any(|entry| entry.name() == name)
                && omitted_members.insert(name.clone())
            {
                omitted.push(UNREAD_PAYLOAD, span.id.clone());
            }
        }
    }
    let written = written_document(resolution, &objects, &extensions, &properties)?;
    for id in written.unheld {
        omitted.push(UNHELD_PAYLOAD, id);
    }
    let mut dropped = written.dropped;
    dropped.extend(unreadable_objects);
    dropped.extend(unreadable_extensions);
    dropped.extend(unreadable_properties);

    {
        let mut archive = zip::ZipWriter::new(&mut *output);
        let file_options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .last_modified_time(zip::DateTime::default());
        let ordered = std::iter::once(("Document.xml", written.xml.as_bytes())).chain(
            entry_order(entries, &physical_ledger)
                .into_iter()
                .map(|entry| (entry.name(), entry.data())),
        );
        for (name, data) in ordered {
            archive
                .start_file(name, file_options)
                .map_err(|error| std::io::Error::other(format!("cannot write {name}: {error}")))?;
            archive.write_all(data)?;
        }
        archive.finish().map_err(|error| {
            std::io::Error::other(format!("cannot finish FCStd archive: {error}"))
        })?;
    }
    let mut losses = edit_dropped(
        EntityKind::Feature,
        "FCStd",
        "holds these records of the retained Document.xml as the source stores them, without \
         their edits",
        dropped,
    )
    .into_iter()
    .collect::<Vec<_>>();
    losses.extend(evaluation_stale(
        "FCStd",
        "writes input edits or empty payload entry names of these objects beside their stored \
         shapes, and their schema-2 declarations hold no recompute mark",
        written.stale,
    ));
    let notes = vec![
        format!(
            "semantic FCStd archive written for {} (SchemaVersion={} FileVersion={})",
            resolution.target(),
            resolution.schema_version(),
            resolution.document().file_version.as_str()
        ),
        "unsupported retained entries and unedited XML records were preserved".into(),
    ];
    Ok(WriteOutcome {
        census: cadmpeg_ir::report::export::EntityCensus {
            basis: cadmpeg_ir::report::export::CensusBasis::IrArenas,
            counts: resolution.ir().census(),
        },
        losses,
        omitted,
        notes,
    })
}

struct WriteOutcome {
    census: cadmpeg_ir::report::export::EntityCensus,
    losses: Vec<LossNote>,
    /// The retained records the archive leaves out, declared dropped.
    omitted: Omissions,
    notes: Vec<String>,
}

/// The retained entries in the source archive's order: each at the lowest
/// offset of a local header the physical ledger states for its name, then
/// those the ledger places nowhere, in arena order.
fn entry_order<'e>(
    entries: &'e [EntryRecord],
    physical_ledger: &[ArchiveSpan],
) -> Vec<&'e EntryRecord> {
    let mut offsets = HashMap::new();
    for span in physical_ledger {
        if let ArchiveSpanRole::LocalSignature(name) = &span.role {
            let start = span.span.start();
            offsets
                .entry(name.as_str())
                .and_modify(|offset: &mut u64| *offset = (*offset).min(start))
                .or_insert(start);
        }
    }
    let mut ordered = entries.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|entry| {
        let offset = offsets.get(entry.name()).copied();
        (offset.is_none(), offset)
    });
    ordered
}

/// The `Document.xml` the archive holds, and the edits of the document it
/// leaves out.
struct WrittenDocument {
    xml: String,
    /// Records whose edit the output holds as the source stores them.
    dropped: BTreeSet<String>,
    /// Schema-2 objects the output changes without a recompute mark: input
    /// edits, or properties written with an empty file name.
    stale: BTreeSet<String>,
    /// Properties written with an empty file name in place of an archive
    /// entry the output does not hold.
    unheld: BTreeSet<String>,
}

impl WrittenDocument {
    /// The written `xml`, which reads back as `graph`, with every record of
    /// the document that `graph` does not state named as dropped.
    fn read_back_as(
        xml: String,
        graph: &Graph,
        expected: (&[ObjectRecord], &[ExtensionRecord], &[PropertyRecord]),
        held: &HashSet<&str>,
        stale: BTreeSet<String>,
        unheld: BTreeSet<String>,
    ) -> Self {
        let (objects, extensions, properties) = expected;
        let none = BTreeSet::new();
        let mut dropped = differing_properties(properties, graph, &none, &none, held);
        dropped.extend(declaration_edits(objects, extensions, graph));
        dropped.extend(absent_properties(properties, graph));
        Self {
            xml,
            dropped,
            stale,
            unheld,
        }
    }
}

/// Patch `Document.xml` with every property edit it can hold (§3.4, Paths
/// item 4).
///
/// A property whose edit has no serializer here, or whose written form does
/// not read back as its record states it, stays as the source stores it, and
/// the patch is made again without it. Where the patched document does not
/// read back, the archive holds the stored `Document.xml`
/// ([`stored_document`]). Object and extension declarations are never
/// patched; an edit of one is reported.
/// An input edit marks its object `Touched`, except in schema 2, which has no
/// recompute mark: there the object is reported stale. A property that names
/// an archive entry the output does not hold is written with an empty file
/// name, and its object is marked the same way. The edits reported dropped are
/// the records of the document the written file does not state.
fn written_document(
    resolution: &Resolution<'_>,
    objects: &[ObjectRecord],
    extensions: &[ExtensionRecord],
    properties: &[PropertyRecord],
) -> Result<WrittenDocument, CodecError> {
    let source = resolution.document_xml();
    let held = std::iter::once("Document.xml")
        .chain(resolution.entries().iter().map(EntryRecord::name))
        .collect::<HashSet<_>>();
    let removed = properties
        .iter()
        .flat_map(PropertyRecord::side_entries)
        .filter(|name| !held.contains(name.as_str()))
        .cloned()
        .collect::<HashSet<_>>();
    let (properties, unheld) = without_entries(properties, &removed);
    let properties = properties.as_slice();
    let mut kept = BTreeSet::new();
    for pass in 0..2 {
        let patch = patch_document(source, properties, &kept)?;
        kept.extend(patch.unwritten);
        let emptied = emptied_owners(properties, &unheld, &kept);
        let (marked, stale) = marked_objects(resolution, objects, &patch.changed_inputs, &emptied);
        let Some(xml) = with_touches(resolution, patch.xml, &marked)? else {
            break;
        };
        let Some(graph) = read_back(&xml, resolution, true)? else {
            break;
        };
        let differing = differing_properties(properties, &graph, &kept, &patch.edited, &held);
        if differing.is_empty() {
            let unheld = unheld.difference(&kept).cloned().collect();
            return Ok(WrittenDocument::read_back_as(
                xml,
                &graph,
                (&marked, extensions, properties),
                &held,
                stale,
                unheld,
            ));
        }
        debug_assert!(
            pass == 0,
            "a property left as stored changes how no other property reads back"
        );
        kept.extend(differing);
    }
    stored_document(
        resolution,
        (objects, extensions, properties),
        &held,
        &removed,
    )
}

/// `Document.xml` as the source stores it, except that each property naming
/// a `removed` archive entry is written with an empty file name and its
/// object is marked as on the patched rung. Every edit the
/// document holds of a record the source stores otherwise is reported
/// dropped. Where the stored `Document.xml` does not read back, the archive
/// holds it unchanged and the report names the document.
fn stored_document(
    resolution: &Resolution<'_>,
    expected: (&[ObjectRecord], &[ExtensionRecord], &[PropertyRecord]),
    held: &HashSet<&str>,
    removed: &HashSet<String>,
) -> Result<WrittenDocument, CodecError> {
    let (objects, extensions, properties) = expected;
    let source = resolution.document_xml();
    if let Some(stored) = read_back(source, resolution, false)? {
        let (stored_properties, unheld) = without_entries(&stored.properties, removed);
        let kept = stored_properties
            .iter()
            .filter(|property| !unheld.contains(&property.id))
            .map(|property| property.id.clone())
            .collect();
        let patch = patch_document(source, &stored_properties, &kept)?;
        let emptied = emptied_owners(&stored_properties, &unheld, &patch.unwritten);
        let (stored_marked, _) =
            marked_objects(resolution, &stored.objects, &HashSet::new(), &emptied);
        if let Some(xml) = with_touches(resolution, patch.xml, &stored_marked)? {
            if let Some(graph) = read_back(&xml, resolution, true)? {
                let (marked, stale) =
                    marked_objects(resolution, objects, &HashSet::new(), &emptied);
                let unheld = unheld.difference(&patch.unwritten).cloned().collect();
                return Ok(WrittenDocument::read_back_as(
                    xml,
                    &graph,
                    (&marked, extensions, properties),
                    held,
                    stale,
                    unheld,
                ));
            }
        }
    }
    Ok(WrittenDocument {
        xml: source.to_owned(),
        dropped: BTreeSet::from([resolution.document().id.clone()]),
        stale: BTreeSet::new(),
        unheld: BTreeSet::new(),
    })
}

/// The owners of the properties in `unheld` outside `kept`: those written
/// with an empty file name.
fn emptied_owners<'p>(
    properties: &'p [PropertyRecord],
    unheld: &BTreeSet<String>,
    kept: &BTreeSet<String>,
) -> HashSet<&'p str> {
    properties
        .iter()
        .filter(|property| unheld.contains(&property.id) && !kept.contains(&property.id))
        .map(|property| property.owner.as_str())
        .collect()
}

/// `objects` with `Touched` set on each object an input edit or an emptied
/// file name changes, or, in schema 2, which has no recompute mark, the
/// objects reported stale.
fn marked_objects(
    resolution: &Resolution<'_>,
    objects: &[ObjectRecord],
    changed_inputs: &HashSet<String>,
    emptied: &HashSet<&str>,
) -> (Vec<ObjectRecord>, BTreeSet<String>) {
    let schema_two = resolution.vocabulary() == Vocabulary::Features;
    let mut marked = objects.to_vec();
    let mut stale = BTreeSet::new();
    for object in &mut marked {
        if !changed_inputs.contains(object.id()) && !emptied.contains(object.id().as_str()) {
            continue;
        }
        if schema_two {
            stale.insert(object.id().clone());
        } else {
            object
                .attributes
                .insert("Touched".to_owned(), "1".to_owned());
        }
    }
    (marked, stale)
}

/// `xml` with the `Touched` marks of `marked` written, where the schema has
/// the mark.
fn with_touches(
    resolution: &Resolution<'_>,
    xml: String,
    marked: &[ObjectRecord],
) -> Result<Option<String>, CodecError> {
    if resolution.vocabulary() == Vocabulary::Features {
        Ok(Some(xml))
    } else {
        patch_object_touches(&xml, marked)
    }
}

/// `properties` with each value attribute that names a `removed` entry made
/// empty, and the ids of the properties that changed.
fn without_entries(
    properties: &[PropertyRecord],
    removed: &HashSet<String>,
) -> (Vec<PropertyRecord>, BTreeSet<String>) {
    let mut unheld = BTreeSet::new();
    let properties = properties
        .iter()
        .cloned()
        .map(|mut property| {
            let type_name = property.type_name.clone();
            if let PropertyBody::Persisted {
                values,
                side_entries,
                ..
            } = &mut property.body
            {
                if side_entries.iter().any(|name| removed.contains(name)) {
                    for value in values {
                        for (key, name) in &mut value.attributes {
                            if crate::persistence::names_side_entry(&type_name, key)
                                && removed.contains(name)
                            {
                                name.clear();
                            }
                        }
                    }
                    side_entries.retain(|name| !removed.contains(name));
                    unheld.insert(property.id.clone());
                }
            }
            property
        })
        .collect();
    (properties, unheld)
}

/// Parse a written `Document.xml` as the decoder does, or `None` where it does
/// not read. With `resolve`, each property's archive entries are resolved
/// against the entries the output holds.
fn read_back(
    xml: &str,
    resolution: &Resolution<'_>,
    resolve: bool,
) -> Result<Option<Graph>, CodecError> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &policy)?;
    let graph = crate::container::admit_document(&ctx, xml.as_bytes()).and_then(|admitted| {
        let mut graph = crate::persistence::parse_document(&admitted, resolution.vocabulary())?;
        if !resolve {
            return Ok(graph);
        }
        let mut views = BTreeMap::new();
        views.insert(
            "Document.xml".to_owned(),
            cadmpeg_core::decode::View::over_retained(resolution.document_xml().as_bytes()),
        );
        for entry in resolution.entries() {
            views.insert(
                entry.name().to_owned(),
                cadmpeg_core::decode::View::over_retained(entry.data()),
            );
        }
        crate::persistence::resolve_side_entries(
            &ctx,
            &mut graph.properties,
            &views,
            &mut graph.losses,
        )?;
        Ok(graph)
    });
    match graph {
        Ok(graph) => Ok(Some(graph)),
        Err(error) => recover(error).map(|()| None),
    }
}

/// The properties outside `kept` whose record the written graph does not
/// state: a repeated id, a record the graph lacks or reads otherwise, or an
/// edited value that names an archive entry the output does not hold.
fn differing_properties(
    expected: &[PropertyRecord],
    written: &Graph,
    kept: &BTreeSet<String>,
    edited: &BTreeSet<String>,
    held: &HashSet<&str>,
) -> BTreeSet<String> {
    let mut differing = BTreeSet::new();
    let mut expected_by_id = HashMap::with_capacity(expected.len());
    for property in expected
        .iter()
        .filter(|property| !kept.contains(&property.id))
    {
        if expected_by_id
            .insert(property.id.as_str(), property)
            .is_some()
        {
            differing.insert(property.id.clone());
        }
    }
    let mut written_by_id = HashMap::with_capacity(written.properties.len());
    for property in &written.properties {
        if written_by_id
            .insert(property.id.as_str(), property)
            .is_some()
            && expected_by_id.contains_key(property.id.as_str())
        {
            differing.insert(property.id.clone());
        }
    }
    for (id, property) in expected_by_id {
        let holds = written_by_id.get(id).is_some_and(|candidate| {
            same_property_semantics(property, candidate)
                && (!edited.contains(id)
                    || candidate
                        .side_entries()
                        .iter()
                        .all(|entry| held.contains(entry.as_str())))
        });
        if !holds {
            differing.insert(property.id.clone());
        }
    }
    differing
}

/// The properties the written graph states that the document holds no record
/// of: a removal the patch does not write.
fn absent_properties(expected: &[PropertyRecord], written: &Graph) -> Vec<String> {
    let expected = expected
        .iter()
        .map(|property| property.id.as_str())
        .collect::<HashSet<_>>();
    written
        .properties
        .iter()
        .filter(|property| !expected.contains(property.id.as_str()))
        .map(|property| property.id.clone())
        .collect()
}

/// The object and extension records whose declaration the written graph does
/// not state as the document does, including declarations the document no
/// longer holds.
fn declaration_edits(
    expected_objects: &[ObjectRecord],
    expected_extensions: &[ExtensionRecord],
    written: &Graph,
) -> BTreeSet<String> {
    let mut edits = BTreeSet::new();
    let objects = written
        .objects
        .iter()
        .map(|object| (object.id(), object))
        .collect::<HashMap<_, _>>();
    for expected in expected_objects {
        let holds = objects.get(expected.id()).is_some_and(|written| {
            expected.name() == written.name()
                && expected.type_name == written.type_name
                && expected.persistent_id == written.persistent_id
                && expected.view_type == written.view_type
                && expected.attributes == written.attributes
                && expected.dependencies == written.dependencies
                && expected.dependency_allow_partial == written.dependency_allow_partial
                && expected.order == written.order
        });
        if !holds {
            edits.insert(expected.id().clone());
        }
    }
    let expected_ids = expected_objects
        .iter()
        .map(ObjectRecord::id)
        .collect::<HashSet<_>>();
    edits.extend(
        written
            .objects
            .iter()
            .filter(|object| !expected_ids.contains(object.id()))
            .map(|object| object.id().clone()),
    );
    let extensions = written
        .extensions
        .iter()
        .map(|extension| (extension.id.as_str(), extension))
        .collect::<HashMap<_, _>>();
    for expected in expected_extensions {
        if extensions.get(expected.id.as_str()) != Some(&expected) {
            edits.insert(expected.id.clone());
        }
    }
    let expected_ids = expected_extensions
        .iter()
        .map(|extension| extension.id.as_str())
        .collect::<HashSet<_>>();
    edits.extend(
        written
            .extensions
            .iter()
            .filter(|extension| !expected_ids.contains(extension.id.as_str()))
            .map(|extension| extension.id.clone()),
    );
    edits
}

fn same_property_semantics(left: &PropertyRecord, right: &PropertyRecord) -> bool {
    if left.owner != right.owner
        || left.name != right.name
        || left.type_name != right.type_name
        || left.family != right.family
        || left.status != right.status
        || left.order != right.order
        || left.is_transient() != right.is_transient()
    {
        return false;
    }
    match (&left.body, &right.body) {
        (PropertyBody::Transient, PropertyBody::Transient) => true,
        (PropertyBody::Unreadable(left), PropertyBody::Unreadable(right)) => left == right,
        (
            PropertyBody::Persisted {
                values: left_values,
                links: left_links,
                side_entries: left_side_entries,
                dynamic: left_dynamic,
            },
            PropertyBody::Persisted {
                values: right_values,
                links: right_links,
                side_entries: right_side_entries,
                dynamic: right_dynamic,
            },
        ) => {
            left_values.len() == right_values.len()
                && left_values.iter().zip(right_values).all(|(left, right)| {
                    left.tag == right.tag
                        && left.order == right.order
                        && left.attributes == right.attributes
                        && left.text == right.text
                })
                && left_links == right_links
                && left_side_entries == right_side_entries
                && left_dynamic == right_dynamic
        }
        _ => false,
    }
}

fn patch_object_touches(
    source: &str,
    objects: &[ObjectRecord],
) -> Result<Option<String>, CodecError> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)?;
    let xml = match ctx.parse_xml(source, "FreeCAD object declarations") {
        Ok(xml) => xml,
        Err(error) => return recover(error).map(|()| None),
    };
    let by_name = objects
        .iter()
        .map(|object| (object.name().as_str(), object))
        .collect::<HashMap<_, _>>();
    let mut edits = Vec::new();
    for section in xml
        .root_element()
        .children()
        .filter(|node| node.has_tag_name("Objects") || node.has_tag_name("Features"))
    {
        for node in section.children().filter(roxmltree::Node::is_element) {
            let Some(object) = node.attribute("name").and_then(|name| by_name.get(name)) else {
                continue;
            };
            if object.attributes.get("Touched").map(String::as_str) != Some("1")
                || node.attribute("Touched") == Some("1")
            {
                continue;
            }
            if let Some(attribute) = node.attribute_node("Touched") {
                let range = attribute.range_value();
                edits.push((range.start, range.end, "1"));
            } else {
                let insert = node.range().start + 1 + node.tag_name().name().len();
                edits.push((insert, insert, " Touched=\"1\""));
            }
        }
    }
    let mut output = source.to_owned();
    for (start, end, replacement) in edits.into_iter().rev() {
        output.replace_range(start..end, replacement);
    }
    Ok(Some(output))
}

/// `Document.xml` with each property outside `kept` written over its
/// retained span.
struct PatchedProperties {
    xml: String,
    /// Objects an edited property is an input of.
    changed_inputs: HashSet<String>,
    /// Properties whose written form differs from the stored one.
    edited: BTreeSet<String>,
    /// Properties left as stored: the span the record names does not hold its
    /// retained text, or the edit has no serializer here.
    unwritten: BTreeSet<String>,
}

fn patch_document(
    source: &str,
    properties: &[PropertyRecord],
    kept: &BTreeSet<String>,
) -> Result<PatchedProperties, CodecError> {
    let mut ordered = properties
        .iter()
        .filter(|property| !kept.contains(&property.id))
        .collect::<Vec<_>>();
    ordered.sort_by_key(|property| property.xml.start());
    let mut patched = PatchedProperties {
        xml: String::with_capacity(source.len()),
        changed_inputs: HashSet::new(),
        edited: BTreeSet::new(),
        unwritten: BTreeSet::new(),
    };
    let mut cursor = 0usize;
    for property in ordered {
        let Some((start, end)) = retained_span(source, property, cursor) else {
            patched.unwritten.insert(property.id.clone());
            continue;
        };
        let serialized = match serialize_property(property) {
            Ok(serialized) => serialized,
            Err(error) => {
                recover(error)?;
                patched.unwritten.insert(property.id.clone());
                continue;
            }
        };
        if serialized != property.xml.text() {
            patched.edited.insert(property.id.clone());
            if crate::mutation::requires_recompute(property) {
                patched.changed_inputs.insert(property.owner.clone());
            }
        }
        patched.xml.push_str(&source[cursor..start]);
        patched.xml.push_str(&serialized);
        cursor = end;
    }
    patched.xml.push_str(&source[cursor..]);
    Ok(patched)
}

/// The byte range of `property`'s retained text in `source`, where it starts
/// at or after `cursor` and `source` holds that text there.
fn retained_span(source: &str, property: &PropertyRecord, cursor: usize) -> Option<(usize, usize)> {
    let start = usize::try_from(property.xml.start()).ok()?;
    let end = usize::try_from(property.xml.end()).ok()?;
    (start >= cursor && source.get(start..end) == Some(property.xml.text())).then_some((start, end))
}

fn serialize_property(property: &PropertyRecord) -> Result<String, CodecError> {
    validate_property_wrapper(property)?;
    if matches!(property.body, PropertyBody::Unreadable(_)) {
        return Ok(property.xml.text().to_owned());
    }
    let mut replacement = property.xml.text().to_owned();
    let wrapped = format!("<Root>{}</Root>", property.xml.text());
    let parsed = roxmltree::Document::parse(&wrapped).map_err(|error| {
        CodecError::malformed(format_args!("invalid retained property XML: {error}"))
    })?;
    let source_ranges = parsed
        .root_element()
        .first_element_child()
        .into_iter()
        .flat_map(|property| {
            property
                .descendants()
                .filter(move |node| node.is_element() && *node != property)
        })
        .map(|node| (node.range().start - 6, node.range().end - 6))
        .collect::<Vec<_>>();
    if source_ranges.len() != property.values().len() {
        return Err(CodecError::malformed(format_args!(
            "property {} value provenance count changed",
            property.id
        )));
    }
    let mut edits = Vec::new();
    for (value, (start, end)) in property.values().iter().zip(source_ranges) {
        let serialized = serialize_value(value)?;
        if serialized == value.raw_xml {
            continue;
        }
        if property.xml.text()[start..end] != value.raw_xml {
            return Err(CodecError::malformed(format_args!(
                "property {} retained value {} disagrees with provenance",
                property.id, value.order
            )));
        }
        edits.push((start, end, serialized));
    }
    edits.sort_by_key(|(start, _, _)| *start);
    if edits.windows(2).any(|pair| pair[0].1 > pair[1].0) {
        return Err(CodecError::NotImplemented(format!(
            "overlapping edits in nested FCStd property {}",
            property.id
        )));
    }
    for (start, end, serialized) in edits.into_iter().rev() {
        replacement.replace_range(start..end, &serialized);
    }
    Ok(replacement)
}

fn validate_property_wrapper(property: &PropertyRecord) -> Result<(), CodecError> {
    let wrapped = format!("<Root>{}</Root>", property.xml.text());
    let parsed = roxmltree::Document::parse(&wrapped).map_err(|error| {
        CodecError::malformed(format_args!("invalid retained property XML: {error}"))
    })?;
    let element = parsed
        .root_element()
        .first_element_child()
        .ok_or_else(|| CodecError::Malformed("retained property has no element".into()))?;
    let expected_tag = if property.is_transient() {
        "_Property"
    } else {
        "Property"
    };
    let status = element
        .attribute("status")
        .map(str::parse::<u64>)
        .transpose()
        .map_err(|_| CodecError::Malformed("retained property has invalid status".into()))?;
    if element.tag_name().name() != expected_tag
        || element.attribute("name") != Some(property.name.as_str())
        || element.attribute("type") != Some(property.type_name.as_str())
        || status != property.status
    {
        return Err(CodecError::NotImplemented(format!(
            "editing FCStd property declaration {} requires a typed serializer",
            property.id
        )));
    }
    Ok(())
}

fn serialize_value(value: &ValueRecord) -> Result<String, CodecError> {
    let wrapped = format!("<Root>{}</Root>", value.raw_xml);
    let parsed = roxmltree::Document::parse(&wrapped).map_err(|error| {
        CodecError::malformed(format_args!("invalid retained property value XML: {error}"))
    })?;
    let original = parsed
        .root_element()
        .first_element_child()
        .ok_or_else(|| CodecError::Malformed("retained property value has no element".into()))?;
    let original_attributes = original
        .attributes()
        .map(|attribute| (attribute.name().to_owned(), attribute.value().to_owned()))
        .collect::<std::collections::BTreeMap<_, _>>();
    let original_text = original
        .children()
        .find_map(|node| node.text())
        .map(str::to_owned);
    if original.tag_name().name() == value.tag
        && original_attributes == value.attributes
        && original_text == value.text
    {
        return Ok(value.raw_xml.clone());
    }
    if original.children().any(|node| node.is_element()) {
        return Err(CodecError::NotImplemented(format!(
            "editing nested FCStd value element {} requires a typed serializer",
            value.tag
        )));
    }
    let mut serialized = String::new();
    serialized.push('<');
    serialized.push_str(&value.tag);
    for (name, content) in &value.attributes {
        serialized.push(' ');
        serialized.push_str(name);
        serialized.push_str("=\"");
        escape_xml(content, &mut serialized, true);
        serialized.push('"');
    }
    match &value.text {
        Some(text) => {
            serialized.push('>');
            escape_xml(text, &mut serialized, false);
            serialized.push_str("</");
            serialized.push_str(&value.tag);
            serialized.push('>');
        }
        None => serialized.push_str("/>"),
    }
    Ok(serialized)
}

pub(crate) fn escape_xml(value: &str, output: &mut String, attribute: bool) {
    for character in value.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' if attribute => output.push_str("&quot;"),
            '\'' if attribute => output.push_str("&apos;"),
            '\t' => output.push_str("&#9;"),
            '\n' => output.push_str("&#10;"),
            '\r' => output.push_str("&#13;"),
            other => output.push(other),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
