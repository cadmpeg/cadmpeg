// SPDX-License-Identifier: Apache-2.0
//! End-to-end NX assembly occurrence transfer against its STEP twin.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::products::{
    ExternalDocument, OccurrenceParent, ProductDefinitionKind, PrototypeReference,
};

use crate::loss::NxLossCode;
use crate::NxCodec;

const STEP_MTC_TYPE_COUNTS: [(&str, usize); 8] = [
    ("90591A141 HEX NUT", 10),
    ("91274A118 SHCS", 14),
    ("91274A141 SHCS", 4),
    ("91304A112 FHSCS", 4),
    ("NIST MTC CRADA POLYCARB", 1),
    ("NIST MTC CRADA COVER", 1),
    ("NIST MTC CRADA PLATE", 1),
    ("NIST MTC CRADA BOX", 1),
];

const STEP_FHSCS_INSTANCE_DISTANCES_MM: [f64; 6] = [
    60.16,
    60.16,
    83.459_999_9,
    83.459_999_9,
    102.882_443_513,
    102.882_443_513,
];
const EPS_STEP_INSTANCE_DISTANCE_MM: f64 = 0.003;
const EPS_NX_PLACEMENT_ROTATION: f64 = 1.0e-4;
const COUNT_ASSUMPTION_UUID: &str = "01234567-89ab-cdef-0123-456789abcdef";

fn append_fastload_string(bytes: &mut Vec<u8>, tag: u8, value: &str) {
    bytes.push(tag);
    bytes.push(u8::try_from(value.len() + 2).unwrap());
    bytes.extend(value.as_bytes());
    bytes.push(0);
}

fn fastload_structure_with_two_same_prototype_uses() -> Vec<u8> {
    let mut bytes = vec![1, 2, 0x42, 0];
    bytes.extend([1, 2]);
    append_fastload_string(&mut bytes, 4, "MODEL");
    bytes.extend([1, 3, 0, 0, 1, 3, b'9', b'9']);
    bytes.extend([1, 2, 0xff, 0xff, 0xff, 0xff, 1, 2]);
    append_fastload_string(&mut bytes, 4, "pin");
    bytes.extend([1, 3, 1, 1]);
    bytes.extend([1, 2, 3, 38]);
    bytes.extend(COUNT_ASSUMPTION_UUID.as_bytes());
    bytes.push(0);
    bytes.extend([1, 3, 1, 1]);

    let mut structure = vec![0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0];
    structure.extend(u32::try_from(bytes.len()).unwrap().to_be_bytes());
    structure.extend(bytes);
    structure
}

fn placement_with_uuid(uuid: &str) -> Vec<u8> {
    let mut frame = crate::layout::assembly_placement_prefix::MAGIC_VALUE.to_vec();
    frame.extend([1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0]);
    frame.extend([0, 0, 0, 1, 1, 0xf0, 0, 0, 0]);
    append_fastload_string(&mut frame, 3, "instance");
    append_fastload_string(&mut frame, 3, uuid);
    frame
}

#[test]
fn decode_uses_one_prototype_across_unequal_counts_with_a_loss() {
    let file = crate::test_support::test_prt::prt_with_named_payloads(&[
        (
            "/Root/UG_PART/UG_PART",
            placement_with_uuid(COUNT_ASSUMPTION_UUID),
        ),
        (
            "/Root/FastLoad/Structure",
            fastload_structure_with_two_same_prototype_uses(),
        ),
    ]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();
    let placement = result
        .ir()
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.native_ref.is_some())
        .expect("placement is emitted");
    assert_eq!(placement.name.as_deref(), Some("pin"));
    assert!(matches!(
        &placement.prototype,
        PrototypeReference::External {
            document: ExternalDocument::Missing {},
            object: None,
        }
    ));
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == NxLossCode::AssemblyPrototypeCountAssumed.kind()));
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == NxLossCode::AssemblyUuidPrototypeAssumed.kind()));
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == NxLossCode::AssemblyExternalDocumentUnresolved.kind()));
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == NxLossCode::AssemblyOccurrencesUnresolved.kind()));
}

#[test]
fn decode_emits_mtc_external_occurrences_with_step_twin_counts_and_spacing() {
    let bytes = include_bytes!("../../../../corpus/nist-pmi/nx/NIST mtc crada assembly.prt");
    let result = NxCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let model = &result.ir().model;
    let roots = model
        .occurrences
        .iter()
        .filter(|occurrence| matches!(occurrence.parent, OccurrenceParent::Root {}))
        .collect::<Vec<_>>();
    assert_eq!(roots.len(), 1);
    let root = roots[0];
    let children = model
        .occurrences
        .iter()
        .filter(|occurrence| {
            matches!(
                &occurrence.parent,
                OccurrenceParent::Occurrence { occurrence: parent } if parent == &root.id
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(children.len(), 36);
    assert_eq!(model.occurrences.len(), 37);
    assert_eq!(model.product_definitions.len(), 1);
    assert_eq!(
        model.product_definitions[0].kind,
        ProductDefinitionKind::Group
    );

    let mut type_counts = BTreeMap::new();
    let mut placement_refs = BTreeSet::new();
    let mut external_filenames = BTreeSet::new();
    let mut fhs_positions = Vec::new();
    for occurrence in &children {
        let PrototypeReference::External { document, object } = &occurrence.prototype else {
            panic!("MTC component references are external");
        };
        assert!(object.is_none());
        let name = occurrence
            .name
            .as_deref()
            .expect("FastLoad prototype name is retained as the occurrence name");
        *type_counts.entry(name).or_insert(0) += 1;
        let ExternalDocument::Path { path } = document else {
            panic!("MTC external filename uniquely matches each FastLoad prototype");
        };
        assert!(!path.as_str().contains('/'));
        assert!(!path.as_str().contains('\\'));
        assert!(path.as_str().eq_ignore_ascii_case(&format!("{name}.prt")));
        external_filenames.insert(path.as_str());

        let native_ref = occurrence.native_ref.as_deref().unwrap();
        assert!(native_ref.starts_with("nx:assembly:placement#"));
        assert!(placement_refs.insert(native_ref));
        if name == "91304A112 FHSCS" {
            let rows = occurrence.transform.affine_rows();
            assert!(rows[0][2].abs() < EPS_NX_PLACEMENT_ROTATION);
            assert!(rows[1][2].abs() < EPS_NX_PLACEMENT_ROTATION);
            assert!((rows[2][2] - 1.0).abs() < EPS_NX_PLACEMENT_ROTATION);
            fhs_positions.push([rows[0][3], rows[1][3], rows[2][3]]);
        }
    }
    assert_eq!(type_counts, BTreeMap::from(STEP_MTC_TYPE_COUNTS));
    assert_eq!(placement_refs.len(), 36);
    assert_eq!(external_filenames.len(), 8);

    let mut fhs_distances = Vec::new();
    for left in 0..fhs_positions.len() {
        for right in left + 1..fhs_positions.len() {
            let squared_distance = (0..3)
                .map(|axis| (fhs_positions[left][axis] - fhs_positions[right][axis]).powi(2))
                .sum::<f64>();
            fhs_distances.push(squared_distance.sqrt());
        }
    }
    fhs_distances.sort_by(f64::total_cmp);
    assert_eq!(fhs_distances.len(), STEP_FHSCS_INSTANCE_DISTANCES_MM.len());
    for (actual, expected) in fhs_distances.iter().zip(STEP_FHSCS_INSTANCE_DISTANCES_MM) {
        assert!((actual - expected).abs() < EPS_STEP_INSTANCE_DISTANCE_MM);
    }

    let losses = &result.report().losses;
    assert!(losses
        .iter()
        .any(|loss| loss.code == NxLossCode::AssemblyPlacementUnitsAssumed.kind()));
    assert!(losses
        .iter()
        .any(|loss| loss.code == NxLossCode::AssemblyHierarchyAssumed.kind()));
    assert!(losses
        .iter()
        .any(|loss| loss.code == NxLossCode::AssemblyExternalFilenameAssumed.kind()));
    assert!(losses
        .iter()
        .any(|loss| loss.code == NxLossCode::AssemblyUuidPrototypeAssumed.kind()));
    assert!(!losses
        .iter()
        .any(|loss| loss.code == NxLossCode::AssemblyExternalDocumentUnresolved.kind()));
    assert!(!losses
        .iter()
        .any(|loss| loss.code == NxLossCode::AssemblyPrototypeCountAssumed.kind()));
    assert!(!losses
        .iter()
        .any(|loss| loss.code == NxLossCode::AssemblyOccurrencesUnresolved.kind()));
}
