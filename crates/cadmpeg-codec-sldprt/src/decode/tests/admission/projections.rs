// SPDX-License-Identifier: Apache-2.0
//! Decode projection resource admission tests.

use crate::test_support::container::make_block;
use crate::test_support::container::outer_header;
use crate::test_support::parasolid::triangle_body;
use crate::SldprtCodec;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use std::io::Cursor;

use super::{
    collection_refusal_at, collection_refusal_with_options, retained_refusal_at,
    work_refusal_with_options, work_refusal_with_request,
};

fn regeneration_parent_source() -> Vec<u8> {
    let mut source = outer_header();
    source.extend(make_block(
        0x43,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Outer" Type="Custom" id="1"><Feature Name="Nested" Type="Custom" id="2"/></Feature></Keywords>"#,
    ));
    source
}

#[test]
fn metadata_regeneration_parent_refuses_collection_limit() {
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let refusal = collection_refusal_with_options(
        &regeneration_parent_source(),
        options,
        "install decoded feature regeneration parent",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::CollectionItems
    );
    assert_eq!(refusal.additional, 1);
}

#[test]
fn metadata_regeneration_parent_refuses_retained_limit() {
    let mut options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(
        &regeneration_parent_source(),
        &mut options,
        "install decoded feature regeneration parent",
    );
    assert!(matches!(error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                && limit.operation == "install decoded feature regeneration parent"
    ));
}

#[test]
fn metadata_regeneration_parent_refuses_work_limit() {
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let refusal = work_refusal_with_request(
        &regeneration_parent_source(),
        options,
        "install decoded feature regeneration parent",
        Some(1), // One visitor work unit; key reads have separate requests.
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
    assert_eq!(refusal.additional, 1);
}

fn composite_curve_source() -> Vec<u8> {
    let mut source = outer_header();
    source.extend(make_block(
        0x43,
        "Contents/Keywords",
        br#"<Keywords><CompositeCurve Name="Composite" id="10" Segments="curve-a;curve-b"/></Keywords>"#,
    ));
    source
}

#[test]
fn metadata_curve_projection_refuses_collection_limit() {
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let refusal = collection_refusal_with_options(
        &composite_curve_source(),
        options,
        "project SLDPRT composite curve segments",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::CollectionItems
    );
    assert_eq!(refusal.additional, 1);
}

#[test]
fn metadata_curve_projection_refuses_retained_limit() {
    let mut options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(
        &composite_curve_source(),
        &mut options,
        "retain SLDPRT datum and curve reference",
    );
    assert!(matches!(error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                && limit.operation == "retain SLDPRT datum and curve reference"
    ));
}

#[test]
fn metadata_curve_projection_refuses_work_limit() {
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let refusal = work_refusal_with_request(
        &composite_curve_source(),
        options,
        "project SLDPRT composite curve segments",
        Some(1), // One visitor work unit; key reads have separate requests.
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
    assert_eq!(refusal.additional, 1);
}

fn variable_fillet_source() -> Vec<u8> {
    let mut source = outer_header();
    source.extend(make_block(
        0x43,
        "Contents/Keywords",
        br#"<Keywords><Fillet Name="Variable" id="10" Edges="edge-a"><Dimension Name="Radius0">2mm</Dimension><Dimension Name="Position0">0</Dimension><Dimension Name="Radius1">3mm</Dimension><Dimension Name="Position1">1</Dimension></Fillet></Keywords>"#,
    ));
    source
}

#[test]
fn metadata_edit_projection_refuses_collection_limit() {
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let refusal = collection_refusal_with_options(
        &variable_fillet_source(),
        options,
        "collect SLDPRT variable fillet radii",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::CollectionItems
    );
    assert_eq!(refusal.additional, 1);
}

#[test]
fn metadata_edit_projection_refuses_retained_limit() {
    let mut options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(
        &variable_fillet_source(),
        &mut options,
        "retain SLDPRT edit selection reference",
    );
    assert!(matches!(error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                && limit.operation == "retain SLDPRT edit selection reference"
    ));
}

#[test]
fn metadata_edit_projection_refuses_work_limit() {
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let refusal = work_refusal_with_request(
        &variable_fillet_source(),
        options,
        "scan SLDPRT variable fillet radii",
        Some(1), // One visitor work unit; key reads have separate requests.
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
    assert_eq!(refusal.additional, 1);
}

fn native_definition_source() -> Vec<u8> {
    let mut source = outer_header();
    source.extend(make_block(
        0x43,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Custom" Type="Custom" id="10"><Dimension Name="Length">1mm</Dimension></Feature></Keywords>"#,
    ));
    source
}

#[test]
fn metadata_native_definition_refuses_collection_limit() {
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let refusal = collection_refusal_with_options(
        &native_definition_source(),
        options,
        "collect SLDPRT native definition parameters",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::CollectionItems
    );
    assert_eq!(refusal.additional, 1);
}

#[test]
fn metadata_native_definition_refuses_retained_limit() {
    let mut options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(
        &native_definition_source(),
        &mut options,
        "retain SLDPRT native definition kind",
    );
    assert!(matches!(error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                && limit.operation == "retain SLDPRT native definition kind"
    ));
}

#[test]
fn metadata_native_definition_refuses_work_limit() {
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let refusal = work_refusal_with_request(
        &native_definition_source(),
        options,
        "collect SLDPRT native definition parameters",
        Some(1), // One visitor work unit; key reads have separate requests.
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
    assert_eq!(refusal.additional, 1);
}

#[test]
fn metadata_surface_projection_refuses_retained_limit() {
    let mut source = outer_header();
    source.extend(make_block(
        0x43,
        "Contents/Keywords",
        br#"<Keywords><TrimSurface Name="Trim" id="10"/></Keywords>"#,
    ));
    let mut options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(&source, &mut options, "retain SLDPRT trim surface tool");
    assert!(matches!(error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                && limit.operation == "retain SLDPRT trim surface tool"
    ));
}

fn loft_reference_source() -> Vec<u8> {
    let mut source = outer_header();
    source.extend(make_block(
        0x43,
        "Contents/Keywords",
        br#"<Keywords><Loft Name="Loft" id="10" Profiles="a,b" Guides="c"/></Keywords>"#,
    ));
    source
}

#[test]
fn metadata_loft_projection_refuses_collection_limit() {
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let refusal = collection_refusal_with_options(
        &loft_reference_source(),
        options,
        "project SLDPRT loft references",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::CollectionItems
    );
    assert_eq!(refusal.additional, 1);
}

#[test]
fn metadata_loft_projection_refuses_retained_limit() {
    let mut options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(
        &loft_reference_source(),
        &mut options,
        "retain SLDPRT loft reference",
    );
    assert!(matches!(error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                && limit.operation == "retain SLDPRT loft reference"
    ));
}

#[test]
fn metadata_loft_projection_refuses_work_limit() {
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let refusal = work_refusal_with_options(
        &loft_reference_source(),
        options,
        "project SLDPRT loft references",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
    assert_eq!(refusal.additional, 3);
}

#[test]
fn metadata_hole_projection_refuses_retained_limit() {
    let mut source = outer_header();
    source.extend(make_block(
        0x43, "Contents/Keywords",
        br#"<Keywords><Hole Name="Hole" id="10" Face="face-a"><Dimension Name="Diameter">4mm</Dimension><Dimension Name="Depth">9mm</Dimension></Hole></Keywords>"#,
    ));
    let mut options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(&source, &mut options, "retain SLDPRT hole face reference");
    assert!(matches!(error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                && limit.operation == "retain SLDPRT hole face reference"
    ));
}

fn construction_reference_source() -> Vec<u8> {
    let mut source = outer_header();
    source.extend(make_block(
        0x43, "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch" id="1"/><Extrusion Name="Boss" id="2" Profile="1"><Dimension Name="Depth">1mm</Dimension></Extrusion></Keywords>"#,
    ));
    source
}

#[test]
fn metadata_construction_binding_refuses_collection_limit() {
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let refusal = collection_refusal_with_options(
        &construction_reference_source(),
        options,
        "index SLDPRT native construction features",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::CollectionItems
    );
    assert_eq!(refusal.additional, 1);
}

#[test]
fn metadata_construction_binding_refuses_work_limit() {
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let refusal = work_refusal_with_request(
        &construction_reference_source(),
        options,
        "index SLDPRT native construction sources",
        Some(2), // Two feature slots in the admitted construction-source iteration.
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
    assert_eq!(refusal.additional, 2);
}

#[test]
fn metadata_offset_plane_binding_refuses_collection_limit() {
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let refusal = collection_refusal_with_options(
        &construction_reference_source(),
        options,
        "index SLDPRT offset plane ordinals",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::CollectionItems
    );
    assert_eq!(refusal.additional, 1);
}

#[test]
fn metadata_offset_plane_binding_refuses_work_limit() {
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let refusal = work_refusal_with_request(
        &construction_reference_source(),
        options,
        "index SLDPRT offset plane ordinals",
        Some(2), // Two feature slots in the admitted plane-fact iteration.
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
    assert_eq!(refusal.additional, 2);
}

#[test]
fn metadata_parameter_projection_refuses_collection_limit() {
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let refusal = collection_refusal_with_options(
        &native_definition_source(),
        options,
        "collect SLDPRT projected parameters",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::CollectionItems
    );
    assert_eq!(refusal.additional, 1);
}

#[test]
fn metadata_parameter_projection_refuses_retained_limit() {
    let mut options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(
        &native_definition_source(),
        &mut options,
        "retain SLDPRT parameter expression",
    );
    assert!(matches!(error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                && limit.operation == "retain SLDPRT parameter expression"
    ));
}

#[test]
fn metadata_parameter_projection_refuses_work_limit() {
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let refusal = work_refusal_with_request(
        &native_definition_source(),
        options,
        "scan SLDPRT project_parameters values",
        Some(1), // One admitted history or feature slot in the parameter projection.
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
    assert_eq!(refusal.additional, 1);
}

#[test]
fn metadata_parameter_ordering_refuses_collection_limit() {
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let refusal = collection_refusal_with_options(
        &native_definition_source(),
        options,
        "order SLDPRT parameter dependencies",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::CollectionItems
    );
    assert_eq!(refusal.additional, 1);
}

#[test]
fn metadata_parameter_ordering_refuses_work_limit() {
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let refusal = work_refusal_with_request(
        &native_definition_source(),
        options,
        "order SLDPRT parameter dependencies",
        Some(1), // One visitor work unit; key reads have separate requests.
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
    assert_eq!(refusal.additional, 1);
}

#[test]
fn metadata_parameter_aliases_refuse_collection_limit() {
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let refusal = collection_refusal_with_options(
        &native_definition_source(),
        options,
        "index SLDPRT parameter aliases",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::CollectionItems
    );
    assert_eq!(refusal.additional, 1);
}

#[test]
fn metadata_parameter_aliases_refuse_retained_limit() {
    let mut options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(
        &native_definition_source(),
        &mut options,
        "retain SLDPRT parameter alias",
    );
    assert!(matches!(error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                && limit.operation == "retain SLDPRT parameter alias"
    ));
}

#[test]
fn metadata_parameter_aliases_refuse_work_limit() {
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let refusal = work_refusal_with_request(
        &native_definition_source(),
        options,
        "scan SLDPRT parameter aliases",
        Some(1), // One visitor work unit; key reads have separate requests.
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
    assert_eq!(refusal.additional, 1);
}

#[test]
fn metadata_parameter_value_states_refuse_collection_limit() {
    let options = DecodeOptions::default();
    let refusal = collection_refusal_with_options(
        &native_definition_source(),
        options,
        "collect SLDPRT parameter value states",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::CollectionItems
    );
    assert_eq!(refusal.additional, 1);
}

#[test]
fn metadata_parameter_value_states_refuse_work_limit() {
    let options = DecodeOptions::default();
    let refusal = work_refusal_with_request(
        &native_definition_source(),
        options,
        "collect SLDPRT parameter value state",
        Some(1), // One visitor work unit; key reads have separate requests.
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
    assert_eq!(refusal.additional, 1);
}

#[test]
fn metadata_parameter_value_states_refuse_retained_limit() {
    let mut source = outer_header();
    source.extend(make_block(
        0x43, "Contents/Keywords",
        br#"<Keywords><Feature Name="Custom" Type="Custom" id="10"><Dimension Name="Note">plain text</Dimension></Feature></Keywords>"#,
    ));
    let mut options = DecodeOptions::default();
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(&source, &mut options, "retain SLDPRT parameter value text");
    assert!(matches!(error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                && limit.operation == "retain SLDPRT parameter value text"
    ));
}

#[test]
fn metadata_parameter_dependencies_refuse_collection_limit() {
    let mut source = outer_header();
    source.extend(make_block(
        0x43, "Contents/Keywords",
        br#"<Keywords><Feature Name="Equations" Type="EquationDriven" id="10"><Dimension Name="A">1</Dimension><Dimension Name="B">A + 1</Dimension></Feature></Keywords>"#,
    ));
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let refusal =
        collection_refusal_with_options(&source, options, "collect SLDPRT parameter dependencies");
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::CollectionItems
    );
    assert_eq!(refusal.additional, 1);
}

#[test]
fn metadata_parameter_expression_refuses_nesting_limit() {
    let mut source = outer_header();
    source.extend(make_block(
        0x43, "Contents/Keywords",
        br#"<Keywords><Feature Name="Equations" Type="EquationDriven" id="10"><Dimension Name="A">+ + + + + + + + + + + + 1</Dimension></Feature></Keywords>"#,
    ));
    let mut options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    options.policy.limits.max_recursion_depth = 8;
    let error = SldprtCodec
        .decode(&mut Cursor::new(&source), &options)
        .expect_err("recursive parameter expression exceeds the nesting limit");
    assert!(matches!(error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RecursionDepth
                && limit.operation == "parse SLDPRT parameter unary"
    ));
}

fn class_binding_source() -> Vec<u8> {
    let mut source = crate::test_support::history::sldprt_with_body_and_resolved_features(
        &triangle_body(),
        &[0, 1],
    );
    source.extend(make_block(
        0x43,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="Sketch" id="10"/></Keywords>"#,
    ));
    source
}

fn class_binding_scoped_refusal(options: DecodeOptions) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let source = class_binding_source();
    // Decode setup has its own scratch peak; the fresh caller context isolates this index.
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(&source), &options)
        .unwrap();
    let native = crate::test_support::native::sldprt_native(decoded.ir());
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "index SLDPRT input class names",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&source, &arena, &policy)?;
            let mut histories = native.feature_histories.clone();
            crate::resolved_features::classes::bind_history_classes(
                &ctx,
                &mut histories,
                &native.feature_input_lanes,
            )
        },
    )
}

#[test]
fn metadata_class_binding_refuses_collection_limit() {
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let refusal = collection_refusal_with_options(
        &class_binding_source(),
        options,
        "index SLDPRT input class names",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::CollectionItems
    );
}

#[test]
fn geometry_class_binding_refuses_collection_limit() {
    let refusal = collection_refusal_at(&class_binding_source(), "index SLDPRT input class names");
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::CollectionItems
    );
}

#[test]
fn metadata_class_binding_refuses_scoped_limit() {
    let error = class_binding_scoped_refusal(DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes
            && limit.operation == "index SLDPRT input class names")
    );
}

#[test]
fn geometry_class_binding_refuses_scoped_limit() {
    let error = class_binding_scoped_refusal(DecodeOptions::default());
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes
            && limit.operation == "index SLDPRT input class names")
    );
}

#[test]
fn metadata_class_binding_refuses_work_limit() {
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let refusal = work_refusal_with_options(
        &class_binding_source(),
        options,
        "bind SLDPRT history classes",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
}

#[test]
fn geometry_class_binding_refuses_work_limit() {
    let refusal = work_refusal_with_options(
        &class_binding_source(),
        DecodeOptions::default(),
        "bind SLDPRT history classes",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
}

#[test]
fn metadata_scalar_binding_refuses_collection_limit() {
    let refusal = collection_refusal_with_options(
        &class_binding_source(),
        DecodeOptions {
            container_only: true,
            ..DecodeOptions::default()
        },
        "collect SLDPRT scalar binding candidates",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::CollectionItems
    );
}

#[test]
fn metadata_scalar_binding_refuses_work_limit() {
    let refusal = work_refusal_with_options(
        &class_binding_source(),
        DecodeOptions {
            container_only: true,
            ..DecodeOptions::default()
        },
        "scan SLDPRT scalar binding candidates",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
}

#[test]
fn metadata_scalar_binding_refuses_retained_limit() {
    let mut options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(
        &class_binding_source(),
        &mut options,
        "retain SLDPRT scalar binding identity",
    );
    assert!(
        matches!(error, cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
            && limit.operation == "retain SLDPRT scalar binding identity")
    );
}

#[test]
fn geometry_scalar_binding_refuses_collection_limit() {
    let refusal = collection_refusal_with_options(
        &class_binding_source(),
        DecodeOptions::default(),
        "collect SLDPRT scalar binding candidates",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::CollectionItems
    );
}

#[test]
fn geometry_scalar_binding_refuses_work_limit() {
    let refusal = work_refusal_with_options(
        &class_binding_source(),
        DecodeOptions::default(),
        "scan SLDPRT scalar binding candidates",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
}

#[test]
fn geometry_scalar_binding_refuses_retained_limit() {
    let mut options = DecodeOptions::default();
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(
        &class_binding_source(),
        &mut options,
        "retain SLDPRT scalar binding identity",
    );
    assert!(
        matches!(error, cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
            && limit.operation == "retain SLDPRT scalar binding identity")
    );
}

#[test]
fn metadata_adjacent_profiles_refuses_collection_limit() {
    let refusal = collection_refusal_with_options(
        &class_binding_source(),
        DecodeOptions {
            container_only: true,
            ..DecodeOptions::default()
        },
        "collect SLDPRT adjacent profile objects",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::CollectionItems
    );
}

#[test]
fn metadata_adjacent_profiles_refuses_index_collection_limit() {
    let limit = collection_refusal_with_options(
        &class_binding_source(),
        DecodeOptions {
            container_only: true,
            ..DecodeOptions::default()
        },
        "index SLDPRT adjacent profiles",
    );
    assert_eq!(limit.operation, "index SLDPRT adjacent profiles");
}

#[test]
fn metadata_adjacent_profiles_refuses_work_limit() {
    let refusal = work_refusal_with_options(
        &class_binding_source(),
        DecodeOptions {
            container_only: true,
            ..DecodeOptions::default()
        },
        "scan SLDPRT adjacent profile objects",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
}

#[test]
fn geometry_adjacent_profiles_refuses_collection_limit() {
    let refusal = collection_refusal_with_options(
        &class_binding_source(),
        DecodeOptions::default(),
        "collect SLDPRT adjacent profile objects",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::CollectionItems
    );
}

#[test]
fn geometry_adjacent_profiles_refuses_index_collection_limit() {
    let limit = collection_refusal_with_options(
        &class_binding_source(),
        DecodeOptions::default(),
        "index SLDPRT adjacent profiles",
    );
    assert_eq!(limit.operation, "index SLDPRT adjacent profiles");
}

#[test]
fn geometry_adjacent_profiles_refuses_work_limit() {
    let refusal = work_refusal_with_options(
        &class_binding_source(),
        DecodeOptions::default(),
        "scan SLDPRT adjacent profile objects",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
}

#[test]
fn metadata_dissected_sketches_refuses_collection_limit() {
    let refusal = collection_refusal_with_options(
        &class_binding_source(),
        DecodeOptions {
            container_only: true,
            ..DecodeOptions::default()
        },
        "index SLDPRT dissected profiles",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::CollectionItems
    );
}

#[test]
fn metadata_dissected_sketches_refuses_work_limit() {
    let refusal = work_refusal_with_options(
        &class_binding_source(),
        DecodeOptions {
            container_only: true,
            ..DecodeOptions::default()
        },
        "classify SLDPRT dissected profiles",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
}

#[test]
fn geometry_dissected_sketches_refuses_collection_limit() {
    let refusal = collection_refusal_with_options(
        &class_binding_source(),
        DecodeOptions::default(),
        "index SLDPRT dissected profiles",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::CollectionItems
    );
}

#[test]
fn geometry_dissected_sketches_refuses_work_limit() {
    let refusal = work_refusal_with_options(
        &class_binding_source(),
        DecodeOptions::default(),
        "classify SLDPRT dissected profiles",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
}

#[test]
fn metadata_sweep_adjacent_profiles_refuses_collection_limit() {
    let refusal = collection_refusal_with_options(
        &class_binding_source(),
        DecodeOptions {
            container_only: true,
            ..DecodeOptions::default()
        },
        "collect SLDPRT feature binding candidates",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::CollectionItems
    );
}

#[test]
fn metadata_sweep_adjacent_profiles_refuses_work_limit() {
    let refusal = work_refusal_with_options(
        &class_binding_source(),
        DecodeOptions {
            container_only: true,
            ..DecodeOptions::default()
        },
        "scan SLDPRT feature binding candidates",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
}

#[test]
fn geometry_sweep_adjacent_profiles_refuses_collection_limit() {
    let refusal = collection_refusal_with_options(
        &class_binding_source(),
        DecodeOptions::default(),
        "collect SLDPRT feature binding candidates",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::CollectionItems
    );
}

#[test]
fn geometry_sweep_adjacent_profiles_refuses_work_limit() {
    let refusal = work_refusal_with_options(
        &class_binding_source(),
        DecodeOptions::default(),
        "scan SLDPRT feature binding candidates",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
}

#[test]
fn geometry_mirror_surface_planes_refuses_collection_limit() {
    let refusal = collection_refusal_with_options(
        &class_binding_source(),
        DecodeOptions::default(),
        "index SLDPRT mirror surface planes",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::CollectionItems
    );
}

#[test]
fn geometry_mirror_surface_planes_refuses_work_limit() {
    let refusal = work_refusal_with_options(
        &class_binding_source(),
        DecodeOptions::default(),
        "index SLDPRT mirror surface planes",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
}

fn sweep_binding_source() -> Vec<u8> {
    let mut source = crate::test_support::container::sldprt_with_body(&triangle_body());
    let mut payload = crate::test_support::history::resolved_feature_classes_with_ids(&[
        ("moSweep_c", "Sweep1", 20),
        ("moProfileFeature_c", "Sketch1", 10),
    ]);
    payload.extend(crate::test_support::parasolid::parasolid_with_body(
        "profile",
        "SCH_SW_33103_11000",
        &triangle_body(),
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &payload,
    ));
    source.extend(make_block(0x43, "Contents/Keywords",
        br#"<Keywords><Sweep Name="Sweep1" Type="Sweep" id="20"/><Sketch Name="Sketch1" Type="Sketch" id="10"/></Keywords>"#));
    source
}

#[test]
fn metadata_sweep_adjacent_profiles_refuses_retained_limit() {
    let mut options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(
        &sweep_binding_source(),
        &mut options,
        "retain SLDPRT feature binding identity",
    );
    assert!(
        matches!(error, cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
            && limit.operation == "retain SLDPRT feature binding identity")
    );
}

#[test]
fn geometry_sweep_adjacent_profiles_refuses_retained_limit() {
    let mut options = DecodeOptions::default();
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(
        &sweep_binding_source(),
        &mut options,
        "retain SLDPRT feature binding identity",
    );
    assert!(
        matches!(error, cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
            && limit.operation == "retain SLDPRT feature binding identity")
    );
}

#[test]
fn metadata_pattern_inputs_refuses_collection_limit() {
    let refusal = collection_refusal_with_options(
        &class_binding_source(),
        DecodeOptions {
            container_only: true,
            ..DecodeOptions::default()
        },
        "collect SLDPRT pattern input candidates",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::CollectionItems
    );
}

#[test]
fn metadata_pattern_inputs_refuses_work_limit() {
    let refusal = work_refusal_with_options(
        &class_binding_source(),
        DecodeOptions {
            container_only: true,
            ..DecodeOptions::default()
        },
        "scan SLDPRT pattern input candidates",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
}

#[test]
fn metadata_pattern_inputs_refuses_retained_limit() {
    let mut options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(
        &class_binding_source(),
        &mut options,
        "retain SLDPRT pattern native identity",
    );
    assert!(
        matches!(error, cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
            && limit.operation == "retain SLDPRT pattern native identity")
    );
}

#[test]
fn geometry_pattern_inputs_refuses_collection_limit() {
    let refusal = collection_refusal_with_options(
        &class_binding_source(),
        DecodeOptions::default(),
        "collect SLDPRT pattern input candidates",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::CollectionItems
    );
}

#[test]
fn geometry_pattern_inputs_refuses_work_limit() {
    let refusal = work_refusal_with_options(
        &class_binding_source(),
        DecodeOptions::default(),
        "scan SLDPRT pattern input candidates",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
}

#[test]
fn geometry_pattern_inputs_refuses_retained_limit() {
    let mut options = DecodeOptions::default();
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(
        &class_binding_source(),
        &mut options,
        "retain SLDPRT pattern native identity",
    );
    assert!(
        matches!(error, cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
            && limit.operation == "retain SLDPRT pattern native identity")
    );
}

fn configuration_lane_source() -> Vec<u8> {
    let mut source = sweep_binding_source();
    source.extend(make_block(
        0x43,
        "Contents/ConfigurationDefinitions",
        br#"<Keywords><Configuration Name="Default" id="0" SourceIndex="0"/></Keywords>"#,
    ));
    source
}

#[test]
fn metadata_configuration_lanes_refuse_collection_limit() {
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let refusal = collection_refusal_with_options(
        &configuration_lane_source(),
        options,
        "index SLDPRT configuration lane identities",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::CollectionItems
    );
}

#[test]
fn geometry_configuration_lanes_refuse_collection_limit() {
    let refusal = collection_refusal_with_options(
        &configuration_lane_source(),
        DecodeOptions::default(),
        "index SLDPRT configuration lane identities",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::CollectionItems
    );
}

#[test]
fn metadata_configuration_lanes_refuse_work_limit() {
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let refusal = work_refusal_with_options(
        &configuration_lane_source(),
        options,
        "scan SLDPRT configuration lane identities",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
}

#[test]
fn geometry_configuration_lanes_refuse_work_limit() {
    let refusal = work_refusal_with_options(
        &configuration_lane_source(),
        DecodeOptions::default(),
        "scan SLDPRT configuration lane identities",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
}

#[test]
fn metadata_configuration_parameter_kinds_refuse_collection_limit() {
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let refusal = collection_refusal_with_options(
        &native_definition_source(),
        options,
        "index SLDPRT configuration parameter kinds",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::CollectionItems
    );
}

#[test]
fn geometry_configuration_parameter_kinds_refuse_collection_limit() {
    let refusal = collection_refusal_with_options(
        &geometry_parameter_source(),
        DecodeOptions::default(),
        "index SLDPRT configuration parameter kinds",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::CollectionItems
    );
}

#[test]
fn metadata_configuration_parameter_kinds_refuse_work_limit() {
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let refusal = work_refusal_with_options(
        &native_definition_source(),
        options,
        "scan SLDPRT configuration parameter kinds",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
}

#[test]
fn geometry_configuration_parameter_kinds_refuse_work_limit() {
    let refusal = work_refusal_with_options(
        &geometry_parameter_source(),
        DecodeOptions::default(),
        "scan SLDPRT configuration parameter kinds",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
}

fn geometry_parameter_source() -> Vec<u8> {
    let mut source = crate::test_support::container::sldprt_with_body(&triangle_body());
    source.extend(make_block(0x43, "Contents/Keywords",
        br#"<Keywords><Feature Name="Custom" Type="Custom" id="10"><Dimension Name="Length">1mm</Dimension></Feature></Keywords>"#));
    source
}

#[test]
fn geometry_topology_selections_refuse_collection_limit() {
    let refusal = collection_refusal_with_options(
        &geometry_parameter_source(),
        DecodeOptions::default(),
        "index SLDPRT topology selections",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::CollectionItems
    );
}

#[test]
fn geometry_topology_selections_refuse_work_limit() {
    let refusal = work_refusal_with_options(
        &geometry_parameter_source(),
        DecodeOptions::default(),
        "index SLDPRT topology selections",
    );
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
}

#[test]
fn geometry_topology_selections_refuse_retained_limit() {
    let mut options = DecodeOptions::default();
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(
        &topology_plane_source(),
        &mut options,
        "retain SLDPRT topology selection identity",
    );
    assert!(
        matches!(error, cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
            && limit.operation == "retain SLDPRT topology selection identity")
    );
}

fn topology_plane_source() -> Vec<u8> {
    let mut source = crate::test_support::container::sldprt_with_body(&triangle_body());
    source.extend(make_block(0x43, "Contents/Keywords",
        br#"<Keywords><Plane Name="Plane" Type="Plane" Origin="0mm,0mm,0mm" Normal="0,0,1" UAxis="1,0,0"><Dimension Name="D1">1mm</Dimension></Plane></Keywords>"#));
    source
}

#[test]
fn metadata_thread_enrichment_refuses_collection_limit() {
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let limit = collection_refusal_with_options(
        &native_definition_source(),
        options,
        "enrich SLDPRT cosmetic thread diameters",
    );
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::CollectionItems
    );
    assert_eq!(limit.additional, 1);
}

#[test]
fn geometry_thread_enrichment_refuses_collection_limit() {
    let limit = collection_refusal_with_options(
        &geometry_parameter_source(),
        DecodeOptions::default(),
        "enrich SLDPRT cosmetic thread diameters",
    );
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::CollectionItems
    );
    assert_eq!(limit.additional, 1);
}

#[test]
fn metadata_thread_enrichment_refuses_work_limit() {
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let limit = work_refusal_with_request(
        &native_definition_source(),
        options,
        "enrich SLDPRT cosmetic thread diameters",
        Some(1), // One visitor work unit; key reads have separate requests.
    );
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
    assert_eq!(limit.additional, 1);
}

#[test]
fn geometry_thread_enrichment_refuses_work_limit() {
    let limit = work_refusal_with_request(
        &geometry_parameter_source(),
        DecodeOptions::default(),
        "enrich SLDPRT cosmetic thread diameters",
        Some(1),
    );
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
    assert_eq!(limit.additional, 1);
}

fn hole_ownership_source(geometry: bool) -> Vec<u8> {
    let mut source = if geometry {
        crate::test_support::container::sldprt_with_body(&triangle_body())
    } else {
        outer_header()
    };
    source.extend(make_block(0x43, "Contents/Keywords", br#"<Keywords><HoleWizard Name="Hole" Type="HoleWizard" id="7"/><Sketch Name="Profile" Type="Sketch" id="8"><Dimension Name="Diameter">&lt;MOD-DIAM&gt;4.2</Dimension><Dimension Name="Depth">6.8</Dimension></Sketch><Sketch Name="Position" Type="Sketch" id="9"/></Keywords>"#));
    source
}

#[test]
fn metadata_hole_ownership_refuses_collection_limit() {
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let limit = collection_refusal_with_options(
        &hole_ownership_source(false),
        options,
        "enrich SLDPRT hole profile ownership",
    );
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::CollectionItems
    );
    assert_eq!(limit.additional, 1);
}

#[test]
fn geometry_hole_ownership_refuses_collection_limit() {
    let limit = collection_refusal_with_options(
        &hole_ownership_source(true),
        DecodeOptions::default(),
        "enrich SLDPRT hole profile ownership",
    );
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::CollectionItems
    );
    assert_eq!(limit.additional, 1);
}

#[test]
fn metadata_hole_ownership_refuses_work_limit() {
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let limit = work_refusal_with_request(
        &hole_ownership_source(false),
        options,
        "enrich SLDPRT hole profile ownership",
        Some(1), // One visitor work unit; key reads have separate requests.
    );
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
    assert_eq!(limit.additional, 1);
}

#[test]
fn geometry_hole_ownership_refuses_work_limit() {
    let limit = work_refusal_with_request(
        &hole_ownership_source(true),
        DecodeOptions::default(),
        "enrich SLDPRT hole profile ownership",
        Some(1),
    );
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
    // Select the single entity visit, independently of key-copy requests with the same label.
    assert_eq!(limit.additional, 1);
}

#[test]
fn metadata_hole_ownership_refuses_retained_limit() {
    let mut options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(
        &hole_ownership_source(false),
        &mut options,
        "copy SLDPRT hole profile ownership",
    );
    assert!(
        matches!(error, cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
            && limit.operation == "copy SLDPRT hole profile ownership")
    );
}

#[test]
fn geometry_hole_ownership_refuses_retained_limit() {
    let mut options = DecodeOptions::default();
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(
        &hole_ownership_source(true),
        &mut options,
        "copy SLDPRT hole profile ownership",
    );
    assert!(
        matches!(error, cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
            && limit.operation == "copy SLDPRT hole profile ownership")
    );
}

fn hole_bound_sketch_source() -> Vec<u8> {
    let mut source =
        crate::test_support::history::sldprt_with_nested_sketch_profile(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    source
}

#[test]
fn metadata_profiled_hole_projection_refuses_collection_limit() {
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let limit = collection_refusal_with_options(
        &hole_ownership_source(false),
        options,
        "project SLDPRT profiled hole constructions",
    );
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::CollectionItems
    );
    assert_eq!(limit.additional, 1);
}

#[test]
fn metadata_profiled_hole_projection_refuses_work_limit() {
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let limit = work_refusal_with_request(
        &hole_ownership_source(false),
        options,
        "project SLDPRT profiled hole constructions",
        Some(1), // One visitor work unit; key reads have separate requests.
    );
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
    assert_eq!(limit.additional, 1);
}

#[test]
fn metadata_profiled_hole_projection_refuses_retained_limit() {
    let mut options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(
        &hole_bound_sketch_source(),
        &mut options,
        "project SLDPRT profiled hole constructions",
    );
    assert!(
        matches!(error, cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
            && limit.operation == "project SLDPRT profiled hole constructions")
    );
}

#[test]
fn geometry_profiled_hole_projection_refuses_collection_limit() {
    let options = DecodeOptions::default();
    let limit = collection_refusal_with_options(
        &hole_ownership_source(true),
        options,
        "project SLDPRT profiled hole constructions",
    );
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::CollectionItems
    );
    assert_eq!(limit.additional, 1);
}

#[test]
fn geometry_profiled_hole_projection_refuses_work_limit() {
    let options = DecodeOptions::default();
    let limit = work_refusal_with_request(
        &hole_ownership_source(true),
        options,
        "project SLDPRT profiled hole constructions",
        Some(1),
    );
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
    assert_eq!(limit.additional, 1);
}

#[test]
fn geometry_profiled_hole_projection_refuses_retained_limit() {
    let mut options = DecodeOptions::default();
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(
        &hole_bound_sketch_source(),
        &mut options,
        "project SLDPRT profiled hole constructions",
    );
    assert!(
        matches!(error, cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
            && limit.operation == "project SLDPRT profiled hole constructions")
    );
}

#[test]
fn metadata_hole_position_projection_refuses_collection_limit() {
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let limit = collection_refusal_with_options(
        &hole_ownership_source(false),
        options,
        "project SLDPRT hole position sketches",
    );
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::CollectionItems
    );
    assert_eq!(limit.additional, 1);
}

#[test]
fn metadata_hole_position_projection_refuses_work_limit() {
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let limit = work_refusal_with_request(
        &hole_ownership_source(false),
        options,
        "project SLDPRT hole position sketches",
        Some(1), // One visitor work unit; key reads have separate requests.
    );
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
    assert_eq!(limit.additional, 1);
}

#[test]
fn metadata_hole_position_projection_refuses_retained_limit() {
    let mut options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(
        &hole_bound_sketch_source(),
        &mut options,
        "project SLDPRT hole position sketches",
    );
    assert!(
        matches!(error, cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
            && limit.operation == "project SLDPRT hole position sketches")
    );
}

#[test]
fn geometry_hole_position_projection_refuses_collection_limit() {
    let options = DecodeOptions::default();
    let limit = collection_refusal_with_options(
        &hole_ownership_source(true),
        options,
        "project SLDPRT hole position sketches",
    );
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::CollectionItems
    );
    assert_eq!(limit.additional, 1);
}

#[test]
fn geometry_hole_position_projection_refuses_work_limit() {
    let options = DecodeOptions::default();
    let limit = work_refusal_with_request(
        &hole_ownership_source(true),
        options,
        "project SLDPRT hole position sketches",
        Some(1),
    );
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
    assert_eq!(limit.additional, 1);
}

#[test]
fn geometry_hole_position_projection_refuses_retained_limit() {
    let mut options = DecodeOptions::default();
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(
        &hole_bound_sketch_source(),
        &mut options,
        "project SLDPRT hole position sketches",
    );
    assert!(
        matches!(error, cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
            && limit.operation == "project SLDPRT hole position sketches")
    );
}

#[test]
fn metadata_bore_backed_position_projection_refuses_collection_limit() {
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let limit = collection_refusal_with_options(
        &hole_ownership_source(false),
        options,
        "project SLDPRT bore backed position sketches",
    );
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::CollectionItems
    );
    assert_eq!(limit.additional, 1);
}

#[test]
fn metadata_bore_backed_position_projection_refuses_work_limit() {
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let limit = work_refusal_with_request(
        &hole_ownership_source(false),
        options,
        "project SLDPRT bore backed position sketches",
        Some(1), // One visitor work unit; key reads have separate requests.
    );
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
    assert_eq!(limit.additional, 1);
}

#[test]
fn geometry_bore_backed_position_projection_refuses_collection_limit() {
    let options = DecodeOptions::default();
    let limit = collection_refusal_with_options(
        &hole_ownership_source(true),
        options,
        "project SLDPRT bore backed position sketches",
    );
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::CollectionItems
    );
    assert_eq!(limit.additional, 1);
}

#[test]
fn geometry_bore_backed_position_projection_refuses_work_limit() {
    let options = DecodeOptions::default();
    let limit = work_refusal_with_request(
        &hole_ownership_source(true),
        options,
        "project SLDPRT bore backed position sketches",
        Some(1), // One visitor work unit; key reads have separate requests.
    );
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
    assert_eq!(limit.additional, 1);
}
