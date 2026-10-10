// SPDX-License-Identifier: Apache-2.0

use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::parameters::{DesignParameter, DesignParameterOwner, DesignParameterOwnerWire};
use crate::records::topology::construction::{
    DesignConstructionOperandGroup, DesignConstructionOperandGroupDraft,
    DesignConstructionOperandGroupFrame, DesignConstructionOperandGroupFrameDraft,
    DesignConstructionOperandRole,
};
use crate::records::topology::extrude_selection::DesignOperandRole;
use crate::records::topology::fillet::{DesignFilletMidpoint, DesignFilletRadiusLaw};
use cadmpeg_core::decode::{DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const STREAM: &str = "FusionAssetName[Active]/Design1/BulkStream.dat";

struct FilletFixture {
    scopes: Vec<DesignParameterScope>,
    groups: Vec<DesignConstructionOperandGroup>,
    owners: Vec<DesignParameterOwner>,
    parameters: Vec<DesignParameter>,
}

impl FilletFixture {
    fn decode(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Vec<crate::records::topology::fillet::DesignFilletRadiusGroup>, CodecError> {
        crate::design::decode::operands::decode_fillet_radius_groups(
            ctx,
            &self.scopes,
            &self.groups,
            &self.owners,
            &self.parameters,
        )
    }
}

fn scope(record_index: u32) -> DesignParameterScope {
    DesignParameterScope::empty(
        &format!("f3d:{STREAM}:scope#{record_index}"),
        DesignFeatureKind::Fillet,
        record_index,
    )
}

fn group(
    scope_record_index: u32,
    record_index: u32,
    ordinal: u32,
) -> DesignConstructionOperandGroup {
    let frame =
        DesignConstructionOperandGroupFrame::try_from(DesignConstructionOperandGroupFrameDraft {
            member_count_offset: 1021,
            auxiliary_records: Vec::new(),
            auxiliary_paths: Vec::new(),
            trailing_records: Vec::new(),
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index: 180,
            opaque_index_offset: 1071,
            opaque_scalar: 0.125,
            opaque_scalar_offset: 1075,
            variant: false,
        })
        .expect("valid construction operand frame");
    DesignConstructionOperandGroup::try_from(DesignConstructionOperandGroupDraft {
        id: format!("f3d:{STREAM}:operand-group#{record_index}"),
        scope_record_index,
        scope_reference_ordinal: ordinal,
        record_index,
        byte_offset: 1000,
        class_tag: crate::records::references::DesignClassTag::try_from("332".to_owned())
            .expect("class tag"),
        members: Vec::new(),
        lost_edge_references: Vec::new(),
        frame,
        operand_role: DesignConstructionOperandRole::Other(DesignOperandRole::ROLE_0X5),
        role_offset: 1053,
        paired_class_tag: crate::records::references::DesignClassTag::try_from("259".to_owned())
            .expect("paired class tag"),
        paired_byte_offset: 1124,
    })
    .expect("valid construction operand group")
}

fn parameter(record_index: u32, owner_record_index: u32, source_kind: &str) -> DesignParameter {
    let mut parameter = crate::design::decode::parameters::parse_design_parameter_record(
        &crate::design::test_support::parameter_record(
            Some(owner_record_index),
            "1",
            source_kind,
            None,
            "fillet-test",
            1.0,
        ),
    )
    .expect("canonical Fillet parameter");
    parameter.id = format!("f3d:{STREAM}:parameter#{record_index}");
    parameter.record_index = record_index;
    parameter
}

fn owner(record_index: u32, scope_record_index: u32, ordinal: u32) -> DesignParameterOwner {
    let mut owner = crate::design::decode::parameters::parse_parameter_owner(
        &cadmpeg_test_support::service_decode_context(),
        &crate::design::test_support::parameter_owner_frame(),
    )
    .expect("service decode context")
    .expect("valid parameter owner")
    .into_record("Design/BulkStream.dat", 0)
    .expect("parameter owner record");
    let mut wire = DesignParameterOwnerWire::from(owner.clone());
    wire.id = format!("f3d:{STREAM}:owner#{record_index}");
    wire.record_index = record_index;
    wire.scope_record_index = scope_record_index;
    wire.local_ordinal = ordinal;
    wire.parameter_record_index = record_index + 1;
    wire.companion_record_index = record_index + 2;
    wire.evaluated_value = 1.0;
    owner = DesignParameterOwner::try_from(wire).expect("valid parameter owner record");
    owner
}

fn one_scope_fixture(kinds: &[&str]) -> FilletFixture {
    let scope_record_index = 12;
    let mut fixture = FilletFixture {
        scopes: vec![scope(scope_record_index)],
        groups: vec![group(scope_record_index, 100, 0)],
        owners: Vec::new(),
        parameters: Vec::new(),
    };
    for (ordinal, source_kind) in kinds.iter().enumerate() {
        let owner_record_index = 200 + u32::try_from(ordinal).expect("owner ordinal fits u32") * 3;
        fixture.owners.push(owner(
            owner_record_index,
            scope_record_index,
            u32::try_from(ordinal).expect("owner ordinal fits u32"),
        ));
        fixture.parameters.push(parameter(
            owner_record_index + 1,
            owner_record_index,
            source_kind,
        ));
    }
    fixture
}

fn constant_fixture(
    scope_count: usize,
    groups_per_scope: usize,
    with_weights: bool,
) -> FilletFixture {
    let mut fixture = FilletFixture {
        scopes: Vec::new(),
        groups: Vec::new(),
        owners: Vec::new(),
        parameters: Vec::new(),
    };
    let mut owner_record_index = 200u32;
    for scope_ordinal in 0..scope_count {
        let scope_record_index = 12 + u32::try_from(scope_ordinal).expect("scope ordinal fits u32");
        fixture.scopes.push(scope(scope_record_index));
        for group_ordinal in 0..groups_per_scope {
            let group_ordinal_u32 = u32::try_from(group_ordinal).expect("group ordinal fits u32");
            let group_record_index = 100
                + u32::try_from(scope_ordinal).expect("scope ordinal fits u32") * 10
                + group_ordinal_u32;
            fixture.groups.push(group(
                scope_record_index,
                group_record_index,
                group_ordinal_u32,
            ));
            let source_kinds = ["Radius", "TangencyWeight"];
            let source_kind_count = if with_weights { 2 } else { 1 };
            for source_kind in source_kinds.into_iter().take(source_kind_count) {
                let ordinal = fixture
                    .owners
                    .iter()
                    .filter(|owner| owner.scope_record_index() == scope_record_index)
                    .count();
                fixture.owners.push(owner(
                    owner_record_index,
                    scope_record_index,
                    u32::try_from(ordinal).expect("owner ordinal fits u32"),
                ));
                fixture.parameters.push(parameter(
                    owner_record_index + 1,
                    owner_record_index,
                    source_kind,
                ));
                owner_record_index += 3;
            }
        }
    }
    fixture
}

fn minimum_vec_bytes<T>() -> u64 {
    u64::try_from(
        std::mem::size_of::<T>()
            .checked_mul(4)
            .expect("four initial collection slots"),
    )
    .expect("collection bytes fit u64")
}

fn assert_refusal(
    fixture: &FilletFixture,
    dimension: ResourceDimension,
    operation: &'static str,
    additional: u64,
) {
    let error = crate::test_support::resource_refusal_at(dimension, operation, 0, |ctx| {
        fixture.decode(ctx).map(|_| ())
    });
    assert!(
        matches!(
            error,
            CodecError::ResourceLimit(failure)
                if failure.dimension == dimension
                    && failure.operation == operation
                    && failure.additional == additional
        ),
        "{operation}: {error:?}"
    );
}

fn admitted(count: usize) -> u64 {
    u64::try_from(count).expect("admitted count fits u64")
}

#[test]
fn fillet_scoped_projection_collectors_refuse_at_each_exact_boundary() {
    let constant = constant_fixture(1, 2, true);
    let output = crate::test_support::with_decode_context(|ctx| constant.decode(ctx))
        .expect("valid constant Fillet groups");
    assert_eq!(output.len(), 2);
    assert!(output
        .iter()
        .all(|group| matches!(&group.law, DesignFilletRadiusLaw::Constant { .. })));

    let chordal = one_scope_fixture(&["ChordLen"]);
    let output = crate::test_support::with_decode_context(|ctx| chordal.decode(ctx))
        .expect("valid chordal Fillet group");
    assert!(matches!(
        output.as_slice(),
        [group] if matches!(&group.law, DesignFilletRadiusLaw::Chordal { .. })
    ));

    let asymmetric = one_scope_fixture(&["EdgeOffset1", "EdgeOffset2", "TangencyWeight"]);
    let output = crate::test_support::with_decode_context(|ctx| asymmetric.decode(ctx))
        .expect("valid asymmetric Fillet group");
    assert!(matches!(
        output.as_slice(),
        [group] if matches!(&group.law, DesignFilletRadiusLaw::Asymmetric { .. })
    ));

    let variable = one_scope_fixture(&["StartRadius", "EndRadius", "MidRadius", "MidParams"]);
    let output = crate::test_support::with_decode_context(|ctx| variable.decode(ctx))
        .expect("valid variable Fillet group");
    assert!(matches!(
        output.as_slice(),
        [group] if matches!(
            &group.law,
                DesignFilletRadiusLaw::Variable { middle, .. }
                if middle.len() == 1
                    && middle[0]
                        == (DesignFilletMidpoint {
                            radius_parameter_record_index: 207,
                            parameter_record_index: 210,
                        })
        )
    ));
    let endpoint_only = one_scope_fixture(&["StartRadius", "EndRadius"]);
    let output = crate::test_support::with_decode_context(|ctx| endpoint_only.decode(ctx))
        .expect("valid endpoint-only variable Fillet group");
    assert!(matches!(output.as_slice(), [group]
        if matches!(&group.law, DesignFilletRadiusLaw::Variable { middle, .. }
            if middle.is_empty())));

    // The scope's groups are copied into exact scoped storage.
    let scope_groups = constant.groups.len();
    assert_refusal(
        &constant,
        ResourceDimension::WorkUnits,
        "f3d Fillet scope groups",
        admitted(scope_groups),
    );
    assert_refusal(
        &constant,
        ResourceDimension::CollectionItems,
        "f3d Fillet scope groups",
        admitted(scope_groups),
    );
    assert_refusal(
        &constant,
        ResourceDimension::MaterializedBytes,
        "f3d Fillet scope groups",
        admitted(scope_groups * std::mem::size_of::<&DesignConstructionOperandGroup>()),
    );
    // Owned parameters and their partition grow scoped storage one slot at a time.
    assert_refusal(
        &constant,
        ResourceDimension::CollectionItems,
        "f3d Fillet owned parameters",
        1,
    );
    assert_refusal(
        &constant,
        ResourceDimension::MaterializedBytes,
        "f3d Fillet owned parameters",
        minimum_vec_bytes::<(u32, &DesignParameter)>(),
    );
    assert_refusal(
        &constant,
        ResourceDimension::CollectionItems,
        "f3d Fillet parameters by kind",
        1,
    );
    assert_refusal(
        &constant,
        ResourceDimension::MaterializedBytes,
        "f3d Fillet parameters by kind",
        minimum_vec_bytes::<&DesignParameter>(),
    );
    // Midpoints are retained in the output law.
    assert_refusal(
        &variable,
        ResourceDimension::CollectionItems,
        "f3d Fillet middle parameters",
        1,
    );
    assert_refusal(
        &variable,
        ResourceDimension::RetainedBytes,
        "f3d Fillet middle parameters",
        minimum_vec_bytes::<DesignFilletMidpoint>(),
    );

    for (fixture, operation, additional) in [
        (
            &constant,
            "index F3D Fillet parameters",
            constant.parameters.len(),
        ),
        (&constant, "scan F3D Fillet scopes", constant.scopes.len()),
        (
            &constant,
            "index F3D construction operand groups by scope",
            constant.groups.len(),
        ),
        (
            &constant,
            "index F3D Fillet owners by scope",
            constant.owners.len(),
        ),
        (&constant, "scan F3D Fillet owners", constant.owners.len()),
        (
            &constant,
            "classify F3D Fillet parameters",
            constant.owners.len(),
        ),
        (&constant, "pair F3D Fillet groups", 2),
    ] {
        assert_refusal(
            fixture,
            ResourceDimension::WorkUnits,
            operation,
            admitted(additional),
        );
    }
}

/// The least materialized-byte ceiling under which `fixture` decodes.
fn materialized_peak(fixture: &FilletFixture) -> u64 {
    let decodes = |ceiling: u64| {
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = ceiling;
        crate::test_support::with_decode_policy(&policy, |ctx| fixture.decode(ctx)).is_ok()
    };
    let (mut lower, mut upper) = (0_u64, 1 << 20);
    assert!(decodes(upper), "fixture decodes within the search ceiling");
    while lower < upper {
        let middle = lower + (upper - lower) / 2;
        if decodes(middle) {
            upper = middle;
        } else {
            lower = middle + 1;
        }
    }
    upper
}

#[test]
fn fillet_materialized_projection_storage_is_released_between_scopes() {
    let fixture = constant_fixture(2, 2, false);
    let output = crate::test_support::with_decode_context(|ctx| fixture.decode(ctx))
        .expect("valid constant Fillet groups");
    assert_eq!(
        output
            .iter()
            .map(|group| group.scope_record_index)
            .collect::<Vec<_>>(),
        [12, 12, 13, 13]
    );
    // The same indexes with one Fillet scope: the second scope is a Chamfer,
    // which builds no projections.
    let mut single_projection = constant_fixture(2, 2, false);
    single_projection.scopes[1] = DesignParameterScope::empty(
        &format!("f3d:{STREAM}:scope#13"),
        DesignFeatureKind::Chamfer,
        13,
    );
    assert_eq!(
        materialized_peak(&fixture),
        materialized_peak(&single_projection),
        "one scope's projections are released before the next scope builds its own"
    );
}
