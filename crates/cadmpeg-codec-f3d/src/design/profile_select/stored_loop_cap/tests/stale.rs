// SPDX-License-Identifier: Apache-2.0

use super::super::super::cap_regions::compact::{tests::prism_over, Sweep};
use super::super::super::{EntitySelectionPathResolution, ScopedExtrudeProfileResolution};
use super::{divided_rectangle, xy_sketch, TOLERANCE};
use crate::design::geometry::ProfileGeometryWork;
use crate::history_records::{AsmDeltaState, AsmEntityVersion, AsmHistoricalTopology};
use crate::loss::F3dLossCode;
use crate::records::sketch_geometry::SketchCurveIdentity;
use crate::records::sketch_placement::DesignSketchPlacement;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::entity_selection::{
    DesignProfileCurveEntry, DesignProfileCurveLoop, DesignProfileCurveSelection,
    DesignProfileRegionGroup,
};
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_ir::features::extents::{
    ExtrudeDirection, ExtrudeExtent, ExtrudeSide, ExtrudeStart, LinearTermination,
};
use cadmpeg_ir::features::operation_settings::BooleanOp;
use cadmpeg_ir::features::profiles::PlanarProfileRef;
use cadmpeg_ir::scalar::{NonZeroLength, SlopeAngle};

fn group() -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: "stream:group".into(),
            scope_record_index: 7,
            scope_reference_ordinal: 0,
            record_index: 9,
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("277".to_owned())
                .unwrap(),
            members: vec![
                crate::records::identity::Located {
                    value: 10,
                    offset: 0,
                },
                crate::records::identity::Located {
                    value: 11,
                    offset: 11,
                },
            ],
            lost_edge_references: Vec::new(),
            frame: crate::records::topology::construction::DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 0,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: Vec::new(),
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 1,
                    opaque_index_offset: 18,
                    opaque_scalar: 0.0,
                    opaque_scalar_offset: 22,
                    variant: false,
                    closing_flag: false,
                },
            )
            .unwrap(),
            operand_role:
                crate::records::topology::construction::DesignConstructionOperandRole::Other(
                    DesignOperandRole::ROLE_0X5,
                ),
            role_offset: 0,
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "277".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 0,
        },
    )
    .unwrap()
}

fn placement() -> DesignSketchPlacement {
    DesignSketchPlacement {
        frame: crate::records::sketch_placement::DesignSketchFrame::new(
            0,
            crate::records::sketch_placement::DesignSketchFrameForm::ScopeCompact,
        )
        .unwrap(),
        id: "stream:placement".into(),
        scope_record_index: Some(7),
        entity_id: crate::records::identity::DesignEntityId::try_from("Sketch_42".to_owned())
            .expect("valid entity ID"),

        visibility: None,
        display_name: None,

        class_tag: crate::records::references::DesignClassTag::try_from("277".to_owned()).unwrap(),
        record_index: 20,

        paired_class_tag: crate::records::references::DesignClassTag::try_from("277".to_owned())
            .unwrap(),
    }
}

fn curve(record_index: u32, primary_id: u64, secondary_id: u64) -> SketchCurveIdentity {
    SketchCurveIdentity {
        id: format!("stream:curve-{record_index}"),
        record_index,
        owner_reference: Some(42),
        geometry_group: None,
        class_tag: crate::records::references::DesignClassTag::try_from("450".to_owned()).unwrap(),
        byte_offset: 0,
        geometry_offset: 0,
        entity_genesis: None,
        primary_id: std::num::NonZeroU64::new(primary_id).unwrap(),
        secondary_id,
        geometry: None,
        planar_z_noise: None,
        tail_marker: None,
    }
}

fn selection() -> DesignProfileCurveSelection {
    let mut selection = DesignProfileCurveSelection {
        id: "stream:selection".into(),
        scope_record_index: 7,
        group_record_index: 9,
        group_member_ordinal: 0,
        record_index: 10,
        byte_offset: 100,
        owner_text: "42".into(),
        owner_text_offset: 200,
        sketch_scope_record_index: 7,
        sketch_scope_offset: 300,
        scope_sketch_containers: Vec::new(),
        identity_record_offset: 500,
        identity_end_offset: 0,
        groups: vec![DesignProfileRegionGroup {
            further: Vec::new(),
            region: vec![DesignProfileCurveLoop {
                outer: true,
                entries: [1, 2, 3, 4, 999]
                    .into_iter()
                    .enumerate()
                    .map(|(i, id)| DesignProfileCurveEntry {
                        primary_id: id,
                        secondary_id: 0,
                        primary_id_offset: 588 + u64::try_from(i).unwrap() * 40,
                        entry_fields: [0, 1, 1],
                    })
                    .collect(),
            }],
        }],
    };
    selection.identity_end_offset = selection.lists_end().unwrap();
    assert!(selection.valid_layout());
    assert!(!selection.compact_references());
    selection
}

fn join(length: f64, draft: Option<SlopeAngle>) -> ExtrudeExtent {
    ExtrudeExtent::OneSided {
        side: ExtrudeSide {
            termination: LinearTermination::Blind {
                length: NonZeroLength::new(length).unwrap(),
            },
            draft,
        },
    }
}

fn select(
    mutate: impl FnOnce(&mut DesignProfileCurveSelection, &mut Vec<SketchCurveIdentity>),
    width: f64,
) -> Option<(PlanarProfileRef, Vec<F3dLossCode>)> {
    select_through(
        mutate,
        added(prism_over(width)),
        BooleanOp::Join,
        &join(2.0, None),
    )
}

type Versioned = (AsmHistoricalTopology, Vec<AsmEntityVersion>);

/// A transition from no body to `after`.
fn added(after: AsmHistoricalTopology) -> (Versioned, Versioned) {
    (
        (AsmHistoricalTopology::default(), Vec::new()),
        (after, Vec::new()),
    )
}

/// A Cut from the sketch plane that removes the rectangle from the origin to
/// `(2, 1)` out of the prism of height 2 over the rectangle to `(4, 1)`. The
/// wall at `x = 0` is deleted, a wall at `x = 2` is inserted, and the faces
/// at heights 0 and 2 and along `y = 0` and `y = 1` are updated.
fn cut() -> (Versioned, Versioned) {
    let versions = |faces: &[(i64, i64)]| {
        faces
            .iter()
            .map(|&(entity_ref, record_ref)| AsmEntityVersion {
                entity_ref,
                record_ref,
            })
            .collect::<Vec<_>>()
    };
    let before = prism_over(4.0);
    let mut after = prism_over(2.0);
    let offset = cadmpeg_ir::math::Vector3::new(2.0, 0.0, 0.0);
    for row in &mut after.point_positions {
        row.position = row.position.translated(offset, 1.0);
    }
    for row in &mut after.surface_planes {
        row.origin = row.origin.translated(offset, 1.0);
    }
    for row in &mut after.curve_axes {
        row.origin = row.origin.translated(offset, 1.0);
    }
    for face in &mut after.faces {
        if *face == 5 {
            *face = 6;
        }
    }
    for row in &mut after.face_surfaces {
        if row.entity == 5 {
            row.entity = 6;
        }
    }
    for row in &mut after.face_loops {
        if row.owner_ref == 5 {
            row.owner_ref = 6;
        }
    }
    (
        (
            before,
            versions(&[(0, 1), (1, 1), (2, 1), (3, 1), (4, 1), (5, 1)]),
        ),
        (
            after,
            versions(&[(0, 2), (1, 2), (2, 2), (3, 1), (4, 2), (6, 1)]),
        ),
    )
}

/// The stale selection resolved through the transition from `states.0` to
/// `states.1` of a sweep from the profile plane along its normal.
fn select_through(
    mutate: impl FnOnce(&mut DesignProfileCurveSelection, &mut Vec<SketchCurveIdentity>),
    states: (Versioned, Versioned),
    operation: BooleanOp,
    extent: &ExtrudeExtent,
) -> Option<(PlanarProfileRef, Vec<F3dLossCode>)> {
    let mut group = group();
    group
        .try_set_members(vec![crate::records::identity::Located {
            value: 10,
            offset: 0,
        }])
        .unwrap();
    let placements = [placement()];
    let mut sketch = xy_sketch();
    sketch.id = crate::ids::neutral_sketch_id(&placements[0]);
    let source_entities = divided_rectangle();
    let mut curves = source_entities
        .iter()
        .enumerate()
        .map(|(i, _)| {
            curve(
                u32::try_from(i + 1).unwrap(),
                u64::try_from(i + 1).unwrap(),
                0,
            )
        })
        .collect::<Vec<_>>();
    let mut selection = selection();
    mutate(&mut selection, &mut curves);
    selection.identity_end_offset = selection.lists_end().unwrap();
    let entities = curves
        .iter()
        .map(|curve| {
            let shared = curves
                .iter()
                .filter(|other| {
                    other.primary_id == curve.primary_id && other.secondary_id == curve.secondary_id
                })
                .count()
                > 1;
            let id = if shared {
                crate::ids::neutral_sketch_record_id(&sketch.id, curve.record_index)
            } else {
                crate::ids::neutral_sketch_curve_id(
                    &sketch.id,
                    curve.primary_id.get(),
                    curve.secondary_id,
                )
            };
            cadmpeg_ir::sketches::SketchEntity::new(
                id,
                sketch.id.clone(),
                source_entities[usize::try_from(curve.primary_id.get() - 1).unwrap()]
                    .geometry
                    .clone(),
            )
        })
        .collect::<Vec<_>>();
    let states = [(2, states.1), (1, states.0)]
        .into_iter()
        .map(|(state_id, (topology, versions))| {
            (
                AsmDeltaState {
                    id: format!("synthetic:test:state#{state_id}"),
                    parent: "synthetic:test:history#stale".into(),
                    byte_offset: 0,
                    state_id,
                    version_flag: 1,
                    state_flag: 0,
                    previous_ref: None,
                    next_ref: None,
                    node_index: state_id,
                    partner_ref: None,
                    owner_ref: 0,
                    bulletin_boards: Vec::new(),
                    records: Vec::new(),
                    transition: None,
                },
                Some(topology),
                versions,
            )
        })
        .collect();
    let history = crate::test_support::history::history(
        "synthetic:test:history#stale".into(),
        0,
        None,
        false,
        states,
    );
    let sketches = [sketch];
    let selections = [selection];
    crate::test_support::with_decode_context(|ctx| {
        let work = ProfileGeometryWork::new();
        let resolution = EntitySelectionPathResolution {
            operands: &[],
            profile_curves: &selections,
            placements: &placements,
            curve_identities: &curves,
            sketches: &sketches,
            sketch_entities: &entities,
            spatial_sketches: &[],
            spatial_sketch_entities: &[],
        };
        super::super::resolve_stale(
            &group,
            &resolution,
            ScopedExtrudeProfileResolution {
                entities: &entities,
                spatial_entities: &[],
                histories: std::slice::from_ref(&history),
                linear_tolerance: TOLERANCE,
                angular_tolerance: TOLERANCE,
                profile_work: &work,
                ctx,
            },
            (2, 1),
            Sweep {
                operation: &operation,
                start: &ExtrudeStart::ProfilePlane {},
                direction: &ExtrudeDirection::ProfileNormal {},
                extent,
            },
        )
    })
    .unwrap()
}

#[test]
fn surviving_typed_members_require_the_complete_certified_union_boundary() {
    let (selected, losses) = select(|_, _| {}, 2.0).expect("certified union");
    assert_eq!(
        losses,
        vec![F3dLossCode::FeatureProfileRegionIdentitiesStale]
    );
    let PlanarProfileRef::SketchRegions { regions, .. } = selected else {
        panic!("trimmed regions")
    };
    assert_eq!(regions.len(), 2);
    assert!(select(|_, _| {}, 1.0).is_none());
}

#[test]
fn an_absent_boundary_member_keeps_the_selection_native() {
    assert!(select(
        |_, curves| curves.retain(|curve| curve.primary_id.get() != 1),
        2.0
    )
    .is_none());
}

#[test]
fn several_live_records_for_one_identity_keep_the_selection_native() {
    assert!(select(|_, curves| curves.push(curve(99, 1, 0)), 2.0).is_none());
}

#[test]
fn a_missing_hole_keeps_the_selection_native() {
    assert!(select(
        |selection, _| {
            selection.groups[0].region.push(DesignProfileCurveLoop {
                outer: false,
                entries: vec![DesignProfileCurveEntry {
                    primary_id: 998,
                    secondary_id: 0,
                    primary_id_offset: 793,
                    entry_fields: [0, 1, 1],
                }],
            });
        },
        2.0
    )
    .is_none());
}

#[test]
fn a_fully_bound_selection_uses_the_complete_member_rule() {
    assert!(select(
        |selection, _| {
            selection.groups[0].region[0].entries.pop();
        },
        2.0
    )
    .is_none());
}

#[test]
fn an_unbound_owner_keeps_the_selection_native() {
    assert!(select(|selection, _| selection.owner_text = "43".into(), 2.0).is_none());
}

#[test]
fn an_unknown_traversal_keeps_the_selection_native() {
    assert!(select(
        |selection, _| selection.groups[0].region[0].entries[0].entry_fields[0] = 2,
        2.0
    )
    .is_none());
}

#[test]
fn surviving_typed_members_certify_a_new_body_and_a_cut() {
    for (states, operation) in [
        (added(prism_over(2.0)), BooleanOp::NewBody),
        (cut(), BooleanOp::Cut),
    ] {
        let (selected, losses) = select_through(|_, _| {}, states, operation, &join(2.0, None))
            .unwrap_or_else(|| panic!("{operation:?} certifies the union"));
        assert_eq!(
            losses,
            vec![F3dLossCode::FeatureProfileRegionIdentitiesStale]
        );
        let PlanarProfileRef::SketchRegions { regions, .. } = selected else {
            panic!("trimmed regions")
        };
        assert_eq!(regions.len(), 2);
    }
}

#[test]
fn a_drafted_or_two_sided_sweep_keeps_the_selection_native() {
    let side = || ExtrudeSide {
        termination: LinearTermination::Blind {
            length: NonZeroLength::new(2.0).unwrap(),
        },
        draft: None,
    };
    for extent in [
        join(2.0, Some(SlopeAngle::new(0.1).unwrap())),
        ExtrudeExtent::TwoSided {
            first: side(),
            second: side(),
        },
        ExtrudeExtent::Symmetric { side: side() },
    ] {
        assert!(
            select_through(|_, _| {}, added(prism_over(2.0)), BooleanOp::Join, &extent).is_none()
        );
    }
}
