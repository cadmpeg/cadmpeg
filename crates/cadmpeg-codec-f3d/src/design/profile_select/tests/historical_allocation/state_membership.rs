// SPDX-License-Identifier: Apache-2.0
//! State membership uses the caller context and preserves historical selection.

use super::*;
use cadmpeg_core::decode::refusal_probe::RefusalProbe;

const EPS_PROFILE_SELECTION: f64 = 0.000_001;

fn query(
    ctx: &DecodeContext<'_>,
    members: &[&DesignExtrudeSelectionMember],
    sketch: &Sketch,
    entities: &[SketchEntity],
    histories: &[AsmHistory],
) -> Result<Option<super::super::super::ResolvedProfileSelection>, CodecError> {
    let budget = WorkBudget::new(MAX_ARRANGEMENT_WALK_WORK);
    super::super::super::historical_selection_regions(
        members, sketch, entities, histories, EPS_PROFILE_SELECTION, &budget, ctx,
    )
}

fn assert_state_refusal(operation: &'static str) {
    let mut first = historical_point_member();
    first.historical.as_mut().unwrap().state_ids = vec![3, 2, 3];
    let second = historical_point_member();
    let original = [first.clone(), second.clone()];
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, Some(1));
        let Err(CodecError::ResourceLimit(original)) = query(ctx, &[&first, &second], &empty_sketch(), &[], &[]) else {
            panic!("actual state query must refuse");
        };
        assert_eq!(original.operation, operation);
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!(original.additional, 1);
        assert_eq!(ctx.resource_refusal(), Some(original));
        assert!(matches!(ctx.charge_work(0, "later historical profile boundary"), Err(CodecError::ResourceLimit(later)) if later == original));
    });
    assert_eq!([first, second], original);
}

#[test]
fn historical_first_member_state_preserves_original_refusal() {
    assert_state_refusal("visit F3D historical selection first member state");
}

#[test]
fn historical_duplicate_state_preserves_original_refusal() {
    assert_state_refusal("visit F3D historical selection duplicate state");
}

#[test]
fn historical_intersecting_member_preserves_original_refusal() {
    assert_state_refusal("visit F3D historical selection intersecting member");
}

#[test]
fn historical_intersection_state_preserves_original_refusal() {
    assert_state_refusal("visit F3D historical selection intersection state");
}

#[test]
fn historical_member_binding_state_preserves_original_refusal() {
    assert_state_refusal("visit F3D historical selection member binding state");
}

#[test]
fn historical_selected_state_preserves_original_refusal() {
    assert_state_refusal("visit F3D historical selection selected state");
}

fn two_regions() -> (Sketch, Vec<SketchEntity>, Vec<AsmHistory>) {
    let mut sketch = empty_sketch();
    let mut entities = Vec::new();
    for region in 0..2 {
        let origin = f64::from(region * 2);
        let corners = [Point2::new(origin, 0.0), Point2::new(origin + 1.0, 0.0), Point2::new(origin + 1.0, 1.0), Point2::new(origin, 1.0)];
        let mut boundary = Vec::new();
        for edge in 0..4 {
            let id = SketchEntityId::mint(format!("synthetic:profile-membership:edge#{region}-{edge}")).unwrap();
            entities.push(SketchEntity::new(id.clone(), sketch.id.clone(), SketchGeometry::try_from(SketchGeometryDefinition::Line { start: corners[edge], end: corners[(edge + 1) % 4] }).unwrap()));
            boundary.push(SketchEntityUse { entity: id, reversed: false });
        }
        sketch.profiles.try_push(boundary).unwrap();
    }
    let states = [(2, 0.5), (3, 2.5)].map(|(state, x)| transition_state(state, AsmHistoricalTopology {
        point_positions: vec![AsmHistoricalPoint { point: 40, position: Point3::new(x, 0.5, 0.0) }],
        ..AsmHistoricalTopology::default()
    }, None));
    let histories = vec![AsmHistory { id: "synthetic:profile-membership:history".into(), byte_offset: 0, preamble: None, record_table_binding_budget_exceeded: false, states: states.into() }];
    (sketch, entities, histories)
}

#[test]
fn historical_state_intersection_preserves_selected_region_and_native_inputs() {
    let (sketch, entities, histories) = two_regions();
    let mut first = historical_point_member();
    first.historical.as_mut().unwrap().state_ids = vec![3, 2, 3, 2];
    let mut second = historical_point_member();
    second.historical.as_mut().unwrap().state_ids.extend(std::iter::repeat_n(99, 4096));
    let original = [first.clone(), second.clone()];
    crate::test_support::with_decode_context(|ctx| {
        let Some(super::super::super::ResolvedProfileSelection::Regions(regions)) = query(ctx, &[&first, &second], &sketch, &entities, &histories).unwrap() else { panic!("the common state's interior selects a region"); };
        let [SketchProfileRegion::Trimmed { outer_boundary, hole_boundaries }] = regions.as_slice() else { panic!("the common state's square has one trimmed boundary"); };
        assert!(hole_boundaries.is_empty());
        assert_eq!(outer_boundary.as_slice().len(), 4);
        for (actual, expected) in outer_boundary.as_slice().iter().zip(&sketch.profiles[0]) {
            assert_eq!(actual.entity, expected.entity);
            assert_eq!(actual.reversed, expected.reversed);
            assert_eq!(actual.parameter_range.endpoints(), [0.0, 1.0]);
        }
        assert_eq!(ctx.resource_refusal(), None);
    });
    assert_eq!([first, second], original);
}

#[test]
fn historical_missing_and_empty_binding_preserve_no_common_state() {
    let (sketch, entities, histories) = two_regions();
    let first = historical_point_member();
    for absent in [false, true] {
        let mut second = historical_point_member();
        if absent { second.historical = None; }
        else { second.historical.as_mut().unwrap().state_ids.clear(); }
        crate::test_support::with_decode_context(|ctx| {
            assert!(query(ctx, &[&first, &second], &sketch, &entities, &histories).unwrap().is_none());
            assert_eq!(ctx.resource_refusal(), None);
        });
    }
}

#[test]
fn historical_state_scratch_refuses_before_backing_growth_and_refunds_after_drop() {
    let member = historical_point_member();
    let bytes = u64::try_from(4 * std::mem::size_of::<i64>()).unwrap();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = bytes - 1;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let Err(CodecError::ResourceLimit(original)) = query(ctx, &[&member], &empty_sketch(), &[], &[]) else { panic!("actual state backing must refuse"); };
        assert_eq!(original.operation, "f3d historical selection state id");
        assert_eq!(original.dimension, ResourceDimension::MaterializedBytes);
        assert_eq!((original.used, original.additional), (0, bytes));
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
    policy.limits.max_materialized_bytes = bytes;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(query(ctx, &[&member], &empty_sketch(), &[], &[]).unwrap().is_none());
        let storage = ctx.reserve_scoped(bytes, "observe dropped historical state backing").unwrap();
        drop(storage);
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn historical_empty_queries_keep_original_stored_refusal() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, "original historical profile refusal") else { panic!("original refusal"); };
        assert!(matches!(query(ctx, &[], &empty_sketch(), &[], &[]), Err(CodecError::ResourceLimit(later)) if later == original));
        let feature = cadmpeg_ir::features::FeatureId::mint("synthetic:profile-membership:feature#1").unwrap();
        assert!(matches!(super::super::super::historical_face_profile_selection(&[], &[], None, &feature, &[], ctx), Err(CodecError::ResourceLimit(later)) if later == original));
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
}

fn face_binding_fixture() -> (DesignExtrudeSelectionGroup, DesignExtrudeSelectionMember, Vec<AsmHistory>, cadmpeg_ir::features::FeatureId) {
    let group = DesignExtrudeSelectionGroup::try_from(crate::records::topology::extrude_selection::DesignExtrudeSelectionGroupWire {
        id: "f3d:Design/BulkStream.dat:selection-group#9".into(), scope_record_index: 7, scope_reference_ordinal: 0, record_index: 9, byte_offset: 0, class_tag: "277".into(), member_count_offset: 32, members: vec![10], member_offsets: vec![37], opaque_index: 1, opaque_index_offset: 47, opaque_scalar: 0.0, opaque_scalar_offset: 51, variant: false, paired_class_tag: "277".into(), paired_byte_offset: 100,
    }).unwrap();
    let mut member = historical_point_member();
    member.id = "f3d:Design/BulkStream.dat:selection-member#10".into();
    member.historical = Some(HistoricalBinding { kind: AsmHistoricalEntityKind::Face, entity_ref: 10, state_ids: vec![2] });
    let histories = vec![AsmHistory { id: "f3d:Design/BulkStream.dat:history#1".into(), byte_offset: 0, preamble: None, record_table_binding_budget_exceeded: false, states: vec![transition_state(2, AsmHistoricalTopology { faces: vec![10], ..AsmHistoricalTopology::default() }, None)] }];
    let feature = cadmpeg_ir::features::FeatureId::mint("synthetic:profile-membership:feature#1").unwrap();
    (group, member, histories, feature)
}

#[test]
fn historical_profile_binding_state_preserves_original_refusal() {
    let (group, member, histories, feature) = face_binding_fixture();
    let original_member = member.clone();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let operation = "visit F3D historical profile binding state";
        let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, Some(1));
        let Err(CodecError::ResourceLimit(original)) = super::super::super::historical_face_profile_selection(&[&group], std::slice::from_ref(&member), Some(2), &feature, &histories, ctx) else { panic!("actual profile binding must refuse"); };
        assert_eq!(original.operation, operation);
        assert_eq!(original.additional, 1);
        assert_eq!(ctx.resource_refusal(), Some(original));
        assert!(matches!(ctx.charge_work(0, "later binding boundary"), Err(CodecError::ResourceLimit(later)) if later == original));
    });
    assert_eq!(member, original_member);
}

#[test]
fn historical_profile_empty_binding_preserves_unrestricted_native_face_identity() {
    let (group, mut member, histories, feature) = face_binding_fixture();
    member.historical.as_mut().unwrap().state_ids.clear();
    let original_member = member.clone();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        // Empty bindings do not execute a state lookup.
        let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "visit F3D historical profile binding state", Some(1));
        let Some(cadmpeg_ir::features::PlanarProfileRef::HistoricalFaces { state, faces, native }) = super::super::super::historical_face_profile_selection(&[&group], std::slice::from_ref(&member), Some(2), &feature, &histories, ctx).unwrap() else { panic!("empty binding remains unrestricted"); };
        assert_eq!(state, crate::ids::feature_input_topology_id(&feature, 2));
        assert_eq!(faces.as_slice(), &[crate::ids::history_input_face_id(&crate::ids::history_input_prefix(&feature.key(), 2), 10)]);
        assert_eq!(native.as_slice()[0].as_str(), group.id.as_str());
        assert_eq!(native.as_slice().len(), 1);
        assert_eq!(ctx.resource_refusal(), None);
    });
    assert_eq!(member, original_member);
}

#[test]
fn historical_profile_first_matching_state_skips_suffix_and_absent_state_rejects() {
    let (group, mut member, histories, feature) = face_binding_fixture();
    member.historical.as_mut().unwrap().state_ids.extend(std::iter::repeat_n(99, 4096));
    let mut head = member.clone();
    head.historical.as_mut().unwrap().state_ids.truncate(1);
    let mut reached = None;
    let mut selected = None;
    for input in [&head, &member] {
        crate::test_support::with_decode_context(|ctx| {
            let actual = super::super::super::historical_face_profile_selection(&[&group], std::slice::from_ref(input), Some(2), &feature, &histories, ctx).unwrap();
            assert!(actual.is_some());
            assert_eq!(ctx.resource_refusal(), None);
            let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(u64::MAX, "observe historical binding reached work") else { panic!("work observer"); };
            assert_eq!(ctx.resource_refusal(), Some(original));
            if let Some(expected) = reached {
                assert_eq!(original.used, expected);
                assert_eq!(actual, selected);
            } else {
                reached = Some(original.used);
                selected = actual;
            }
        });
    }
    member.historical.as_mut().unwrap().state_ids = vec![99];
    crate::test_support::with_decode_context(|ctx| {
        assert!(super::super::super::historical_face_profile_selection(&[&group], std::slice::from_ref(&member), Some(2), &feature, &histories, ctx).unwrap().is_none());
        assert_eq!(ctx.resource_refusal(), None);
    });
}
