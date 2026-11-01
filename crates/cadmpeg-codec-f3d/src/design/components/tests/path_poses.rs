// SPDX-License-Identifier: Apache-2.0
//! Component-path world poses applied to expanded nested occurrences.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::ids::OccurrenceId;

use super::{
    carrier, guid, nested_fixture, placed, ComponentDefinition, DesignComponentOccurrence,
    StreamComponents, STREAM,
};
use crate::loss::F3dLossCode;
use crate::records::component_path::{
    ComponentPathTransform, DesignComponentPath, DesignComponentPathElement,
    DesignComponentPathElementKind as Kind,
};
use crate::records::feature::assembly_features::DesignComponentOccurrencePlacement;

const TOLERANCE: f64 = 1e-9;
const SUB_MODULE: u64 = 500;
const LEAF: u64 = 600;
const SUB_COMPONENT: u64 = 150;
const SUB_ENTITY: u64 = 151;

fn pose(rows: [[f64; 3]; 3], translation: [f64; 3]) -> [[f64; 4]; 4] {
    [
        [rows[0][0], rows[0][1], rows[0][2], translation[0]],
        [rows[1][0], rows[1][1], rows[1][2], translation[1]],
        [rows[2][0], rows[2][1], rows[2][2], translation[2]],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

const EYE: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

fn path(carrier: u64, elements: &[(u64, Kind)], rows: [[f64; 4]; 4]) -> DesignComponentPath {
    DesignComponentPath {
        id: format!("{STREAM}:design-component-path#{carrier}"),
        carrier_record_index: carrier,
        elements: elements
            .iter()
            .zip(0_u32..)
            .map(
                |((record_index, kind), discriminator)| DesignComponentPathElement {
                    record_index: *record_index,
                    kind: *kind,
                    discriminator,
                },
            )
            .collect(),
        transform: Some(ComponentPathTransform::try_from(rows).unwrap()),
        frame_bytes: Vec::new().into(),
    }
}

/// The nested fixture plus a sub-module (record 500, definition 150) of the
/// module (record 100) that holds a leaf (record 600) of definition 159.
/// `modules` is the number of root-contained instances of the module.
fn fixture(
    modules: usize,
) -> (
    Vec<DesignComponentOccurrence>,
    BTreeMap<String, StreamComponents>,
) {
    let (mut carriers, mut components) = nested_fixture();
    components.get_mut(STREAM).unwrap().roots.insert(
        SUB_COMPONENT,
        ComponentDefinition {
            root_entity: SUB_ENTITY,
            context_uuid: guid("00000000-0000-4000-8000-000000000150"),
            name: Some("Sub".to_owned()),
        },
    );
    let mut sub = carrier(
        SUB_MODULE,
        "00000000-0000-4000-8000-000000000005",
        placed(3, pose(EYE, [9.0, 9.0, 9.0])),
        101,
    );
    sub.component_record_index = SUB_COMPONENT;
    let leaf = carrier(
        LEAF,
        "00000000-0000-4000-8000-000000000006",
        DesignComponentOccurrencePlacement::Base,
        SUB_ENTITY,
    );
    carriers.extend([sub, leaf]);
    if modules == 2 {
        let mut second = carrier(
            400,
            "00000000-0000-4000-8000-000000000004",
            placed(1, pose(EYE, [1.0, 1.0, 1.0])),
            super::DOCUMENT_ROOT,
        );
        second.component_record_index = 100;
        carriers.push(second);
    }
    (carriers, components)
}

fn project(
    carriers: &[DesignComponentOccurrence],
    components: &BTreeMap<String, StreamComponents>,
    paths: &[DesignComponentPath],
) -> super::super::LocalComponentProjection {
    let empty = BTreeSet::new();
    super::super::project_local_components(&super::super::LocalComponentSources {
        hidden_bodies: None,
        scopes: &[],
        occurrences: carriers,
        paths,
        components,
        body_bindings: &[],
        bodies: Some(&empty),
        root_ordinal_start: 0,
        limits: cadmpeg_core::decode::ResourceLimits::default(),
    })
    .unwrap()
}

fn by_record(carriers: &[DesignComponentOccurrence], record: u32) -> &DesignComponentOccurrence {
    carriers
        .iter()
        .find(|carrier| carrier.record_index == record)
        .unwrap()
}

/// The instance of `record` below the chain of records `outer`, outermost first.
fn instance(carriers: &[DesignComponentOccurrence], chain: &[u32]) -> OccurrenceId {
    let (first, rest) = chain.split_first().unwrap();
    let mut id =
        crate::ids::neutral_component_occurrence_id(&by_record(carriers, *first).occurrence_guid);
    for record in rest {
        id = crate::ids::nested_component_occurrence_id(
            &id,
            &by_record(carriers, *record).occurrence_guid,
        );
    }
    id
}

fn world(projection: &super::super::LocalComponentProjection, id: &OccurrenceId) -> [[f64; 4]; 4] {
    let graph = cadmpeg_ir::products::AssemblyGraph::new(&projection.occurrences).unwrap();
    cadmpeg_test_support::assembly::resolved_transform(&graph, id)
        .unwrap()
        .rows()
}

fn local(projection: &super::super::LocalComponentProjection, id: &OccurrenceId) -> [[f64; 4]; 4] {
    projection
        .occurrences
        .iter()
        .find(|occurrence| &occurrence.id == id)
        .unwrap()
        .transform
        .rows()
}

/// A centimetre pose as the millimetre world matrix it states.
fn millimetres(mut rows: [[f64; 4]; 4]) -> [[f64; 4]; 4] {
    for row in &mut rows[..3] {
        row[3] *= 10.0;
    }
    rows
}

fn assert_close(actual: [[f64; 4]; 4], expected: [[f64; 4]; 4]) {
    for (left, right) in actual.iter().flatten().zip(expected.iter().flatten()) {
        assert!(
            (left - right).abs() < TOLERANCE,
            "{actual:?} != {expected:?}"
        );
    }
}

fn codes(projection: &super::super::LocalComponentProjection) -> Vec<String> {
    projection
        .losses
        .iter()
        .map(|loss| format!("{:?}", loss.code))
        .collect()
}

fn has(projection: &super::super::LocalComponentProjection, code: F3dLossCode) -> bool {
    projection
        .losses
        .iter()
        .any(|loss| loss.code == code.kind())
}

#[test]
fn paths_of_two_and_more_elements_place_nested_instances_outermost_inward() {
    let (carriers, components) = fixture(1);
    let leaf_pose = pose(
        [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
        [0.5, 0.25, 2.0],
    );
    // A mirrored placement is valid.
    let sub_pose = pose(
        [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        [2.0, 0.0, 1.0],
    );
    let two = pose(EYE, [1.0, 2.0, 3.0]);
    let paths = [
        path(900, &[(159, Kind::Component), (200, Kind::Occurrence)], two),
        path(
            901,
            &[
                (SUB_COMPONENT, Kind::Component),
                (SUB_MODULE, Kind::Occurrence),
                (100, Kind::Occurrence),
            ],
            sub_pose,
        ),
        // Elements in record order, as stored: the component first here, the
        // leaf last, the containing records between.
        path(
            902,
            &[
                (159, Kind::Component),
                (100, Kind::Occurrence),
                (SUB_MODULE, Kind::Occurrence),
                (LEAF, Kind::Occurrence),
            ],
            leaf_pose,
        ),
    ];
    let projection = project(&carriers, &components, &paths);
    let leaf = instance(&carriers, &[100, 500, 600]);
    let sub = instance(&carriers, &[100, 500]);
    let direct = instance(&carriers, &[100, 200]);
    assert_close(world(&projection, &direct), millimetres(two));
    assert_close(world(&projection, &sub), millimetres(sub_pose));
    // The sub-module's stored placement was stale; the leaf still lands on its
    // own pose because its parent was corrected first.
    assert_close(world(&projection, &leaf), millimetres(leaf_pose));
    let determinant = {
        let rows = world(&projection, &sub);
        rows[0][0] * rows[1][1] * rows[2][2]
    };
    assert!(determinant < 0.0);
    assert!(has(&projection, F3dLossCode::LocalOccurrencePathPose));
    assert!(!has(
        &projection,
        F3dLossCode::LocalOccurrencePathPoseUnresolved
    ));
    assert!(!has(
        &projection,
        F3dLossCode::LocalOccurrencePathPoseDisagrees
    ));
    // The sibling that no path names keeps its stored placement.
    let stored = local(
        &project(&carriers, &components, &[]),
        &instance(&carriers, &[100, 300]),
    );
    assert_close(
        local(&projection, &instance(&carriers, &[100, 300])),
        stored,
    );
}

#[test]
fn contained_records_decide_between_instances_of_one_definition() {
    let (carriers, components) = fixture(2);
    let bare = path(
        900,
        &[(159, Kind::Component), (200, Kind::Occurrence)],
        pose(EYE, [1.0, 2.0, 3.0]),
    );
    let projection = project(&carriers, &components, std::slice::from_ref(&bare));
    // Two instances of record 200 match, so the carrier applies to neither.
    assert!(has(
        &projection,
        F3dLossCode::LocalOccurrencePathPoseUnresolved
    ));
    assert!(!has(&projection, F3dLossCode::LocalOccurrencePathPose));
    let unplaced = project(&carriers, &components, &[]);
    for module in [100, 400] {
        let id = instance(&carriers, &[module, 200]);
        assert_close(local(&projection, &id), local(&unplaced, &id));
    }

    let named = path(
        901,
        &[
            (159, Kind::Component),
            (200, Kind::Occurrence),
            (400, Kind::Occurrence),
        ],
        pose(EYE, [4.0, 0.0, 0.0]),
    );
    let projection = project(&carriers, &components, &[bare, named.clone()]);
    let placed = instance(&carriers, &[400, 200]);
    assert_close(
        world(&projection, &placed),
        millimetres(pose(EYE, [4.0, 0.0, 0.0])),
    );
    let other = instance(&carriers, &[100, 200]);
    assert_close(local(&projection, &other), local(&unplaced, &other));
    assert!(has(&projection, F3dLossCode::LocalOccurrencePathPose));
    assert!(has(
        &projection,
        F3dLossCode::LocalOccurrencePathPoseUnresolved
    ));
}

#[test]
fn unmatched_paths_are_refused_with_a_loss() {
    let (carriers, components) = fixture(1);
    let unplaced = project(&carriers, &components, &[]);
    let rows = pose(EYE, [8.0, 8.0, 8.0]);
    let refused = [
        // A leaf below a record that does not contain it.
        path(
            900,
            &[
                (159, Kind::Component),
                (200, Kind::Occurrence),
                (SUB_MODULE, Kind::Occurrence),
            ],
            rows,
        ),
        // No component element.
        path(
            901,
            &[(200, Kind::Occurrence), (300, Kind::Occurrence)],
            rows,
        ),
        // An element that no carrier names.
        path(
            902,
            &[
                (159, Kind::Component),
                (200, Kind::Occurrence),
                (777, Kind::Other),
            ],
            rows,
        ),
        // A component that is not the leaf's definition.
        path(
            903,
            &[(150, Kind::Component), (200, Kind::Occurrence)],
            rows,
        ),
    ];
    let projection = project(&carriers, &components, &refused);
    assert!(!has(&projection, F3dLossCode::LocalOccurrencePathPose));
    let message = projection
        .losses
        .iter()
        .find(|loss| loss.code == F3dLossCode::LocalOccurrencePathPoseUnresolved.kind())
        .map(|loss| loss.message.clone())
        .expect("unresolved loss");
    assert!(
        message.starts_with("4 component-path carrier(s)"),
        "{message}"
    );
    for chain in [&[100_u32, 200][..], &[100, 300], &[100, 500, 600]] {
        let id = instance(&carriers, chain);
        assert_close(local(&projection, &id), local(&unplaced, &id));
    }
}

#[test]
fn carriers_that_pose_one_instance_differently_keep_the_stored_placement() {
    let (carriers, components) = fixture(1);
    let unplaced = project(&carriers, &components, &[]);
    let elements = [(159, Kind::Component), (200, Kind::Occurrence)];
    let id = instance(&carriers, &[100, 200]);

    let agreeing = [
        path(900, &elements, pose(EYE, [1.0, 0.0, 0.0])),
        path(901, &elements, pose(EYE, [1.0, 0.0, 0.0])),
    ];
    let projection = project(&carriers, &components, &agreeing);
    assert_close(
        world(&projection, &id),
        millimetres(pose(EYE, [1.0, 0.0, 0.0])),
    );
    assert!(!has(
        &projection,
        F3dLossCode::LocalOccurrencePathPoseDisagrees
    ));

    let disagreeing = [
        path(900, &elements, pose(EYE, [1.0, 0.0, 0.0])),
        path(901, &elements, pose(EYE, [2.0, 0.0, 0.0])),
    ];
    let projection = project(&carriers, &components, &disagreeing);
    assert_close(local(&projection, &id), local(&unplaced, &id));
    assert!(has(
        &projection,
        F3dLossCode::LocalOccurrencePathPoseDisagrees
    ));
    assert!(
        !has(&projection, F3dLossCode::LocalOccurrencePathPose),
        "{:?}",
        codes(&projection)
    );
}

#[test]
fn a_pose_equal_to_the_stored_placement_reports_no_loss() {
    let (carriers, components) = fixture(1);
    let unplaced = project(&carriers, &components, &[]);
    let id = instance(&carriers, &[100, 200]);
    let rows = world(&unplaced, &id);
    let mut centimetres = rows;
    for row in &mut centimetres[..3] {
        row[3] /= 10.0;
    }
    let same = path(
        900,
        &[(159, Kind::Component), (200, Kind::Occurrence)],
        centimetres,
    );
    let projection = project(&carriers, &components, &[same]);
    assert_close(world(&projection, &id), rows);
    assert!(!has(&projection, F3dLossCode::LocalOccurrencePathPose));
}

fn multiply(left: [[f64; 4]; 4], right: [[f64; 4]; 4]) -> [[f64; 4]; 4] {
    let mut product = [[0.0; 4]; 4];
    for (row, out) in product.iter_mut().enumerate() {
        for (column, cell) in out.iter_mut().enumerate() {
            *cell = (0..4).map(|k| left[row][k] * right[k][column]).sum();
        }
    }
    product
}

/// A millimetre world matrix as the centimetre pose a carrier stores.
fn centimetres(mut rows: [[f64; 4]; 4]) -> [[f64; 4]; 4] {
    for row in &mut rows[..3] {
        row[3] /= 10.0;
    }
    rows
}

/// The pose of a leaf with definition-local pose `local` inside `holder`.
fn pose_in(
    projection: &super::super::LocalComponentProjection,
    carriers: &[DesignComponentOccurrence],
    holder: &[u32],
    local: [[f64; 4]; 4],
) -> [[f64; 4]; 4] {
    centimetres(multiply(
        world(projection, &instance(carriers, holder)),
        millimetres(local),
    ))
}

/// A root-contained instance of the module at the placement of record 100.
fn module_at_first(record: u64, guid_suffix: u32) -> DesignComponentOccurrence {
    let mut module = carrier(
        record,
        &format!("00000000-0000-4000-8000-000000000{guid_suffix:03}"),
        placed(
            1,
            pose(
                [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
                [3.0, 4.0, 0.0],
            ),
        ),
        super::DOCUMENT_ROOT,
    );
    module.component_record_index = 100;
    module
}

fn leaf_local() -> [[f64; 4]; 4] {
    pose(
        [[1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]],
        [0.5, 0.25, 2.0],
    )
}

fn bare(carrier: u64, record: u64, rows: [[f64; 4]; 4]) -> DesignComponentPath {
    path(
        carrier,
        &[(159, Kind::Component), (record, Kind::Occurrence)],
        rows,
    )
}

fn message(projection: &super::super::LocalComponentProjection, code: F3dLossCode) -> String {
    projection
        .losses
        .iter()
        .find(|loss| loss.code == code.kind())
        .map(|loss| loss.message.clone())
        .unwrap_or_default()
}

#[test]
fn a_unique_consistent_assignment_places_carriers_with_the_inferred_loss() {
    let (carriers, components) = fixture(2);
    let unplaced = project(&carriers, &components, &[]);
    let first = pose_in(&unplaced, &carriers, &[100], leaf_local());
    let second = pose_in(&unplaced, &carriers, &[400], leaf_local());
    // The carriers are stored in the reverse of the module order.
    let paths = [bare(900, 200, second), bare(901, 200, first)];
    let projection = project(&carriers, &components, &paths);
    assert_close(
        world(&projection, &instance(&carriers, &[100, 200])),
        millimetres(first),
    );
    assert_close(
        world(&projection, &instance(&carriers, &[400, 200])),
        millimetres(second),
    );
    assert!(has(
        &projection,
        F3dLossCode::LocalOccurrencePathPoseInferred
    ));
    assert!(!has(
        &projection,
        F3dLossCode::LocalOccurrencePathPoseUnresolved
    ));
    let inferred = message(&projection, F3dLossCode::LocalOccurrencePathPoseInferred);
    assert!(
        inferred.starts_with("2 component-path carrier(s)"),
        "{inferred}"
    );
    assert!(
        inferred.contains("2 occurrence(s) in 1 target(s)"),
        "{inferred}"
    );
}

#[test]
fn two_consistent_assignments_are_refused() {
    let (mut carriers, components) = fixture(2);
    // Two modules at one placement: either assignment gives equal poses.
    carriers.retain(|carrier| carrier.record_index != 400);
    carriers.push(module_at_first(400, 4));
    let unplaced = project(&carriers, &components, &[]);
    let rows = pose_in(&unplaced, &carriers, &[100], leaf_local());
    let paths = [bare(900, 200, rows), bare(901, 200, rows)];
    let projection = project(&carriers, &components, &paths);
    assert!(has(
        &projection,
        F3dLossCode::LocalOccurrencePathPoseUnresolved
    ));
    assert!(!has(
        &projection,
        F3dLossCode::LocalOccurrencePathPoseInferred
    ));
    assert!(!has(&projection, F3dLossCode::LocalOccurrencePathPose));
    for module in [100, 400] {
        let id = instance(&carriers, &[module, 200]);
        assert_close(local(&projection, &id), local(&unplaced, &id));
    }
}

#[test]
fn no_consistent_assignment_is_refused() {
    let (carriers, components) = fixture(2);
    let unplaced = project(&carriers, &components, &[]);
    let first = pose_in(&unplaced, &carriers, &[100], leaf_local());
    let mut second = pose_in(&unplaced, &carriers, &[400], leaf_local());
    second[0][3] += 0.5;
    let paths = [bare(900, 200, first), bare(901, 200, second)];
    let projection = project(&carriers, &components, &paths);
    assert!(has(
        &projection,
        F3dLossCode::LocalOccurrencePathPoseUnresolved
    ));
    assert!(!has(
        &projection,
        F3dLossCode::LocalOccurrencePathPoseInferred
    ));
    assert!(
        message(&projection, F3dLossCode::LocalOccurrencePathPoseUnresolved)
            .starts_with("2 component-path carrier(s)")
    );
    for module in [100, 400] {
        let id = instance(&carriers, &[module, 200]);
        assert_close(local(&projection, &id), local(&unplaced, &id));
    }
}

#[test]
fn an_enumeration_past_the_search_limit_is_refused() {
    let (mut carriers, components) = fixture(1);
    // Ten modules at one placement and ten carriers; the last disagrees, so
    // no full assignment exists but the prefixes number 10!/1.
    for ordinal in 0..9_u32 {
        carriers.push(module_at_first(700 + u64::from(ordinal), 900 + ordinal));
    }
    let unplaced = project(&carriers, &components, &[]);
    let rows = pose_in(&unplaced, &carriers, &[100], leaf_local());
    let mut odd = rows;
    odd[1][3] += 1.0;
    let mut paths = (0..9).map(|n| bare(900 + n, 200, rows)).collect::<Vec<_>>();
    paths.push(bare(950, 200, odd));
    let projection = project(&carriers, &components, &paths);
    assert!(
        message(&projection, F3dLossCode::LocalOccurrencePathPoseUnresolved)
            .starts_with("10 component-path carrier(s)")
    );
    assert!(!has(
        &projection,
        F3dLossCode::LocalOccurrencePathPoseInferred
    ));
}

#[test]
fn a_stale_holder_is_corrected_before_its_leaf_carriers_are_matched() {
    let (carriers, components) = fixture(2);
    let sub_pose = pose(EYE, [2.0, 3.0, 4.0]);
    let sub = path(
        900,
        &[
            (SUB_COMPONENT, Kind::Component),
            (SUB_MODULE, Kind::Occurrence),
            (100, Kind::Occurrence),
        ],
        sub_pose,
    );
    let holders = project(&carriers, &components, std::slice::from_ref(&sub));
    let first = pose_in(&holders, &carriers, &[100, 500], leaf_local());
    let second = pose_in(&holders, &carriers, &[400, 500], leaf_local());
    let leaf = |carrier, rows| {
        path(
            carrier,
            &[(159, Kind::Component), (LEAF, Kind::Occurrence)],
            rows,
        )
    };
    let paths = [sub, leaf(901, first), leaf(902, second)];
    let projection = project(&carriers, &components, &paths);
    assert_close(
        world(&projection, &instance(&carriers, &[100, 500, 600])),
        millimetres(first),
    );
    assert_close(
        world(&projection, &instance(&carriers, &[400, 500, 600])),
        millimetres(second),
    );
    assert!(has(
        &projection,
        F3dLossCode::LocalOccurrencePathPoseInferred
    ));
}
