// SPDX-License-Identifier: Apache-2.0
//! Combine, revolution, sweep, operand, and XML-family projection tests.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::*;
use crate::SldprtCodec;

#[test]
fn decode_resolves_feature_topology_selections() {
    use cadmpeg_ir::features::{
        BodySelection, EdgeSelection, ExtrudeExtent, ExtrudeSide, FaceSelection, FeatureDefinition,
        PathRef, ProfileRef, Termination,
    };

    // Two bodies so the combine has disjoint operands: a body cannot be both
    // the target and the tool of its own boolean.
    let mut body_bytes = Vec::new();
    body_bytes.extend(entity51(2, 500, 0x0017, &[700, 0, 0, 0, 0, 0]));
    body_bytes.extend(entity51(2, 501, 0x0017, &[701, 0, 0, 0, 0, 0]));
    body_bytes.extend(owned_triangle(0, 700, 0.0));
    body_bytes.extend(owned_triangle(200, 701, 10.0));
    let base = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body_bytes)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(base.ir().model.bodies.len(), 2);
    let body = &base.ir().model.bodies[0].id.0;
    let tool_body = &base.ir().model.bodies[1].id.0;
    let face = &base.ir().model.faces[0].id.0;
    let edge = &base.ir().model.edges[0].id.0;
    let keywords = format!(
        r#"<Keywords>
            <Fillet Name="Round" Type="Fillet" id="1" Edges="{edge}"><Dimension Name="Radius">1mm</Dimension></Fillet>
            <DeleteFace Name="Delete" Type="DeleteFace" id="2" Faces="{face}" Heal="true"/>
            <Combine Name="Union" Type="Combine" id="3" Target="{body}" Tools="{tool_body}" Operation="Join"/>
            <Extrusion Name="UpTo" Type="BossExtrude" id="4" Profile="{face}" EndCondition="ToFace" Face="{face}" Operation="Join"/>
            <Hole Name="Drill" Type="Hole" id="5" Face="{face}" EndCondition="ThroughAll"><Dimension Name="Diameter">2mm</Dimension></Hole>
            <Sweep Name="Rail" Type="Sweep" id="6" Profile="{face}" Path="{edge}" Operation="NewBody"/>
        </Keywords>"#
    );
    let mut source = sldprt_with_body(&body_bytes);
    source.extend(make_block(0x42, "Contents/Keywords", keywords.as_bytes()));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let edge_id = decoded.ir().model.edges[0].id.clone();
    let face_id = decoded.ir().model.faces[0].id.clone();
    let body_id = decoded.ir().model.bodies[0].id.clone();
    let tool_body_id = decoded.ir().model.bodies[1].id.clone();

    assert!(matches!(
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::Fillet {
            groups,
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            edges: EdgeSelection::Resolved { edges, native }, ..
        }] if edges == &[base.ir().model.edges[0].id.clone()] && native == edge)
    ));
    assert!(matches!(
        &decoded.ir().model.features[1].definition,
        FeatureDefinition::DeleteFace {
            faces: FaceSelection::Resolved { faces, native },
            ..
        } if faces == &[base.ir().model.faces[0].id.clone()] && native == face
    ));
    assert!(matches!(
        &decoded.ir().model.features[2].definition,
        FeatureDefinition::Combine {
            target: BodySelection::Resolved { bodies, native },
            tools: BodySelection::Resolved { .. },
            ..
        } if bodies == &[base.ir().model.bodies[0].id.clone()] && native == body
    ));
    assert!(matches!(
        &decoded.ir().model.features[3].definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Faces(profile_faces),
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::ToFace {
                        face: FaceSelection::Resolved { faces, native },
                        ..
                    },
                    ..
                }
            },
            ..
        } if profile_faces == &[base.ir().model.faces[0].id.clone()]
            && faces == &[base.ir().model.faces[0].id.clone()] && native == face
    ));
    assert!(matches!(
        &decoded.ir().model.features[4].definition,
        FeatureDefinition::Hole {
            face: Some(FaceSelection::Resolved { faces, native }),
            ..
        } if faces == &[base.ir().model.faces[0].id.clone()] && native == face
    ));
    assert!(matches!(
        &decoded.ir().model.features[5].definition,
        FeatureDefinition::Sweep {
            section: cadmpeg_ir::features::SweepSection::Profile(ProfileRef::Faces(faces)),
            path: Some(PathRef::Edges(edges)),
            ..
        } if faces == std::slice::from_ref(&face_id) && edges == std::slice::from_ref(&edge_id)
    ));

    if let FeatureDefinition::Fillet { groups } = &mut decoded.ir_mut().model.features[0].definition
    {
        groups[0].edges = EdgeSelection::Edges(vec![edge_id.clone()]);
    }
    if let FeatureDefinition::DeleteFace { faces, .. } =
        &mut decoded.ir_mut().model.features[1].definition
    {
        *faces = FaceSelection::Faces(vec![face_id.clone()]);
    }
    if let FeatureDefinition::Combine { target, tools, .. } =
        &mut decoded.ir_mut().model.features[2].definition
    {
        *target = BodySelection::Bodies(vec![body_id.clone()]);
        *tools = BodySelection::Bodies(vec![tool_body_id.clone()]);
    }
    if let FeatureDefinition::Extrude {
        extent:
            ExtrudeExtent::OneSided {
                side:
                    ExtrudeSide {
                        termination: Termination::ToFace { face, .. },
                        ..
                    },
            },
        ..
    } = &mut decoded.ir_mut().model.features[3].definition
    {
        *face = FaceSelection::Faces(vec![face_id.clone()]);
    }
    if let FeatureDefinition::Hole { face, .. } = &mut decoded.ir_mut().model.features[4].definition
    {
        *face = Some(FaceSelection::Faces(vec![face_id.clone()]));
    }
    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let records = &sldprt_native(regenerated.ir()).feature_histories[0].features;
    assert_eq!(records[0].properties["Edges"], edge_id.0);
    assert_eq!(records[1].properties["Faces"], face_id.0);
    assert_eq!(records[2].properties["Target"], body_id.0);
    assert_eq!(records[2].properties["Tools"], tool_body_id.0);
    assert_eq!(records[3].properties["Face"], face_id.0);
    assert_eq!(records[3].properties["Profile"], face_id.0);
    assert_eq!(records[4].properties["Face"], face_id.0);
    assert_eq!(records[5].properties["Profile"], face_id.0);
    assert_eq!(records[5].properties["Path"], edge_id.0);
}

#[test]
fn decode_reports_unresolved_feature_output_scope() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Scoped" Type="Custom" id="1" Scope="MissingBody"/></Keywords>"#,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();

    assert!(decoded.ir().model.features[0].outputs.is_empty());
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.message
            == "1 feature(s) retain non-empty native output scopes that do not resolve to model bodies."
    }));
}

#[test]
fn decode_dispatches_typed_features_by_xml_family() {
    use cadmpeg_ir::features::{ChamferSpec, FeatureDefinition, HoleKind, Length, RadiusSpec};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Sketch Name="Profile" Type="CustomSketch" id="51"/>
            <ReferencePoint Name="Origin" Type="CustomDatum" id="52" Position="1mm,2mm,3mm"/>
            <Fillet Name="Round" Type="CustomFillet" id="53" Dependencies="51,52,51" Algorithm="RollingBall"><Dimension Name="Radius">2mm</Dimension></Fillet>
            <Chamfer Name="Bevel" Type="CustomChamfer" id="54"><Dimension Name="Distance">3mm</Dimension></Chamfer>
            <Hole Name="Drill" Type="CustomHole" id="55"><Dimension Name="Diameter">4mm</Dimension><Dimension Name="Depth">5mm</Dimension></Hole>
        </Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::Sketch { .. }
    ));
    assert!(matches!(
        decoded.ir().model.features[1].definition,
        FeatureDefinition::DatumPoint { .. }
    ));
    assert!(matches!(
        &decoded.ir().model.features[2].definition,
        FeatureDefinition::Fillet {
            groups,
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            radius: RadiusSpec::Constant {
                radius: Length(2.0),
            },
            ..
        }])
    ));
    assert_eq!(
        decoded.ir().model.features[2].dependencies,
        vec![
            decoded.ir().model.features[0].id.clone(),
            decoded.ir().model.features[1].id.clone(),
        ]
    );
    assert_eq!(
        decoded.ir().model.features[2].source_properties["Algorithm"],
        "RollingBall"
    );
    assert!(matches!(
        &decoded.ir().model.features[3].definition,
        FeatureDefinition::Chamfer {
            groups,
            ..
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::ChamferGroup {
            spec: ChamferSpec::Distance {
                distance: Length(3.0),
            },
            ..
        }])
    ));
    assert!(matches!(
        decoded.ir().model.features[4].definition,
        FeatureDefinition::Hole {
            kind: HoleKind::Simple,
            diameter: Some(Length(4.0)),
            ..
        }
    ));

    {
        let mut ir = decoded.ir_mut();
        let FeatureDefinition::Fillet { groups } = &mut ir.model.features[2].definition else {
            panic!("typed custom fillet");
        };
        let RadiusSpec::Constant { radius } = &mut groups[0].radius else {
            panic!("constant fillet");
        };
        *radius = Length(2.5);
        ir.model.features[2]
            .source_properties
            .insert("Algorithm".into(), "FaceBlend".into());
    }
    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let mut regenerated = cadmpeg_test_support::EditableDecodeResult::from(regenerated);
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features;
    assert_eq!(native[2].kind, "CustomFillet");
    assert_eq!(native[2].parameters["Radius"], "2.5mm");
    assert_eq!(native[2].properties["Algorithm"], "FaceBlend");
    assert_eq!(
        regenerated.ir().model.features[2].source_properties["Algorithm"],
        "FaceBlend"
    );
    assert_eq!(
        regenerated.ir().model.features[2].dependencies,
        vec![
            regenerated.ir().model.features[0].id.clone(),
            regenerated.ir().model.features[1].id.clone(),
        ]
    );
    regenerated.ir_mut().model.features[2].dependencies.pop();
    let error = crate::test_support::plan_inherited_write(
        regenerated.ir(),
        regenerated.source_fidelity(),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("dependencies are inconsistent with its operands"),
        "{error}"
    );
}

#[test]
fn decode_projects_compact_combine_with_unresolved_semantics() {
    use cadmpeg_ir::features::{BodySelection, BooleanOp, FeatureDefinition};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Compact" Type="Localized" id="119"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[("moCombineBodies_c", "Compact", 119)]),
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::Combine {
            target: BodySelection::Unresolved,
            tools: BodySelection::Unresolved,
            op: BooleanOp::Unresolved,
            keep_tools: false,
        }
    ));

    decoded.ir_mut().model.features[0].name = Some("Renamed compact combine".into());
    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        regenerated.ir().model.features[0].definition,
        FeatureDefinition::Combine {
            target: BodySelection::Unresolved,
            tools: BodySelection::Unresolved,
            op: BooleanOp::Unresolved,
            keep_tools: false,
        }
    ));
}

#[test]
fn decode_does_not_globalize_configuration_local_combine_selection() {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition};

    fn append_body_path(payload: &mut Vec<u8>, local_id: u32) {
        payload.extend_from_slice(&1u32.to_le_bytes());
        payload.extend_from_slice(&[0, 3, 0, 0]);
        payload.extend_from_slice(&[0; 4]);
        payload.extend_from_slice(&[
            0x7d, 0xc3, 0x94, 0x25, 0xad, 0x49, 0xb2, 0x54, 0x7d, 0xc3, 0x94, 0x25, 0xad, 0x49,
            0xb2, 0x54,
        ]);
        payload.extend_from_slice(&[0, 0]);
        payload.extend_from_slice(&[0x32, 0x80, 0, 0]);
        payload.extend_from_slice(&[1; 12]);
        payload.extend_from_slice(&local_id.to_le_bytes());
    }

    fn combine_payload(has_selection: bool) -> Vec<u8> {
        let mut payload =
            resolved_feature_classes_with_ids(&[("moCombineBodies_c", "Combine", 119)]);
        if has_selection {
            append_body_path(&mut payload, 6);
            append_body_path(&mut payload, 7);
        }
        payload
    }

    let resolved_selection = combine_payload(true);
    assert_eq!(
        (12..resolved_selection.len())
            .filter(
                |offset| crate::resolved_features::terminations::compact_body_path_at(
                    &resolved_selection,
                    *offset
                )
                .is_some()
            )
            .count(),
        2
    );

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="Default"/><Configuration Name="Alternate"/><Feature Name="Combine" Type="Localized" id="119"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_selection,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-1-ResolvedFeatures",
        &combine_payload(false),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::Combine {
            target: BodySelection::Unresolved,
            tools: BodySelection::Unresolved,
            ..
        }
    ));
    let feature_id = decoded.ir().model.features[0].id.clone();
    assert!(matches!(
        &decoded.ir().model.configurations[0].feature_states[&feature_id].definition,
        FeatureDefinition::Combine {
            target: BodySelection::Native(target),
            tools: BodySelection::Native(tools),
            ..
        } if target.starts_with("sldprt:feature-input:body-path:")
            && tools.starts_with("sldprt:feature-input:body-path:")
    ));
    assert!(matches!(
        decoded.ir().model.configurations[1].feature_states[&feature_id].definition,
        FeatureDefinition::Combine {
            target: BodySelection::Unresolved,
            tools: BodySelection::Unresolved,
            ..
        }
    ));

    let mut source = outer_header();
    source.extend(make_block(
        0x20,
        "Contents/Config-1-Partition",
        &parasolid_with_body("partition body", "SCH_SW_33103_11000", &triangle_body()),
    ));
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="Default"/><Configuration Name="Alternate"/><Feature Name="Combine" Type="Localized" id="119"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x43,
        "Contents/SolidWorks",
        br#"<?xml version="1.0"?><swSolidWorks swVersion="34000"><swModel swName="Part" swConfigurationName="Default"/></swSolidWorks>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &combine_payload(false),
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-1-ResolvedFeatures",
        &resolved_selection,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let feature_id = decoded.ir().model.features[0].id.clone();
    assert!(decoded.ir().model.configurations[0].active);
    assert_eq!(decoded.ir().model.configurations[0].source_index, Some(1));
    assert!(matches!(
        &decoded.ir().model.configurations[0].feature_states[&feature_id].definition,
        FeatureDefinition::Combine {
            target: BodySelection::Unresolved,
            tools: BodySelection::Unresolved,
            ..
        }
    ));
    assert!(matches!(
        &decoded.ir().model.configurations[1].feature_states[&feature_id].definition,
        FeatureDefinition::Combine {
            target: BodySelection::Native(target),
            tools: BodySelection::Native(tools),
            ..
        } if target.starts_with("sldprt:feature-input:body-path:")
            && tools.starts_with("sldprt:feature-input:body-path:")
    ));
}

#[test]
fn decode_projects_generic_revolution_with_explicit_operation() {
    use cadmpeg_ir::features::{BooleanOp, FeatureDefinition, RevolveExtent, Termination};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Revolution Name="Generic" Type="GenericRevolution" id="43" Operation="Cut" AxisOrigin="0mm,0mm,0mm" AxisDirection="0,0,1"><Dimension Name="Angle">180deg</Dimension></Revolution></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction {
                extent: Some(RevolveExtent::OneSided {
                    termination: Termination::Angle { angle },
                }),
                ..
            },
            op: BooleanOp::Cut,
        } if (angle.0 - std::f64::consts::PI).abs() < 1.0e-12
    ));
}

#[test]
fn decode_projects_compact_solid_sweep_join_operation() {
    use cadmpeg_ir::features::{BooleanOp, FeatureDefinition, SweepMode};

    let mut source = sldprt_with_body(&triangle_body());
    add_solidworks_version(&mut source, 17_000);
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="Default"/><Feature Name="Sweep" Type="Localized" id="137"/></Keywords>"#,
    ));
    let mut resolved = 15u32.to_le_bytes().to_vec();
    resolved.extend_from_slice(&[0; 8]);
    resolved.extend(resolved_feature_classes_with_ids(&[(
        "moSweep_c",
        "Sweep",
        137,
    )]));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::Sweep {
            mode: SweepMode::Solid {
                op: BooleanOp::Join
            },
            ..
        }
    ));
    let feature_id = &decoded.ir().model.features[0].id;
    assert!(matches!(
        decoded.ir().model.configurations[0].feature_states[feature_id].definition,
        FeatureDefinition::Sweep {
            mode: SweepMode::Solid {
                op: BooleanOp::Join
            },
            ..
        }
    ));
}

#[test]
fn decode_projects_compact_solid_sweep_general_curve_path() {
    use cadmpeg_ir::features::{FeatureDefinition, PathRef};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Sweep" Type="Localized" id="137"/></Keywords>"#,
    ));
    let mut resolved = resolved_feature_classes_with_ids(&[("moSweep_c", "Sweep", 137)]);
    let path_offset = resolved.len();
    resolved.extend_from_slice(&[0xff, 0xff, 0x01, 0x00]);
    let path_class = b"moGeneralCurveRef_w";
    resolved.extend_from_slice(&(path_class.len() as u16).to_le_bytes());
    resolved.extend_from_slice(path_class);
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::Sweep {
            path: Some(PathRef::Native(path)),
            ..
        } if path.ends_with(&format!(":{path_offset}"))
    ));
}

#[test]
fn decode_does_not_globalize_configuration_local_sweep_path() {
    use cadmpeg_ir::features::{FeatureDefinition, PathRef};

    fn sweep_payload(has_path: bool) -> Vec<u8> {
        let mut payload = resolved_feature_classes_with_ids(&[("moSweep_c", "Sweep", 137)]);
        if has_path {
            payload.extend_from_slice(&[0xff, 0xff, 0x01, 0x00]);
            let path_class = b"moGeneralCurveRef_w";
            payload.extend_from_slice(&(path_class.len() as u16).to_le_bytes());
            payload.extend_from_slice(path_class);
        }
        payload
    }

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="Default"/><Configuration Name="Alternate"/><Feature Name="Sweep" Type="Localized" id="137"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &sweep_payload(true),
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-1-ResolvedFeatures",
        &sweep_payload(false),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::Sweep { path: None, .. }
    ));
    let feature_id = decoded.ir().model.features[0].id.clone();
    assert!(matches!(
        &decoded.ir().model.configurations[0].feature_states[&feature_id].definition,
        FeatureDefinition::Sweep {
            path: Some(PathRef::Native(path)),
            ..
        } if path.starts_with("sldprt:feature-input:general-curve-ref:")
    ));
    assert!(matches!(
        decoded.ir().model.configurations[1].feature_states[&feature_id].definition,
        FeatureDefinition::Sweep { path: None, .. }
    ));
}

#[test]
fn decode_projects_native_surface_sweep_class_without_localized_type() {
    use cadmpeg_ir::features::{FeatureDefinition, PathRef, SweepMode};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Operacion1" Type="Personalizado" id="137"/></Keywords>"#,
    ));
    let mut resolved =
        resolved_feature_classes_with_ids(&[("moSweepRefSurface_c", "Operacion1", 137)]);
    let path_offset = resolved.len();
    resolved.extend_from_slice(&[0xff, 0xff, 0x01, 0x00]);
    let path_class = b"moGeneralCurveRef_w";
    resolved.extend_from_slice(&(path_class.len() as u16).to_le_bytes());
    resolved.extend_from_slice(path_class);
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::Sweep {
            mode: SweepMode::Surface,
            path: Some(PathRef::Native(ref path)),
            ..
        }
        if path.ends_with(&format!(":{path_offset}"))
    ));
}

#[test]
fn decode_projects_surface_sweep_reference_curve_profile() {
    use cadmpeg_ir::features::{FeatureDefinition, ProfileRef};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Feature Name="Helix1" Type="Helix/Spiral" id="119"/>
            <Feature Name="Surface-Sweep1" Type="Surface-Sweep" id="137"/>
        </Keywords>"#,
    ));
    let mut resolved = resolved_feature_classes_with_ids(&[
        ("moHelix_c", "Helix1", 119),
        ("moSweepRefSurface_c", "Surface-Sweep1", 137),
    ]);
    resolved.extend_from_slice(&[0xdd, 0x94, 0xff, 0xff, 1, 0]);
    let class = b"moCompReferenceCurve_c";
    resolved.extend_from_slice(&(class.len() as u16).to_le_bytes());
    resolved.extend_from_slice(class);
    let prefix = resolved.len();
    resolved.resize(prefix + 133, 0);
    resolved[prefix..prefix + 10].copy_from_slice(&[0x2b, 0x80, 0x02, 0, 0, 0, 0, 0, 0, 0]);
    resolved[prefix + 45..prefix + 61].fill(0xff);
    let reference = prefix + 81;
    resolved[reference..reference + 4].copy_from_slice(&119u32.to_le_bytes());
    resolved[reference + 4..reference + 8].copy_from_slice(&0x5edf_5674u32.to_le_bytes());
    resolved[reference + 16..reference + 20].copy_from_slice(&0x65u32.to_le_bytes());
    resolved[reference + 24..reference + 28].fill(0xff);
    for offset in [reference + 32, reference + 36, reference + 40] {
        resolved[offset..offset + 4].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff]);
    }
    resolved[reference + 48..reference + 52].copy_from_slice(&[0xf8, 0x2a, 0, 0]);
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let helix = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Helix1"))
        .unwrap();
    let sweep = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Surface-Sweep1"))
        .unwrap();
    assert!(matches!(
        &sweep.definition,
        FeatureDefinition::Sweep {
            section: cadmpeg_ir::features::SweepSection::Profile(ProfileRef::Feature(feature)),
            ..
        } if feature == &helix.id
    ));
    assert!(sweep.dependencies.contains(&helix.id));

    let mut changed_profile = decoded.ir().clone();
    let FeatureDefinition::Sweep { section, .. } = &mut changed_profile
        .model
        .features
        .iter_mut()
        .find(|feature| feature.name.as_deref() == Some("Surface-Sweep1"))
        .unwrap()
        .definition
    else {
        unreachable!("typed surface sweep");
    };
    *section = cadmpeg_ir::features::SweepSection::Profile(ProfileRef::Native("other".into()));
    let error = crate::test_support::plan_inherited_write(
        &changed_profile,
        decoded.source_fidelity(),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(error
        .to_string()
        .contains("changes a reference-curve sweep profile"));

    decoded
        .ir_mut()
        .model
        .features
        .iter_mut()
        .find(|feature| feature.name.as_deref() == Some("Surface-Sweep1"))
        .unwrap()
        .name = Some("Renamed surface sweep".into());
    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        regenerated
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some("Renamed surface sweep"))
            .map(|feature| &feature.definition),
        Some(FeatureDefinition::Sweep {
            section: cadmpeg_ir::features::SweepSection::Profile(ProfileRef::Feature(_)),
            ..
        })
    ));
}

#[test]
fn decode_projects_generated_surface_sweep_profile_path() {
    use cadmpeg_ir::features::{FeatureDefinition, ProfileRef};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Feature Name="Surface-Sweep1" Type="Surface-Sweep" id="137"/>
            <Feature Name="Surface-Sweep2" Type="Surface-Sweep" id="211"/>
        </Keywords>"#,
    ));
    let mut resolved =
        resolved_feature_classes_with_ids(&[("moSweepRefSurface_c", "Surface-Sweep1", 137)]);
    resolved.extend_from_slice(&[0xdd, 0x94, 0xff, 0xff, 1, 0]);
    let class = b"moCompReferenceCurve_c";
    resolved.extend_from_slice(&(class.len() as u16).to_le_bytes());
    resolved.extend_from_slice(class);
    resolved.extend_from_slice(&[0x2b, 0x80, 0x02, 0, 0, 0, 0, 0, 0, 0]);
    resolved.extend(resolved_feature_classes_with_ids(&[(
        "moSweepRefSurface_c",
        "Surface-Sweep2",
        211,
    )]));
    let wrapper = resolved.len();
    resolved.extend_from_slice(&[0xdd, 0x94, 0xa3, 0x92, 0x2b, 0x80, 0x02, 0, 0, 4, 0, 0]);
    let marker = resolved.len() + 12;
    resolved.resize(marker + 18, 0);
    resolved[marker - 12..marker - 8].copy_from_slice(&2u32.to_le_bytes());
    resolved[marker - 8..marker - 4].copy_from_slice(&[4, 2, 0, 0]);
    resolved[marker..marker + 16].copy_from_slice(&[
        0x7d, 0xc3, 0x94, 0x25, 0xad, 0x49, 0xb2, 0x54, 0x7d, 0xc3, 0x94, 0x25, 0xad, 0x49, 0xb2,
        0x54,
    ]);
    let entry = marker + 18;
    resolved.resize(entry + 32, 0);
    resolved[entry..entry + 2].copy_from_slice(&0x8c20u16.to_le_bytes());
    resolved[entry + 4..entry + 8].copy_from_slice(&[0x34, 0x80, 0x37, 0]);
    resolved[entry + 8..entry + 12].copy_from_slice(&137u32.to_le_bytes());
    resolved[entry + 12..entry + 16].copy_from_slice(&0x5edf_56e2u32.to_le_bytes());
    resolved[entry + 16..entry + 20].copy_from_slice(&7u32.to_le_bytes());
    resolved[entry + 28..entry + 32].copy_from_slice(&[0xf8, 0x2a, 0, 0]);
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let first = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Surface-Sweep1"))
        .unwrap();
    let second = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Surface-Sweep2"))
        .unwrap();
    assert!(matches!(
        &second.definition,
        FeatureDefinition::Sweep {
            section: cadmpeg_ir::features::SweepSection::Profile(ProfileRef::Generated {
                curves,
                native,
            }),
            ..
        } if curves.len() == 1
            && curves[0].feature == first.id
            && curves[0].local_id == "7"
            && native.ends_with(&wrapper.to_string())
    ));
    assert!(second.dependencies.contains(&first.id));
}

#[test]
fn decode_retains_e1_feature_input_operands() {
    let mut payload = resolved_features_payload(&[0, 1, 2]);
    let mut replacements = 0;
    for index in 0..payload.len().saturating_sub(1) {
        if payload[index..index + 2] == [0xd6, 0x80] {
            payload[index] = 0xe1;
            replacements += 1;
        }
    }
    assert_eq!(replacements, 2);
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &payload,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(decoded.ir());
    let scalar = &native.feature_input_lanes[0].scalars[0];
    assert!(native.feature_input_lanes[0]
        .references
        .iter()
        .all(|reference| reference.kind == crate::records::FeatureInputOperandKind::E1));
    assert!(scalar.entity_indices.is_empty());
    assert_eq!(
        scalar
            .operands
            .iter()
            .map(|operand| (operand.kind, operand.entity_index))
            .collect::<Vec<_>>(),
        [
            (crate::records::FeatureInputOperandKind::E1, 0),
            (crate::records::FeatureInputOperandKind::E1, 2),
        ]
    );
}

#[test]
fn decode_resolves_feature_input_operands_by_compatible_ordinal() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_features_payload_with_names(&[0, 0, 2], &["Sketch1", "D1"]),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let feature_ref = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Sketch1"))
        .and_then(|feature| feature.native_ref.as_deref())
        .expect("native sketch feature");
    let native = sldprt_native(decoded.ir());
    let lane = &native.feature_input_lanes[0];
    let scalar = &lane.scalars[0];
    assert!(lane
        .references
        .iter()
        .all(|reference| reference.feature_ref.as_deref() == Some(feature_ref)));
    assert_eq!(scalar.operands[0].entity_index, 0);
    assert_eq!(
        scalar.operands[0].entity_ref.as_deref(),
        Some(lane.sketch_entities[0].id.as_str())
    );
    assert_eq!(scalar.operands[1].entity_index, 2);
    assert_eq!(scalar.operands[1].entity_ref, None);

    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut Vec::new(),
    )
    .unwrap();
}

#[test]
fn decode_projects_unambiguous_resolved_feature_parameter() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Boss-Extrude1" Type="BossExtrude"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_features_payload(&[0]),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let feature = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Boss-Extrude1"))
        .expect("projected extrusion feature");
    let cadmpeg_ir::features::FeatureDefinition::Extrude { extent, .. } = &feature.definition
    else {
        panic!("typed extrusion feature");
    };
    assert_eq!(
        extent,
        &cadmpeg_ir::features::ExtrudeExtent::OneSided {
            side: cadmpeg_ir::features::ExtrudeSide {
                termination: cadmpeg_ir::features::Termination::Blind {
                    length: cadmpeg_ir::features::Length(25.0),
                },
                draft: None,
                offset: None,
            }
        }
    );
    let parameter = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.owner.as_ref() == Some(&feature.id) && parameter.name == "D1")
        .expect("projected D1 parameter");
    assert_eq!(parameter.expression, "25mm");
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(25.0)
        ))
    );
    assert!(parameter
        .native_ref
        .as_deref()
        .is_some_and(|id| id.starts_with("sldprt:feature-input:scalar#")));
    let native = sldprt_native(decoded.ir());
    let scalar = native.feature_input_lanes[0]
        .scalars
        .iter()
        .find(|scalar| Some(scalar.id.as_str()) == parameter.native_ref.as_deref())
        .expect("parameter scalar");
    assert_eq!(scalar.feature_ref.as_deref(), feature.native_ref.as_deref());
    assert_eq!(
        native.feature_input_lanes[0].relation_bindings[0].scalar_ref,
        scalar.id
    );
    assert_eq!(
        native.feature_input_lanes[0].relation_bindings[0]
            .feature_ref
            .as_deref(),
        feature.native_ref.as_deref()
    );
}

#[test]
fn decode_does_not_project_ambiguous_resolved_feature_parameter() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Boss-Extrude1" Type="BossExtrude"/></Keywords>"#,
    ));
    let mut payload = resolved_features_payload(&[0]);
    payload.extend_from_slice(&[0x04, 0x80, 0xff, 0xfe, 0xff, 2]);
    payload.extend_from_slice(&[b'D', 0, b'1', 0]);
    payload.extend_from_slice(&[
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00,
        0x00, 0xff, 0xfe, 0xff, 0x00, 0x00, 0x00,
    ]);
    payload.extend_from_slice(&0.050f64.to_le_bytes());
    payload.extend_from_slice(&[
        0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00,
    ]);
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &payload,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(!decoded
        .ir()
        .model
        .parameters
        .iter()
        .any(|parameter| parameter.name == "D1"));
}

#[test]
fn decode_projects_unambiguous_resolved_sketch_parameter() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_features_payload_with_names(&[0], &["Sketch1", "D1"]),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let feature = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Sketch1"))
        .expect("projected sketch feature");
    assert!(matches!(
        feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Sketch { .. }
    ));
    let parameter = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.owner.as_ref() == Some(&feature.id) && parameter.name == "D1")
        .expect("projected sketch D1 parameter");
    assert_eq!(parameter.expression, "25mm");
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(25.0)
        ))
    );
    assert!(parameter
        .native_ref
        .as_deref()
        .is_some_and(|id| id.starts_with("sldprt:feature-input:scalar#")));
}
