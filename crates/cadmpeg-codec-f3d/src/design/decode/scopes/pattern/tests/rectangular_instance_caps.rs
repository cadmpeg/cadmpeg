// SPDX-License-Identifier: Apache-2.0
//! Local caps of the rectangular-pattern instance search.

use super::{append_header, append_transform_record};
use crate::design::decode::scopes::pattern::{exact_rectangular_pattern_instances, InstanceSearch};
use crate::records::feature::patterns::RectangularPatternSearchCap;
use crate::records::feature::patterns::MAX_RECTANGULAR_PATTERN_INSTANCES;
use crate::records::feature::scope::DesignParameterScope;

fn rectangular_instance_fixture() -> (
    Vec<u8>,
    crate::design::decode::sketch::IndexedRecordOffsets,
    DesignParameterScope,
    crate::records::feature::patterns::DesignRectangularPatternConstruction,
) {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#0",
        crate::records::feature::scope::DesignFeatureKind::RPattern,
        0,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                100, 50, 51, 52, 53, 110, 120, 130, 140,
            ]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut bytes = Vec::new();
    append_transform_record(&mut bytes, 100, [0., 0., 0.]);
    for index in 50..=53 {
        append_header(&mut bytes, index);
    }
    append_header(&mut bytes, 110);
    append_transform_record(&mut bytes, 120, [0., 0., 5e199]);
    append_transform_record(&mut bytes, 130, [0., 0., 1e200]);
    append_header(&mut bytes, 140);
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let construction =
        crate::records::feature::patterns::DesignRectangularPatternConstruction::try_from(
            crate::records::feature::patterns::DesignRectangularPatternConstructionWire {
                u_count: 3,
                v_count: 1,
                u_extent: 1e200,
                v_extent: 0.,
                owner_record_indices: [50, 51, 52, 53],
                value_offsets: [501, 502, 503, 504],
                instances: None,
                instance_search_cap: None,
                directions: Vec::new(),
                secondary_direction: None,
                distribution: None,
            },
        )
        .unwrap();
    (bytes, records, scope, construction)
}

/// The instance search over `bytes` under the caps that `limit` gives.
fn instance_search(
    bytes: &[u8],
    scope: &DesignParameterScope,
    construction: &crate::records::feature::patterns::DesignRectangularPatternConstruction,
    limit: fn(RectangularPatternSearchCap) -> usize,
) -> InstanceSearch {
    let records = crate::design::test_support::indexed_record_offsets_for_test(bytes);
    exact_rectangular_pattern_instances(bytes, &records, scope, construction, limit)
}

#[test]
fn rectangular_pattern_local_search_ceilings_cap_the_search() {
    // The fixture's records span 139 bytes or fewer.
    const RECORD_BYTES: usize = 256;
    let record_bytes = |cap: RectangularPatternSearchCap| match cap {
        RectangularPatternSearchCap::RecordBytes => RECORD_BYTES,
        cap => RectangularPatternSearchCap::limit(cap),
    };
    let (bytes, _, scope, construction) = rectangular_instance_fixture();
    let mut wire =
        crate::records::feature::patterns::DesignRectangularPatternConstructionWire::from(
            construction,
        );
    wire.u_count = u32::try_from(MAX_RECTANGULAR_PATTERN_INSTANCES + 1).unwrap();
    let construction =
        crate::records::feature::patterns::DesignRectangularPatternConstruction::try_from(wire)
            .unwrap();
    assert_eq!(
        instance_search(
            &bytes,
            &scope,
            &construction,
            RectangularPatternSearchCap::limit
        ),
        InstanceSearch::Capped(RectangularPatternSearchCap::Instances),
        "instances past the local count ceiling stay unresolved"
    );
    let (mut bytes, _, scope, construction) = rectangular_instance_fixture();
    assert_ne!(
        instance_search(&bytes, &scope, &construction, record_bytes),
        InstanceSearch::Capped(RectangularPatternSearchCap::RecordBytes),
        "records within the record-span ceiling pass it"
    );
    bytes.splice(139..139, std::iter::repeat_n(0, RECORD_BYTES));
    assert_eq!(
        instance_search(&bytes, &scope, &construction, record_bytes),
        InstanceSearch::Capped(RectangularPatternSearchCap::RecordBytes),
        "instances past the local record-span ceiling stay unresolved"
    );
}

#[test]
fn rectangular_pattern_run_search_stops_at_its_work_cap() {
    fn append_translation(bytes: &mut Vec<u8>, translation: [f64; 3]) {
        for value in [
            1.0,
            0.0,
            0.0,
            translation[0],
            0.0,
            1.0,
            0.0,
            translation[1],
            0.0,
            0.0,
            1.0,
            translation[2],
            0.0,
            0.0,
            0.0,
            1.0,
        ] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    let decoyed_instances = |decoys: u32, limit: fn(RectangularPatternSearchCap) -> usize| {
        let (_, _, scope, construction) = rectangular_instance_fixture();
        let mut wire =
            crate::records::feature::patterns::DesignRectangularPatternConstructionWire::from(
                construction,
            );
        wire.u_extent = 2.0;
        let construction =
            crate::records::feature::patterns::DesignRectangularPatternConstruction::try_from(wire)
                .unwrap();
        let mut bytes = Vec::new();
        append_transform_record(&mut bytes, 100, [0., 0., 0.]);
        for decoy in 0..decoys {
            append_translation(&mut bytes, [10. + f64::from(decoy), 0., 0.]);
        }
        for index in 50..=53 {
            append_header(&mut bytes, index);
        }
        append_header(&mut bytes, 110);
        append_transform_record(&mut bytes, 120, [0., 0., 1.]);
        append_transform_record(&mut bytes, 130, [0., 0., 2.]);
        for decoy in 0..decoys {
            append_translation(&mut bytes, [-10. - f64::from(decoy), 0., 0.]);
        }
        append_header(&mut bytes, 140);
        instance_search(&bytes, &scope, &construction, limit)
    };
    assert!(
        matches!(
            decoyed_instances(8, RectangularPatternSearchCap::limit),
            InstanceSearch::Resolved(_)
        ),
        "one endpoint pair spans the extent"
    );
    // Nine candidates at each end make 81 endpoint pairs.
    assert_eq!(
        decoyed_instances(8, |cap| match cap {
            RectangularPatternSearchCap::RunWork => 16,
            cap => RectangularPatternSearchCap::limit(cap),
        }),
        InstanceSearch::Capped(RectangularPatternSearchCap::RunWork),
        "endpoint pairs past the local work cap leave the instances unresolved"
    );
}

#[test]
fn rectangular_pattern_aggregate_search_span_caps_the_search() {
    const INSTANCE_COUNT: u32 = 17;
    const RECORD_SPAN: usize = 256;
    let (_, _, mut scope, construction) = rectangular_instance_fixture();
    let mut references = vec![100, 50, 51, 52, 53, 110];
    references.extend(1000..1000 + INSTANCE_COUNT - 1);
    references.push(2000);
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(references);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut bytes = Vec::new();
    append_transform_record(&mut bytes, 100, [0., 0., 0.]);
    bytes.resize(RECORD_SPAN, 0);
    for index in 50..=53 {
        append_header(&mut bytes, index);
    }
    append_header(&mut bytes, 110);
    for index in 1000..1000 + INSTANCE_COUNT - 1 {
        let start = bytes.len();
        append_transform_record(&mut bytes, index, [0., 0., 0.]);
        bytes.resize(start + RECORD_SPAN, 0);
    }
    append_header(&mut bytes, 2000);
    let mut wire =
        crate::records::feature::patterns::DesignRectangularPatternConstructionWire::from(
            construction,
        );
    wire.u_count = INSTANCE_COUNT;
    let construction =
        crate::records::feature::patterns::DesignRectangularPatternConstruction::try_from(wire)
            .unwrap();
    // Each of the 17 instance records spans RECORD_SPAN bytes, and 16 of
    // them fill the aggregate ceiling.
    assert_eq!(
        instance_search(&bytes, &scope, &construction, |cap| match cap {
            RectangularPatternSearchCap::RecordBytes => RECORD_SPAN,
            RectangularPatternSearchCap::ScannedBytes => 16 * RECORD_SPAN,
            cap => RectangularPatternSearchCap::limit(cap),
        }),
        InstanceSearch::Capped(RectangularPatternSearchCap::ScannedBytes),
        "instances past the aggregate span ceiling stay unresolved"
    );
}
