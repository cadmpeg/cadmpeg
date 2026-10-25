// SPDX-License-Identifier: Apache-2.0
//! Assertions for located source failures and their exact neutral omissions.

use cadmpeg_ir::codec::DecodeResult;
use cadmpeg_ir::source_fidelity::parts::PartState;

pub(super) fn assert_native_projection_from_failed_control(
    result: &DecodeResult,
    owner: &str,
    property: &str,
) {
    let namespace = result.ir().native.namespace("fcstd").expect("namespace");
    let objects = namespace
        .arena_as::<crate::native::ObjectRecord>("objects")
        .unwrap();
    let object = objects
        .iter()
        .find(|object| object.name() == owner)
        .expect("readable owner retained");
    let property_id = crate::native::native_child_id("property", object.id(), property);
    let feature = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some(object.id().as_str()))
        .expect("#224 retains the actual source feature as Native");
    assert!(
        matches!(feature.definition().operation(), cadmpeg_ir::features::FeatureOperation::Native { kind, parameters }
        if kind.as_str() == object.type_name && parameters.keys().all(|name| name.as_str() != property)),
        "no unavailable control is invented in Native.parameters"
    );
    let properties = namespace
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    let held_property = properties.iter().find(|value| value.id == property_id);
    let side_entries = held_property.map_or(&[][..], crate::native::PropertyRecord::side_entries);
    let entries = namespace
        .arena_as::<crate::native::EntryRecord>("entries")
        .unwrap();
    let entry_ids = entries
        .iter()
        .filter(|entry| side_entries.iter().any(|name| name == entry.name()))
        .map(crate::native::EntryRecord::id)
        .collect::<Vec<_>>();
    let homes = namespace
        .arena_as::<cadmpeg_ir::unknown::NativeUnknownRecord>("unknowns")
        .unwrap()
        .into_iter()
        .filter(|unknown| {
            unknown
                .links
                .iter()
                .any(|id| id.as_str() == property_id || entry_ids.contains(&id.as_str()))
                || held_property
                    .is_some_and(|property| binary_home(result, &unknown.id, property, &entry_ids))
                || !properties.iter().any(|record| record.id == property_id)
                    && unknown.links.iter().any(|id| id.as_str() == object.id())
                    && result
                        .source_fidelity()
                        .parts()
                        .get(&unknown.id)
                        .is_some_and(|part| part.level().as_str() == "freecad.property")
                    && result
                        .source_fidelity()
                        .retained_record(unknown.id.as_str())
                        .and_then(|record| record.data())
                        .and_then(|bytes| std::str::from_utf8(bytes).ok())
                        .and_then(|xml| roxmltree::Document::parse(xml).ok())
                        .is_some_and(|xml| xml.root_element().attribute("name") == Some(property))
        })
        .filter_map(|unknown| {
            result
                .source_fidelity()
                .parts()
                .get(&unknown.id)
                .filter(|part| part.state() == &PartState::GivenUp {})
        })
        .collect::<Vec<_>>();
    let losses = result
        .report()
        .losses
        .iter()
        .filter(|loss| {
            loss.code.local_code() == "record.unreadable"
                && loss.provenance.as_ref().is_some_and(|provenance| {
                    homes.iter().any(|part| {
                        provenance.stream_name() == Some(part.stream())
                            && provenance.offset == part.offset()
                            && provenance.byte_len() == Some(part.byte_len())
                    })
                })
        })
        .collect::<Vec<_>>();
    assert!(
        !losses.is_empty(),
        "the actual failed property/payload owns its diagnostic: {:?}",
        result.report().losses
    );
    assert!(
        losses
            .iter()
            .all(|loss| !loss.message.contains(feature.id.as_str())),
        "a retained Feature is never named as dropped"
    );
    for loss in &losses {
        let provenance = loss.provenance.as_ref().expect("located source home");
        assert!(
            result
                .source_fidelity()
                .parts()
                .values()
                .any(|part| part.state() == &PartState::GivenUp {}
                    && provenance.stream_name() == Some(part.stream())
                    && provenance.offset == part.offset()
                    && provenance.byte_len() == Some(part.byte_len())),
            "same original given-up home and exact source extent"
        );
    }
    let missing_structure = if super::super::is_sketch(&object.type_name) {
        Some("sketch")
    } else if super::super::is_spreadsheet(&object.type_name) {
        Some("spreadsheet")
    } else {
        None
    };
    if let Some(kind) = missing_structure {
        let id = super::super::design_identity_text(kind, object, format_args!(""));
        assert_eq!(
            losses
                .iter()
                .filter(|loss| loss.message.contains(&id))
                .count(),
            1,
            "one actual home names the independently unformable neutral row"
        );
    }
    crate::test_support::test_archive::assert_valid_document(result.ir());
}

fn binary_home(
    result: &DecodeResult,
    id: &cadmpeg_ir::ids::UnknownId,
    property: &crate::native::PropertyRecord,
    entries: &[&str],
) -> bool {
    let levels = match property.type_name.as_str() {
        "App::PropertyVectorList" => ["freecad.vector_list_entry", "freecad.vector_list_vector"],
        "App::PropertyFloatList" => ["freecad.float_list_entry", "freecad.float_list_value"],
        "Part::PropertyFilletEdges" => ["freecad.fillet_edges_entry", "freecad.fillet_edge"],
        _ => return false,
    };
    let fidelity = result.source_fidelity();
    let Some(mut part) = fidelity.parts().get(id) else {
        return false;
    };
    if !levels.contains(&part.level().as_str()) {
        return false;
    }
    while part.level().as_str() != "zip.entry" {
        let Some(parent) = part
            .parent()
            .and_then(|parent| fidelity.parts().get(parent))
        else {
            return false;
        };
        part = parent;
    }
    // The retained Entry layout states its physical payload extent. Match
    // that genuine parent, without interpreting logical stream identities.
    entries.iter().any(|id| {
        fidelity.record_layout().get(*id).is_some_and(|layout| {
            layout.stream() == part.stream()
                && layout.pointers().get("archive.data_start") == Some(&part.offset())
                && part
                    .offset()
                    .checked_add(part.byte_len())
                    .is_some_and(|end| layout.pointers().get("archive.data_end") == Some(&end))
        })
    })
}

pub(super) fn assert_failed_value(result: &DecodeResult, owner: &str, property: &str) {
    let owner = crate::native::native_id("object", owner);
    let property_id = crate::native::native_child_id("property", &owner, property);
    let namespace = result.ir().native.namespace("fcstd").unwrap();
    let properties = namespace
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    let record = properties
        .iter()
        .find(|record| record.id == property_id)
        .expect("property header retained");
    assert!(record.body.is_some(), "first readable root survives");
    assert!(!record.values().is_empty());
    assert!(
        matches!(&record.body, Some(crate::native::PropertyBody::Persisted { values, .. }) if values.has_missing()),
        "failed extra root remains a native null slot"
    );
    let unknowns = namespace
        .arena_as::<cadmpeg_ir::unknown::NativeUnknownRecord>("unknowns")
        .unwrap();
    assert!(unknowns.iter().any(|unknown| unknown
        .links
        .iter()
        .any(|id| id.as_str() == property_id)
        && result
            .source_fidelity()
            .parts()
            .get(&unknown.id)
            .is_some_and(|part| part.state() == &PartState::GivenUp {}
                && part.level().as_str() == "freecad.property_value")));
}

pub(super) fn assert_one_failed_part(result: &DecodeResult, level: &str) {
    let failures = result
        .source_fidelity()
        .parts()
        .values()
        .filter(|part| part.state() == &PartState::GivenUp {})
        .collect::<Vec<_>>();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].level().as_str(), level);
    let errors = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.severity == cadmpeg_ir::report::Severity::Error)
        .collect::<Vec<_>>();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code.local_code(), "record.unreadable");
    let provenance = errors[0].provenance.as_ref().unwrap();
    assert_eq!(provenance.stream_name(), Some(failures[0].stream()));
    assert_eq!(provenance.offset, failures[0].offset());
    assert_eq!(provenance.byte_len(), Some(failures[0].byte_len()));
}
