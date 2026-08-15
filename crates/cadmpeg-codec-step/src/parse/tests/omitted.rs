// SPDX-License-Identifier: Apache-2.0
//! Part 21 omitted-name recovery tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]
#![allow(unused_imports)]

use std::fmt::Write as _;
use std::io::Cursor;

use cadmpeg_core::decode::{DecodeMode, InspectOptions};
use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions};
use cadmpeg_ir::eval::{
    model_curve_point_by_id, model_surface_partials_by_id, model_surface_point_by_id, pcurve_uv,
};
use cadmpeg_ir::examples::unit_cube;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, NurbsCurve, NurbsSurface, PcurveGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, ProceduralCurveId, SurfaceId};
use cadmpeg_ir::index::ModelIndex;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::units::{LengthUnit, Units};
use cadmpeg_ir::CadIr;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::ids::StepIdentity;
use crate::loss::StepLossCode;
use crate::test_support::{decode_inline, export};
use crate::{
    write_step, StepCodec, StepError, StepSchema, StepUnsupportedPolicy, StepWriteOptions,
};

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

#[test]
fn omitted_geometry_names_preserve_intersection_curve_topology() {
    let mut source =
        String::from_utf8(include_bytes!("../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace(
                "#2=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('model','3D'));",
                "#2=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1,#69)) REPRESENTATION_CONTEXT('model','3D'));",
            )
            .replace(
                "#57=SURFACE_CURVE('',#16,(#56),.PCURVE_S1.);",
                "#57=INTERSECTION_CURVE(#16,(#56),.PCURVE_S1.);",
            )
            .replace(
                "ENDSEC;\nEND-ISO-10303-21;",
                "#69=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));\nENDSEC;\nEND-ISO-10303-21;",
            );
    for (id, entity) in [
        ("3", "CARTESIAN_POINT"),
        ("4", "CARTESIAN_POINT"),
        ("5", "CARTESIAN_POINT"),
        ("6", "VERTEX_POINT"),
        ("7", "VERTEX_POINT"),
        ("8", "VERTEX_POINT"),
        ("9", "DIRECTION"),
        ("10", "DIRECTION"),
        ("11", "DIRECTION"),
        ("12", "DIRECTION"),
        ("13", "VECTOR"),
        ("14", "VECTOR"),
        ("15", "VECTOR"),
        ("16", "LINE"),
        ("17", "LINE"),
        ("18", "LINE"),
        ("19", "EDGE_CURVE"),
        ("20", "EDGE_CURVE"),
        ("21", "EDGE_CURVE"),
        ("22", "ORIENTED_EDGE"),
        ("23", "ORIENTED_EDGE"),
        ("24", "ORIENTED_EDGE"),
        ("25", "EDGE_LOOP"),
        ("26", "FACE_OUTER_BOUND"),
        ("27", "AXIS2_PLACEMENT_3D"),
        ("28", "PLANE"),
        ("29", "ADVANCED_FACE"),
        ("30", "OPEN_SHELL"),
        ("31", "SHELL_BASED_SURFACE_MODEL"),
        ("33", "ORIENTED_OPEN_SHELL"),
        ("51", "CARTESIAN_POINT"),
        ("52", "DIRECTION"),
        ("53", "VECTOR"),
        ("54", "LINE"),
        ("55", "DEFINITIONAL_REPRESENTATION"),
        ("56", "PCURVE"),
    ] {
        let named = format!("#{id}={entity}('',");
        let unnamed = format!("#{id}={entity}(");
        let previous_len = source.len();
        source = source.replace(&named, &unnamed);
        assert!(
            source.len() < previous_len,
            "fixture record #{id} was not converted to omitted-name syntax"
        );
    }

    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode omitted-name intersection curve");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    let edge = decoded
        .ir()
        .model
        .edges
        .iter()
        .find(|edge| edge.id.as_str() == "step:data:edge#19")
        .expect("omitted-name intersection edge");
    assert_eq!(
        edge.curve.as_ref().map(CurveId::as_str),
        Some("step:data:curve#16")
    );
    assert!(decoded.ir().model.coedges.iter().any(|coedge| {
        coedge
            .pcurves
            .iter()
            .any(|use_| use_.pcurve.as_str() == "step:data:pcurve#56")
    }));
    let name_loss = decoded
        .report()
        .losses
        .iter()
        .find(|loss| {
            loss.code == StepLossCode::ParseNoncanonicalSyntax.kind()
                && loss
                    .message
                    .contains("recovered 37 simple named carrier instance(s)")
        })
        .expect("omitted-name recovery loss");
    assert_eq!(
        name_loss
            .provenance
            .as_ref()
            .and_then(|provenance| provenance.tag.as_deref()),
        Some("entity_name")
    );
    assert!(decoded.report().losses.iter().all(|loss| {
        !loss
            .message
            .contains("INTERSECTION_CURVE #57 has no decoded 3D curve")
    }));

    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}
