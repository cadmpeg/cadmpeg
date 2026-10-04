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

fn assert_collector_boundaries<T>(
    fixture: &FilletFixture,
    operation: &'static str,
    materialized_dimension: ResourceDimension,
    storage_fixture: &FilletFixture,
) {
    for (dimension, additional) in [
        (ResourceDimension::WorkUnits, 1),
        (ResourceDimension::CollectionItems, 1),
        (materialized_dimension, minimum_vec_bytes::<T>()),
    ] {
        let selected = if dimension == materialized_dimension {
            storage_fixture
        } else {
            fixture
        };
        let error = crate::test_support::resource_refusal_at(dimension, operation, 0, |ctx| {
            selected.decode(ctx).map(|_| ())
        });
        assert!(matches!(
            error,
            CodecError::ResourceLimit(failure)
                if failure.dimension == dimension
                    && failure.operation == operation
                    && failure.additional == additional
        ));
    }
}

fn assert_work_admission(fixture: &FilletFixture, operation: &'static str, additional: usize) {
    let expected = u64::try_from(additional).expect("admitted source length fits u64");
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| fixture.decode(ctx).map(|_| ()),
    );
    assert!(matches!(
        error,
        CodecError::ResourceLimit(failure)
            if failure.dimension == ResourceDimension::WorkUnits
                && failure.operation == operation
                && failure.additional == expected
    ));
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

    // Two indexed parameters avoid a hash-table rehash before the projections.
    let storage_constant = constant_fixture(1, 1, true);
    let storage_output =
        crate::test_support::with_decode_context(|ctx| storage_constant.decode(ctx))
            .expect("valid single constant Fillet group");
    assert!(matches!(storage_output.as_slice(), [group]
        if matches!(&group.law, DesignFilletRadiusLaw::Constant { .. })));

    assert_collector_boundaries::<&DesignConstructionOperandGroup>(
        &constant,
        "f3d Fillet scope groups",
        ResourceDimension::MaterializedBytes,
        &storage_constant,
    );
    assert_collector_boundaries::<(u32, &DesignParameter)>(
        &constant,
        "f3d Fillet owned parameters",
        ResourceDimension::MaterializedBytes,
        &storage_constant,
    );
    assert_collector_boundaries::<&DesignParameter>(
        &constant,
        "f3d Fillet radius parameters",
        ResourceDimension::MaterializedBytes,
        &storage_constant,
    );
    assert_collector_boundaries::<&DesignParameter>(
        &constant,
        "f3d Fillet weight parameters",
        ResourceDimension::MaterializedBytes,
        &storage_constant,
    );

    let chordal = one_scope_fixture(&["ChordLen"]);
    let output = crate::test_support::with_decode_context(|ctx| chordal.decode(ctx))
        .expect("valid chordal Fillet group");
    assert!(matches!(
        output.as_slice(),
        [group] if matches!(&group.law, DesignFilletRadiusLaw::Chordal { .. })
    ));
    assert_collector_boundaries::<u32>(
        &chordal,
        "f3d Fillet chord lengths",
        ResourceDimension::MaterializedBytes,
        &chordal,
    );

    let asymmetric = one_scope_fixture(&["EdgeOffset1", "EdgeOffset2", "TangencyWeight"]);
    let output = crate::test_support::with_decode_context(|ctx| asymmetric.decode(ctx))
        .expect("valid asymmetric Fillet group");
    assert!(matches!(
        output.as_slice(),
        [group] if matches!(&group.law, DesignFilletRadiusLaw::Asymmetric { .. })
    ));
    assert_collector_boundaries::<u32>(
        &asymmetric,
        "f3d Fillet asymmetric offsets",
        ResourceDimension::MaterializedBytes,
        &asymmetric,
    );

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
    let storage_variable = one_scope_fixture(&["StartRadius", "EndRadius"]);
    let storage_output =
        crate::test_support::with_decode_context(|ctx| storage_variable.decode(ctx))
            .expect("valid endpoint-only variable Fillet group");
    assert!(matches!(storage_output.as_slice(), [group]
        if matches!(&group.law, DesignFilletRadiusLaw::Variable { middle, .. }
            if middle.is_empty())));
    assert_collector_boundaries::<u32>(
        &variable,
        "f3d Fillet variable parameters",
        ResourceDimension::MaterializedBytes,
        &storage_variable,
    );
    assert_collector_boundaries::<DesignFilletMidpoint>(
        &variable,
        "f3d Fillet middle parameters",
        ResourceDimension::RetainedBytes,
        &variable,
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
            "scan F3D Fillet scope groups",
            constant.groups.len(),
        ),
        (&constant, "scan F3D Fillet owners", constant.owners.len()),
        (
            &constant,
            "scan F3D Fillet radius parameters",
            constant.owners.len(),
        ),
        (
            &constant,
            "scan F3D Fillet weight parameters",
            constant.owners.len(),
        ),
        (&constant, "pair F3D Fillet groups", 2),
        (&constant, "pair F3D Fillet radius parameters", 2),
        (&chordal, "scan F3D Fillet chord-length parameters", 1),
        (&asymmetric, "scan F3D Fillet asymmetric parameters", 3),
        (&variable, "scan F3D Fillet variable parameters", 4),
        (&variable, "scan F3D Fillet midpoint radii", 1),
        (&variable, "scan F3D Fillet midpoint parameter indices", 1),
    ] {
        assert_work_admission(fixture, operation, additional);
    }
}

#[test]
fn fillet_materialized_projection_storage_is_released_between_scopes() {
    let fixture = constant_fixture(2, 2, false);
    let peak_bytes = minimum_vec_bytes::<&DesignConstructionOperandGroup>()
        .checked_add(minimum_vec_bytes::<(u32, &DesignParameter)>())
        .and_then(|bytes| bytes.checked_add(minimum_vec_bytes::<&DesignParameter>()))
        .expect("one Fillet scope projection peak fits u64");
    let mut policy = DecodePolicy::service();
    // The four-entry index grows from four buckets. Its old allocation includes
    // alignment padding, four control bytes, and the sixteen-byte control tail.
    let index_overlap = u64::try_from(
        4 * std::mem::size_of::<((&str, u32), &DesignParameter)>()
            + std::mem::align_of::<((&str, u32), &DesignParameter)>().max(16)
            - 1
            + 4
            + 16,
    )
    .expect("index overlap bytes fit u64");
    let stream_bytes = u64::try_from(
        crate::ids::native_stream(&fixture.scopes[0].id)
            .expect("native stream")
            .len(),
    )
    .expect("stream bytes fit u64");
    // Output ID growth overlaps the live projections with the copied stream.
    let output_peak = peak_bytes
        .checked_add(stream_bytes)
        .expect("output peak fits u64");
    policy.limits.max_materialized_bytes = output_peak.max(index_overlap);
    let output = crate::test_support::with_decode_policy(&policy, |ctx| fixture.decode(ctx))
        .expect("scoped projection receipts release before the next scope");
    assert_eq!(output.len(), 4);
    assert_eq!(
        output
            .iter()
            .map(|group| group.scope_record_index)
            .collect::<Vec<_>>(),
        [12, 12, 13, 13]
    );
}
