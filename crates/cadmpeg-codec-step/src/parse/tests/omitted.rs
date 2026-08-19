// SPDX-License-Identifier: Apache-2.0
//! Part 21 omitted-name recovery tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]
#![allow(unused_imports)]

use std::fmt::Write as _;

use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::codec::{CodecBackend, Confidence};
use cadmpeg_ir::eval::{
    model_curve_point_by_id, model_surface_partials_by_id, model_surface_point_by_id, pcurve_uv,
};
use cadmpeg_ir::examples::unit_cube;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, NurbsCurve, NurbsSurface, PcurveGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{ProceduralCurveId, SurfaceId};
use cadmpeg_ir::index::ModelIndex;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::units::{LengthUnit, Units};
use cadmpeg_ir::CadIr;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::ids::StepIdentity;
use crate::test_support::{decode_inline, export};
use crate::{write_step, StepError, StepSchema, StepUnsupportedPolicy, StepWriteOptions};

#[test]
fn omitted_name_recovery_accounts_for_inserted_parameter_storage() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=CARTESIAN_POINT((0.,0.,0.));ENDSEC;END-ISO-10303-21;";
    let (exchange, diagnostics) = crate::parse::parse(source).expect("recover omitted name");
    let parameters = &exchange.records[&1].partials[0].parameters;

    assert_eq!(parameters.len(), 2);
    assert_eq!(diagnostics.len(), 1);

    let mut recovery_limit = None;
    for max_retained_bytes in 1..=8192 {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_retained_bytes = max_retained_bytes;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(source, &arena, &policy)
                .expect("root fits the test policy");
        let error = crate::parse::parse_with_context(source, &ctx)
            .expect_err("recovered storage must consume retained bytes");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            continue;
        };
        if limit.context.operation == "step_omitted_name_recovery_storage" {
            recovery_limit = Some(limit);
            break;
        }
    }
    let limit = recovery_limit.expect("recovered name storage must have a budget gate");
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::RetainedBytes
    );
    assert!(limit.additional > 0);
}

#[test]
fn parser_recovers_omitted_repositioned_tessellated_item_name() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=REPOSITIONED_TESSELLATED_ITEM(#2);#2=KNOWN();ENDSEC;END-ISO-10303-21;";
    let (exchange, diagnostics) =
        crate::parse::parse(source).expect("recover omitted repositioned item name");
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].kind,
        crate::parse::ParseDiagnosticKind::OmittedEntityName
    );
    assert_eq!(
        exchange.records[&1].partials[0].parameters,
        vec![
            crate::parse::Value::String(Vec::new()),
            crate::parse::Value::Reference(2),
        ]
    );
}

#[test]
fn parser_retains_user_defined_entity_and_type_names() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=!VENDOR_ENTITY(!VENDOR_TYPE(#2));#2=KNOWN();ENDSEC;END-ISO-10303-21;";
    let (exchange, diagnostics) = crate::parse::parse(source).expect("user-defined names");

    assert!(diagnostics.is_empty());
    assert_eq!(exchange.records[&1].partials[0].name, "!VENDOR_ENTITY");
    assert_eq!(
        exchange.records[&1].partials[0].parameters,
        vec![crate::parse::Value::Typed(
            "!VENDOR_TYPE".into(),
            Box::new(crate::parse::Value::Reference(2)),
        )]
    );
}

#[test]
fn parser_retains_user_defined_typed_parameter_from_witness() {
    let source = include_bytes!("../../reader/tests/data/ud01_user_defined_entity.p21");
    let (exchange, diagnostics) = crate::parse::parse(source).expect("user-defined type witness");

    assert!(diagnostics.is_empty());
    assert_eq!(
        exchange.records[&2].partials[0].parameters[2],
        crate::parse::Value::Typed(
            "!VENDOR_TYPE".into(),
            Box::new(crate::parse::Value::List(vec![
                crate::parse::Value::Reference(1),
            ])),
        )
    );
}

#[test]
fn parser_does_not_repair_non_carrier_first_parameters() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=!VENDOR_ENTITY(1,#2);#2=KNOWN();ENDSEC;END-ISO-10303-21;";
    let (exchange, diagnostics) = crate::parse::parse(source).expect("non-carrier entity");

    assert!(diagnostics.is_empty());
    assert_eq!(
        exchange.records[&1].partials[0].parameters,
        vec![
            crate::parse::Value::Integer(1),
            crate::parse::Value::Reference(2),
        ]
    );
}

#[test]
fn parser_recovers_omitted_geometry_name_without_shifting_context_fields() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=CARTESIAN_POINT((0.,1.,2.));#2=GEOMETRIC_REPRESENTATION_CONTEXT(3);#3=MAPPED_ITEM(#1,#2);#4=SEAM_EDGE(*,*,#1,.T.,$);#5=SHAPE_REPRESENTATION((#1),$);#6=CLOSED_SHELL($,(#1));ENDSEC;END-ISO-10303-21;";
    let (exchange, diagnostics) =
        crate::parse::parse(source).expect("omitted geometry name is recoverable");

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].kind,
        crate::parse::ParseDiagnosticKind::OmittedEntityName
    );
    assert!(diagnostics[0]
        .message
        .contains("recovered 4 simple named carrier instance(s)"));
    assert_eq!(
        exchange.records[&1].partials[0].parameters,
        vec![
            crate::parse::Value::String(Vec::new()),
            crate::parse::Value::List(vec![
                crate::parse::Value::Real(0.0),
                crate::parse::Value::Real(1.0),
                crate::parse::Value::Real(2.0),
            ]),
        ]
    );
    assert_eq!(
        exchange.records[&2].partials[0].parameters,
        vec![crate::parse::Value::Integer(3)]
    );
    assert_eq!(
        exchange.records[&3].partials[0].parameters[0],
        crate::parse::Value::String(Vec::new())
    );
    assert_eq!(
        exchange.records[&4].partials[0].parameters[0],
        crate::parse::Value::String(Vec::new())
    );
    assert_eq!(
        exchange.records[&5].partials[0].parameters[0],
        crate::parse::Value::String(Vec::new())
    );
    assert_eq!(
        exchange.records[&6].partials[0].parameters,
        vec![
            crate::parse::Value::Omitted,
            crate::parse::Value::List(vec![crate::parse::Value::Reference(1)]),
        ]
    );
}

#[test]
fn parser_recovers_omitted_shape_representation_with_parameters_name() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=SHAPE_REPRESENTATION_WITH_PARAMETERS((#2),#3);#2=KNOWN();#3=KNOWN();ENDSEC;END-ISO-10303-21;";
    let (exchange, diagnostics) =
        crate::parse::parse(source).expect("recover omitted parameterized shape name");

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].kind,
        crate::parse::ParseDiagnosticKind::OmittedEntityName
    );
    assert_eq!(
        exchange.records[&1].partials[0].parameters,
        vec![
            crate::parse::Value::String(Vec::new()),
            crate::parse::Value::List(vec![crate::parse::Value::Reference(2)]),
            crate::parse::Value::Reference(3),
        ]
    );
}
