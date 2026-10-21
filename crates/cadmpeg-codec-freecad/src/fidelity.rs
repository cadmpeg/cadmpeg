// SPDX-License-Identifier: Apache-2.0
//! Source-only archive and XML images. No native record serializes these bytes.

use cadmpeg_core::CodecError;
use cadmpeg_ir::{
    ids::UnknownId, ArenaClasses, RetainedBytes, RetainedSourceRecord, SourceFidelity,
};

pub(crate) mod validation;

pub(crate) const SOURCE_IMAGE: &str = "fcstd:source:image#archive";

pub(crate) fn retain(
    fidelity: &mut SourceFidelity,
    id: &str,
    stream: &str,
    offset: u64,
    bytes: &[u8],
    untyped: ArenaClasses,
) -> Result<(), CodecError> {
    fidelity
        .insert_retained_record(
            UnknownId::mint(id).map_err(CodecError::malformed)?,
            RetainedSourceRecord::from_bytes(
                stream,
                offset,
                RetainedBytes::Inline {
                    data: bytes.to_vec(),
                },
            )?
            .with_untyped(untyped),
        )
        .map_err(CodecError::malformed)
}

/// Record a subrange without retaining its image twice. The complete logical
/// entry image remains available in fidelity for layout reuse.
pub(crate) fn evidence(
    fidelity: &mut SourceFidelity,
    id: &str,
    stream: &str,
    offset: u64,
    bytes: &[u8],
    untyped: ArenaClasses,
) -> Result<(), CodecError> {
    let evidence = cadmpeg_ir::source_fidelity::SourceEvidence::of(bytes);
    fidelity
        .insert_retained_record(
            UnknownId::mint(id).map_err(CodecError::malformed)?,
            RetainedSourceRecord::from_bytes(
                stream,
                offset,
                RetainedBytes::Digest {
                    byte_len: evidence.length,
                    sha256: evidence.sha256,
                },
            )?
            .with_untyped(untyped),
        )
        .map_err(CodecError::malformed)
}

pub(crate) fn layout<T: serde::Serialize>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    fidelity: &mut SourceFidelity,
    name: &str,
    value: &T,
) -> Result<(), CodecError> {
    let bytes = serde_json::to_vec(value).map_err(CodecError::malformed)?;
    ctx.retain_bytes(
        cadmpeg_core::decode::u64_from_index(bytes.len()),
        "retain FCStd source layout",
    )?;
    retain(
        fidelity,
        &crate::native::native_id("source-layout", name),
        "fcstd:source:layout#archive",
        0,
        &bytes,
        ArenaClasses::new(),
    )
}

pub(crate) fn property(
    fidelity: &mut SourceFidelity,
    property: &crate::native::PropertyRecord,
    objects: &[crate::native::ObjectRecord],
) -> Result<Option<cadmpeg_ir::ArenaClass>, CodecError> {
    let opaque = crate::persistence::opaque_python_payload(property);
    let mut classes = ArenaClasses::new();
    let class = if property.type_name == "Part::PropertyPartShape" {
        cadmpeg_ir::ArenaClass::Model
    } else if property.owner == crate::native::native_id("document", "0")
        || objects.iter().any(|object| {
            object.id().as_str() == property.owner
                && crate::design::is_design_kind(&object.type_name)
        })
    {
        cadmpeg_ir::ArenaClass::Design
    } else {
        cadmpeg_ir::ArenaClass::Model
    };
    if opaque || matches!(property.body, crate::native::PropertyBody::Unreadable(_)) {
        classes.insert(class);
    }
    evidence(
        fidelity,
        &property.id,
        "Document.xml",
        property.xml.start(),
        property.xml.text().as_bytes(),
        classes,
    )?;
    Ok(opaque.then_some(class))
}

/// Charge missing content at the admission that retained only its evidence.
pub(crate) fn mark_unreadable(
    ir: &cadmpeg_ir::CadIr,
    fidelity: &mut SourceFidelity,
) -> Result<(), CodecError> {
    use cadmpeg_ir::ArenaClass;
    let Some(native) = ir.native.namespace("fcstd") else {
        return Ok(());
    };
    let objects = native.arena_as::<crate::native::ObjectRecord>("objects")?;
    let properties = native.arena_as::<crate::native::PropertyRecord>("properties")?;
    let class = |property: &crate::native::PropertyRecord| {
        if property.type_name == "Part::PropertyPartShape" {
            ArenaClass::Model
        } else if property.owner == crate::native::native_id("document", "0")
            || objects.iter().any(|object| {
                object.id().as_str() == property.owner
                    && crate::design::is_design_kind(&object.type_name)
            })
        {
            ArenaClass::Design
        } else {
            ArenaClass::Model
        }
    };
    for property in &properties {
        if matches!(property.body, crate::native::PropertyBody::Unreadable(_)) {
            fidelity
                .mark_untyped(&property.id, [class(property)])
                .map_err(CodecError::malformed)?;
        }
    }
    let entries = native.arena_as::<crate::native::EntryRecord>("entries")?;
    let maps = native.arena_as::<crate::native::element_map::ElementMapRecord>("element_maps")?;
    for property in &properties {
        if matches!(
            property.type_name.as_str(),
            "App::PropertyXLink"
                | "App::PropertyXLinkSub"
                | "App::PropertyXLinkList"
                | "App::PropertyXLinkSubList"
        ) {
            continue;
        }
        let missing = property
            .values()
            .iter()
            .flat_map(|value| &value.attributes)
            .any(|(key, name)| {
                matches!(key.as_str(), "file" | "File")
                    && !name.is_empty()
                    && !entries.iter().any(|entry| entry.name() == name)
            });
        let map_missing = property.values().iter().any(|value| {
            matches!(value.tag.as_str(), "ElementMap" | "ElementMap2")
                && (value
                    .text
                    .as_deref()
                    .is_some_and(|text| !text.trim().is_empty())
                    || value
                        .attributes
                        .get("count")
                        .is_some_and(|count| count != "0"))
        }) && !maps.iter().any(|map| map.property == property.id);
        if missing || map_missing {
            fidelity
                .mark_untyped(&property.id, [class(property)])
                .map_err(CodecError::malformed)?;
        }
    }
    for entry in native.arena_as::<crate::container::UnreadableEntry>("unreadable_entries")? {
        let mut classes = ArenaClasses::new();
        for property in properties.iter().filter(|property| {
            property.side_entries().contains(&entry.name)
                || property.values().iter().any(|value| {
                    value.attributes.iter().any(|(key, name)| {
                        matches!(key.as_str(), "file" | "File") && *name == entry.name
                    })
                })
        }) {
            classes.insert(class(property));
        }
        if entry.name == "GuiDocument.xml"
            || std::path::Path::new(&entry.name)
                .extension()
                .is_some_and(|extension| extension.as_encoded_bytes().eq_ignore_ascii_case(b"brp"))
        {
            classes.insert(ArenaClass::Model);
        }
        if classes.is_empty() {
            classes.extend([ArenaClass::Model, ArenaClass::Design]);
        }
        fidelity
            .mark_untyped(&entry.id, classes)
            .map_err(CodecError::malformed)?;
    }
    if objects.is_empty()
        && native
            .arena_as::<crate::native::DocumentFacts>("document")?
            .iter()
            .any(|document| document.object_count != 0)
    {
        fidelity
            .mark_untyped(SOURCE_IMAGE, [ArenaClass::Model, ArenaClass::Design])
            .map_err(CodecError::malformed)?;
    }
    Ok(())
}
