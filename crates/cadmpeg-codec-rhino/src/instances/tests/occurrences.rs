// SPDX-License-Identifier: Apache-2.0
use super::support::{install_definitions, static_definition};
use crate::chunks::ArchiveVersion;
use crate::test_support::test_dump::{
    instance_reference_payload, object_record_with_payload, point_payload, scan_with_objects,
    set_identity, transform, INSTANCE_REFERENCE_CLASS, POINT_CLASS,
};
use crate::wire::Uuid;
use cadmpeg_ir::products::{OccurrenceParent, PrototypeReference};

#[test]
fn nested_references_form_child_occurrences_of_each_placement() {
    let archive = ArchiveVersion::V5;
    let point_id = [0x51; 16];
    let nested_id = [0x52; 16];
    let inner = [0x61; 16];
    let outer = [0x62; 16];
    let point =
        object_record_with_payload(archive, 1, POINT_CLASS, &point_payload([1.0, 2.0, 3.0]));
    let nested = object_record_with_payload(
        archive,
        0x1000,
        INSTANCE_REFERENCE_CLASS,
        &instance_reference_payload(inner, transform(1.0, [0.0, 5.0, 0.0])),
    );
    let first = object_record_with_payload(
        archive,
        0x1000,
        INSTANCE_REFERENCE_CLASS,
        &instance_reference_payload(outer, transform(1.0, [10.0, 0.0, 0.0])),
    );
    let second = object_record_with_payload(
        archive,
        0x1000,
        INSTANCE_REFERENCE_CLASS,
        &instance_reference_payload(outer, transform(1.0, [20.0, 0.0, 0.0])),
    );
    let mut scan = scan_with_objects(&[point, nested, first, second]);
    set_identity(&mut scan, 0, point_id, "point", None, true);
    set_identity(&mut scan, 1, nested_id, "nested", None, true);
    set_identity(&mut scan, 2, [0x71; 16], "first", None, true);
    set_identity(&mut scan, 3, [0x72; 16], "second", None, true);
    install_definitions(
        &mut scan,
        vec![
            static_definition(inner, &[point_id]),
            static_definition(outer, &[nested_id]),
        ],
    );

    let result = crate::decode::decode_for_test(&scan);
    let model = &result.ir().model;
    assert_eq!(model.product_definitions.len(), 2);
    assert_eq!(model.occurrences.len(), 4);
    let roots = model
        .occurrences
        .iter()
        .filter(|occurrence| matches!(occurrence.parent, OccurrenceParent::Root {}))
        .collect::<Vec<_>>();
    assert_eq!(roots.len(), 2);
    assert_eq!(
        roots
            .iter()
            .map(|occurrence| occurrence.ordinal)
            .collect::<Vec<_>>(),
        [0, 1]
    );
    for root in &roots {
        let children = model
            .occurrences
            .iter()
            .filter(|occurrence| {
                matches!(&occurrence.parent, OccurrenceParent::Occurrence { occurrence: parent } if *parent == root.id)
            })
            .collect::<Vec<_>>();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].transform.affine_rows()[1][3], 5.0);
        assert_eq!(
            children[0].native_ref.as_deref(),
            Some("rhino:object:record#000001")
        );
        let PrototypeReference::Local { definition } = &children[0].prototype else {
            panic!("nested prototype is local");
        };
        assert_eq!(
            definition.as_str(),
            format!("rhino:instance:definition#{}", Uuid::from_wire(inner))
        );
    }
    assert_eq!(roots[0].transform.affine_rows()[0][3], 10.0);
    assert_eq!(roots[1].transform.affine_rows()[0][3], 20.0);
}

#[test]
fn missing_definition_keeps_an_unresolved_occurrence() {
    let archive = ArchiveVersion::V5;
    let reference = object_record_with_payload(
        archive,
        0x1000,
        INSTANCE_REFERENCE_CLASS,
        &instance_reference_payload([0x61; 16], transform(1.0, [3.0, 0.0, 0.0])),
    );
    let mut scan = scan_with_objects(&[reference]);
    set_identity(&mut scan, 0, [0x71; 16], "reference", None, true);

    let result = crate::decode::decode_for_test(&scan);
    let model = &result.ir().model;
    assert!(model.bodies.is_empty());
    assert_eq!(model.occurrences.len(), 1);
    assert_eq!(
        model.occurrences[0].prototype,
        PrototypeReference::Unresolved {}
    );
    assert_eq!(model.occurrences[0].transform.affine_rows()[0][3], 3.0);
    assert!(result.report().losses.iter().all(|loss| {
        loss.code != crate::loss::RhinoLossCode::ObjectFamilyNotTransferred.kind()
    }));
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == crate::loss::RhinoLossCode::ObjectDecodeDiagnostic.kind()
            && loss.message.contains("is missing")
    }));
}

#[test]
fn a_definition_cycle_stops_the_occurrence_tree() {
    let archive = ArchiveVersion::V5;
    let nested_id = [0x52; 16];
    let definition = [0x61; 16];
    let nested = object_record_with_payload(
        archive,
        0x1000,
        INSTANCE_REFERENCE_CLASS,
        &instance_reference_payload(definition, transform(1.0, [1.0, 0.0, 0.0])),
    );
    let root = object_record_with_payload(
        archive,
        0x1000,
        INSTANCE_REFERENCE_CLASS,
        &instance_reference_payload(definition, transform(1.0, [2.0, 0.0, 0.0])),
    );
    let mut scan = scan_with_objects(&[nested, root]);
    set_identity(&mut scan, 0, nested_id, "nested", None, true);
    set_identity(&mut scan, 1, [0x71; 16], "root", None, true);
    install_definitions(&mut scan, vec![static_definition(definition, &[nested_id])]);

    let result = crate::decode::decode_for_test(&scan);
    assert_eq!(result.ir().model.occurrences.len(), 2);
}
