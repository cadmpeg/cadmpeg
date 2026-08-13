// SPDX-License-Identifier: Apache-2.0
//! Synthetic `.sldprt` byte-fixture tests.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions, Encoder};

use cadmpeg_ir::LossTaxonomy;

use crate::container;
use crate::test_support::*;
use crate::SldprtCodec;

#[test]
fn source_record_join_borrows_the_retained_source_image() {
    let payload = vec![0x5a; 4096];
    let payload_ptr = payload.as_ptr();
    let mut fidelity = cadmpeg_ir::SourceFidelity::default();
    fidelity.retained_records = vec![cadmpeg_ir::source_fidelity::RetainedSourceRecord {
        id: "sldprt:file:source-image#0".into(),
        stream: "source".into(),
        offset: 0,
        byte_len: payload.len() as u64,
        sha256: cadmpeg_ir::hash::sha256_hex(&payload),
        data: Some(payload),
    }];

    let records = crate::source_records(&cadmpeg_ir::examples::unit_cube(), &fidelity).unwrap();
    let retained = records[0].data.expect("retained source bytes");
    assert_eq!(retained.as_ptr(), payload_ptr);
}

#[test]
fn decode_refuses_when_max_entities_is_zero_before_ir_build() {
    use cadmpeg_core::decode::ResourceDimension;

    let mut options = DecodeOptions::default();
    options.policy.limits.max_entities = 0;
    let error = SldprtCodec
        .decode(&mut Cursor::new(synthetic_sldprt()), &options)
        .expect_err("max_entities=0 must refuse at container admission");
    assert!(
        matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::Entities
                    && limit.context.operation == "admit SLDPRT container entities"
        ),
        "{error:?}"
    );
}

#[test]
fn decode_refuses_when_max_entities_is_below_container_cardinality() {
    use cadmpeg_core::decode::ResourceDimension;

    let mut options = DecodeOptions::default();
    options.policy.limits.max_entities = 1;
    let error = SldprtCodec
        .decode(&mut Cursor::new(synthetic_sldprt()), &options)
        .expect_err("max_entities below container cardinality must refuse at admission");
    assert!(
        matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::Entities
        ),
        "{error:?}"
    );
}

#[test]
fn decode_surfaces_preview_and_solidworks_xml_metadata() {
    let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    png.extend_from_slice(&13u32.to_be_bytes());
    png.extend_from_slice(b"IHDR");
    png.extend_from_slice(&640u32.to_be_bytes());
    png.extend_from_slice(&480u32.to_be_bytes());
    png.extend_from_slice(&[8, 6, 0, 0, 1]);
    png.extend_from_slice(&0u32.to_be_bytes());

    let mut bmp = vec![0; 28];
    bmp[4..8].copy_from_slice(&40u32.to_le_bytes());
    bmp[8..12].copy_from_slice(&320i32.to_le_bytes());
    bmp[12..16].copy_from_slice(&(-200i32).to_le_bytes());
    bmp[16..18].copy_from_slice(&1u16.to_le_bytes());
    bmp[18..20].copy_from_slice(&8u16.to_le_bytes());
    bmp[20..24].copy_from_slice(&1u32.to_le_bytes());
    bmp[24..28].copy_from_slice(&12_345u32.to_le_bytes());

    let xml = br#"<?xml version="1.0"?><swSolidWorks swVersion="34000" swCreationTime="1700000000" swPath="C:\part.SLDPRT"><swModel id="1" swName="Part" swConfigurationName="Default"/><swConfigurationList><swConfiguration swID="0" swName="Default" swMostRecentConfiguration="NO" swConfigurationNeedsUpdate="YES" swConfigurationFlags="384" swConfigurationAlternateName="Default derived"/></swConfigurationList></swSolidWorks>"#;
    let mut source = outer_header();
    source.extend(make_block(0x10, "PreviewPNG", &png));
    source.extend(make_block(0x11, "PreviewBMP", &bmp));
    source.extend(make_block(0x12, "SolidWorksMetadata", xml));
    source.extend(make_block(
        0x20,
        "Contents/Config-0-Partition",
        &parasolid_with_body(
            "partition body",
            "SCH_SW_33103_11000",
            &owned_triangle(0, 700, 0.0),
        ),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode metadata fixture");
    let attributes = &decoded
        .ir()
        .source
        .as_ref()
        .expect("source metadata")
        .attributes;
    assert_eq!(attributes["png_preview_count"], "1");
    assert_eq!(attributes["png_preview_0_width"], "640");
    assert_eq!(attributes["png_preview_0_height"], "480");
    assert_eq!(attributes["png_preview_0_color_type"], "6");
    assert_eq!(attributes["bmp_thumbnail_count"], "1");
    assert_eq!(attributes["bmp_thumbnail_0_width"], "320");
    assert_eq!(attributes["bmp_thumbnail_0_height"], "-200");
    assert_eq!(attributes["bmp_thumbnail_0_compression"], "1");
    assert_eq!(attributes["sw_version"], "34000");
    assert_eq!(attributes["sw_creation_time_unix"], "1700000000");
    assert_eq!(attributes["sw_path"], r"C:\part.SLDPRT");
    assert_eq!(attributes["sw_name"], "Part");
    assert_eq!(attributes["sw_configuration_name"], "Default");
    assert_eq!(attributes["sw_configuration_0_needs_update"], "YES");
    assert_eq!(attributes["sw_configuration_0_most_recent"], "NO");
    assert_eq!(attributes["sw_configuration_0_flags"], "384");
    assert_eq!(
        attributes["sw_configuration_0_alternate_name"],
        "Default derived"
    );
}

#[test]
fn decode_without_geometry_falls_back_to_metadata() {
    let f = synthetic_sldprt();
    let mut cur = Cursor::new(f);
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert!(!result.report().geometry_transferred);
    assert_eq!(result.ir().native_unknowns("sldprt").unwrap().len(), 1);
    assert_eq!(result.source_fidelity().retained_records.len(), 2);
    assert!(result
        .source_fidelity()
        .retained_record("sldprt:file:source-image#0")
        .is_some_and(|record| record.data.is_some()));
    assert!(result
        .source_fidelity()
        .retained_records
        .iter()
        .any(|record| record.id != "sldprt:file:source-image#0" && record.sha256.len() == 64));
    let source = result.ir().source.as_ref().expect("source metadata");
    assert_eq!(source.format, "sldprt");
    assert_eq!(
        source
            .attributes
            .get("parasolid_schema")
            .map(String::as_str),
        Some("SCH_SW_33103_11000")
    );
}

#[test]
fn decode_explicit_empty_partition_and_deltas_as_an_empty_model() {
    let source = sldprt_with_partition_and_deltas(&[], &[]);
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();

    assert!(decoded.report().geometry_transferred);
    assert!(decoded.ir().model.bodies.is_empty());
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.message.contains("geometry was not transferred")
            || loss.message.contains("topology graph")
    }));
}

#[test]
fn metadata_fallback_binds_resolved_feature_scalars() {
    let mut source = synthetic_sldprt();
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Fillet Name="Round1" Type="Fillet"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_features_payload_with_names(&[0], &["Round1", "D1"]),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(!decoded.report().geometry_transferred);
    let feature = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Round1"))
        .expect("metadata fillet feature");
    let parameter = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.owner.as_ref() == Some(&feature.id) && parameter.name == "D1")
        .expect("metadata D1 parameter");
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(25.0)
        ))
    );
    assert!(parameter.native_ref.is_some());
    assert!(decoded.report().losses.iter().any(|loss| loss
        .message
        .contains("typed feature(s) retain native or unresolved required operation operands")));
}

#[test]
fn retained_source_image_round_trips_byte_exactly() {
    let source = sldprt_with_body(&triangle_body());
    let mut cur = Cursor::new(source.clone());
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert!(!result.source_fidelity().annotations.provenance.is_empty());
    for coedge in &result.ir().model.coedges {
        assert!(result
            .ir()
            .model
            .coedges
            .iter()
            .any(|candidate| candidate.id == coedge.radial_next));
    }
    cadmpeg_test_support::roundtrip::verbatim_replay_holds(
        &SldprtCodec,
        "retained_source_image_round_trips_byte_exactly",
        &source,
    );
}

#[test]
fn decode_degrades_nonfinite_feature_dimensions() {
    use cadmpeg_ir::features::FeatureDefinition;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Extrusion Name="Extrude" Type="BossExtrude" id="1"><Dimension Name="Depth">NaNmm</Dimension></Extrusion>
            <Fillet Name="Fillet" Type="Fillet" id="2"><Dimension Name="Radius">infmm</Dimension></Fillet>
            <Shell Name="Shell" Type="Shell" id="3" Outward="false"><Dimension Name="Thickness">NaNmm</Dimension></Shell>
            <Dome Name="Dome" Type="Dome" id="4" Faces="face:1" Elliptical="false" Reverse="false"><Dimension Name="Height">infmm</Dimension></Dome>
            <Revolve Name="Revolve" Type="Revolve" id="5" AxisOrigin="0mm,0mm,0mm" AxisDirection="0,0,1" Operation="Join"><Dimension Name="Angle">NaNrad</Dimension></Revolve>
        </Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.features.len(), 5);
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::Extrude {
            extent: cadmpeg_ir::features::ExtrudeExtent::OneSided {
                side: cadmpeg_ir::features::ExtrudeSide {
                    termination: cadmpeg_ir::features::Termination::Unresolved,
                    ..
                },
            },
            op: cadmpeg_ir::features::BooleanOp::Join,
            ..
        }
    ));
    assert!(matches!(
        &decoded.ir().model.features[1].definition,
        FeatureDefinition::Fillet {
            ref groups,
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            radius: cadmpeg_ir::features::RadiusSpec::Unresolved {
                form: Some(cadmpeg_ir::features::RadiusForm::Constant),
            },
            ..
        }])
    ));
    assert!(matches!(
        decoded.ir().model.features[2].definition,
        FeatureDefinition::Shell {
            removed_faces: cadmpeg_ir::features::FaceSelection::Unresolved,
            thickness: None,
            outward: Some(false),
            ..
        }
    ));
    assert!(matches!(
        decoded.ir().model.features[3].definition,
        FeatureDefinition::Dome {
            faces: cadmpeg_ir::features::FaceSelection::Native(_),
            height: None,
            elliptical: Some(false),
            reverse: Some(false),
        }
    ));
    assert!(matches!(
        decoded.ir().model.features[4].definition,
        FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction {
                profile: None,
                axis: Some(_),
                extent: None,
                ..
            },
            op: cadmpeg_ir::features::BooleanOp::Join,
        }
    ));
}

#[test]
fn decode_degrades_nonpositive_feature_dimensions() {
    use cadmpeg_ir::features::FeatureDefinition;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Extrusion Name="Extrude" Type="BossExtrude" id="1"><Dimension Name="Depth">0mm</Dimension></Extrusion>
            <Fillet Name="Fillet" Type="Fillet" id="2"><Dimension Name="Radius">-1mm</Dimension></Fillet>
            <Shell Name="Shell" Type="Shell" id="3" Outward="false"><Dimension Name="Thickness">0mm</Dimension></Shell>
            <Dome Name="Dome" Type="Dome" id="4" Faces="face:1" Elliptical="false" Reverse="false"><Dimension Name="Height">-2mm</Dimension></Dome>
            <Hole Name="Hole" Type="Hole" id="5"><Dimension Name="Diameter">0mm</Dimension><Dimension Name="Depth">5mm</Dimension></Hole>
            <Chamfer Name="Chamfer" Type="Chamfer" id="6"><Dimension Name="Distance">-3mm</Dimension></Chamfer>
        </Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.features.len(), 6);
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::Extrude {
            extent: cadmpeg_ir::features::ExtrudeExtent::OneSided {
                side: cadmpeg_ir::features::ExtrudeSide {
                    termination: cadmpeg_ir::features::Termination::Unresolved,
                    ..
                },
            },
            op: cadmpeg_ir::features::BooleanOp::Join,
            ..
        }
    ));
    assert!(matches!(
        &decoded.ir().model.features[1].definition,
        FeatureDefinition::Fillet {
            ref groups,
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            radius: cadmpeg_ir::features::RadiusSpec::Unresolved {
                form: Some(cadmpeg_ir::features::RadiusForm::Constant),
            },
            ..
        }])
    ));
    assert!(matches!(
        decoded.ir().model.features[2].definition,
        FeatureDefinition::Shell {
            removed_faces: cadmpeg_ir::features::FaceSelection::Unresolved,
            thickness: None,
            outward: Some(false),
            ..
        }
    ));
    assert!(matches!(
        decoded.ir().model.features[3].definition,
        FeatureDefinition::Dome {
            faces: cadmpeg_ir::features::FaceSelection::Native(_),
            height: None,
            elliptical: Some(false),
            reverse: Some(false),
        }
    ));
    assert!(matches!(
        decoded.ir().model.features[4].definition,
        FeatureDefinition::Hole {
            kind: cadmpeg_ir::features::HoleKind::Simple,
            diameter: None,
            extent: Some(cadmpeg_ir::features::Termination::Blind {
                length: cadmpeg_ir::features::Length(5.0),
            }),
            ..
        }
    ));
    assert!(matches!(
        &decoded.ir().model.features[5].definition,
        FeatureDefinition::Chamfer {
            ref groups,
            ..
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::ChamferGroup {
            spec: cadmpeg_ir::features::ChamferSpec::Unresolved {
                form: Some(cadmpeg_ir::features::ChamferForm::Distance),
            },
            ..
        }])
    ));
}

#[test]
fn decode_retains_invalid_feature_directions_and_angles_as_native() {
    use cadmpeg_ir::features::{FeatureDefinition, PatternForm, PatternKind};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Feature Name="Seed" Type="NativeSeed" id="1"/>
            <Pattern Name="Pattern" Type="LinearPattern" id="2" Seeds="1" Direction="0,0,0"><Dimension Name="Spacing">2mm</Dimension><Dimension Name="Count">2</Dimension></Pattern>
            <MoveFace Name="Move" Type="MoveFace" id="3" Faces="face:1" Mode="Translate" Direction="0,0,0"><Dimension Name="Distance">2mm</Dimension></MoveFace>
            <Chamfer Name="Chamfer" Type="Chamfer" id="4"><Dimension Name="Distance">2mm</Dimension><Dimension Name="Angle">180deg</Dimension></Chamfer>
            <Revolve Name="Revolve" Type="Revolve" id="5" AxisOrigin="0mm,0mm,0mm" AxisDirection="0,0,1" Operation="Join"><Dimension Name="Angle">-1deg</Dimension></Revolve>
            <Sweep Name="Sweep" Type="Sweep" id="6" Profile="1" Path="1" Operation="Join"><Dimension Name="Scale">inf</Dimension></Sweep>
            <Rib Name="Rib" Type="Rib" id="7" Profile="1" Direction="0,0,0" BothSides="false" Operation="Join"><Dimension Name="Thickness">2mm</Dimension></Rib>
        </Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.features.len(), 7);
    assert!(matches!(
        decoded.ir().model.features[1].definition,
        FeatureDefinition::Pattern {
            pattern: PatternKind::Unresolved {
                form: Some(PatternForm::Linear),
            },
            ..
        }
    ));
    assert!(matches!(
        decoded.ir().model.features[4].definition,
        FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction {
                profile: None,
                axis: Some(_),
                extent: None,
                ..
            },
            op: cadmpeg_ir::features::BooleanOp::Join,
        }
    ));
    assert!(matches!(
        decoded.ir().model.features[6].definition,
        FeatureDefinition::Rib {
            construction: cadmpeg_ir::features::RibConstruction {
                profile: Some(_),
                direction: None,
                thickness: Some(cadmpeg_ir::features::Length(2.0)),
                side: Some(cadmpeg_ir::features::RibSide::OneSided),
                draft: cadmpeg_ir::features::RibDraft::None,
            },
            op: cadmpeg_ir::features::BooleanOp::Join,
        }
    ));
    assert!(matches!(
        &decoded.ir().model.features[3].definition,
        FeatureDefinition::Chamfer {
            ref groups,
            ..
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::ChamferGroup {
            spec: cadmpeg_ir::features::ChamferSpec::Unresolved {
                form: Some(cadmpeg_ir::features::ChamferForm::DistanceAngle),
            },
            ..
        }])
    ));
    for index in [2, 5] {
        assert!(matches!(
            decoded.ir().model.features[index].definition,
            FeatureDefinition::Native { .. }
        ));
    }
}

#[test]
fn decode_preserves_unresolved_active_configuration() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="Default"/><Configuration Name="Manufacturing"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x43,
        "Contents/SolidWorks",
        br#"<?xml version="1.0"?><swSolidWorks swVersion="34000"><swModel swName="Part" swConfigurationName="Missing"/></swSolidWorks>"#,
    ));
    assert_eq!(
        container::active_configuration_index(&container::scan_bytes(&source)),
        None
    );

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();

    assert!(decoded
        .ir()
        .model
        .configurations
        .iter()
        .all(|configuration| configuration.active.is_inactive()));
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.message
            == "active configuration identity is unresolved; 0 of 3 configuration records are active."
    }));
    assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_reports_partition_inferred_configuration() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(decoded.ir().model.configurations.len(), 1);
    assert!(decoded.ir().model.configurations[0].native_ref.is_none());
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.message
            == "1 configuration state(s) are inferred from geometry partitions without native configuration definitions."
    }));
}

#[test]
fn decode_assigns_selected_partition_bodies_to_configuration() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="Default" SourceIndex="0"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.configurations.len(), 1);
    assert!(decoded.ir().model.configurations[0].active.is_active());
    assert_eq!(
        decoded.ir().model.configurations[0].bodies,
        decoded
            .ir()
            .model
            .bodies
            .iter()
            .map(|body| body.id.clone())
            .collect::<Vec<_>>()
    );
    let mut written = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut written)
        .unwrap();
    let round_trip = SldprtCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        round_trip.ir().model.configurations[0].bodies,
        round_trip
            .ir()
            .model
            .bodies
            .iter()
            .map(|body| body.id.clone())
            .collect::<Vec<_>>()
    );
}

#[test]
fn decode_synthesizes_sparse_partition_configuration() {
    let mut source = outer_header();
    source.extend(make_block(
        0x20,
        "Contents/Config-3-Partition",
        &parasolid_with_body("partition body", "SCH_SW_33103_11000", &triangle_body()),
    ));
    assert_eq!(
        container::scan_bytes(&source).blocks[0].section.as_deref(),
        Some("Contents/Config-3-Partition")
    );
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.configurations.len(), 1);
    let configuration = &decoded.ir().model.configurations[0];
    assert_eq!(configuration.ordinal, 0);
    assert_eq!(configuration.source_index, Some(3));
    assert!(configuration.active.is_active());
    assert_eq!(configuration.name, "Config-3");
    assert_eq!(
        configuration.bodies,
        decoded
            .ir()
            .model
            .bodies
            .iter()
            .map(|body| body.id.clone())
            .collect::<Vec<_>>()
    );

    let (mut edited, _, fidelity) = decoded.into_parts();
    edited.model.points[0].position.x += 1.0;
    let mut written = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut written)
        .unwrap();
    let scan = container::scan_bytes(&written);
    assert!(scan
        .blocks
        .iter()
        .any(|block| block.section.as_deref() == Some("Contents/Config-3-Partition")));
    assert!(!scan
        .blocks
        .iter()
        .any(|block| block.section.as_deref() == Some("Contents/Config-0-Partition")));
}

#[test]
fn configuration_source_index_allocation_rejects_exhaustion() {
    let mut used = std::collections::HashSet::from([u32::MAX]);
    let mut next = u32::MAX;
    let error = crate::writer::reserve_configuration_index(&mut used, &mut next).unwrap_err();
    assert!(error.to_string().contains("index space is exhausted"));
}

#[test]
fn decode_encode_is_equivariant_under_rigid_motion() {
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::transform::Transform;

    let motions = [
        (
            [
                [0.0, -1.0, 0.0, 10.0],
                [1.0, 0.0, 0.0, 20.0],
                [0.0, 0.0, 1.0, 30.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
            (|p: Point3| Point3::new(-p.y + 10.0, p.x + 20.0, p.z + 30.0)) as fn(Point3) -> Point3,
        ),
        (
            [
                [1.0, 0.0, 0.0, -5.0],
                [0.0, 0.0, -1.0, 7.0],
                [0.0, 1.0, 0.0, 3.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
            |p: Point3| Point3::new(p.x - 5.0, -p.z + 7.0, p.y + 3.0),
        ),
    ];

    // The `.sldprt` semantic writer refuses a body or face name without a
    // material, so strip the labels the round trip does not exercise here.
    let prepare = |ir: &mut cadmpeg_ir::document::CadIr| {
        ir.model.bodies[0].name = None;
        ir.model.faces.iter_mut().for_each(|face| face.name = None);
        ir.model
            .edges
            .iter_mut()
            .for_each(|edge| edge.param_range = None);
    };

    let mut base = cadmpeg_ir::examples::unit_cube();
    prepare(&mut base);
    base.model.bodies[0].transform = None;
    let mut base_bytes = Vec::new();
    SldprtCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &base,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut base_bytes))
        .unwrap();
    let reference = SldprtCodec
        .decode(&mut Cursor::new(base_bytes), &DecodeOptions::default())
        .unwrap();
    let reference_points: Vec<Point3> = reference
        .ir()
        .model
        .points
        .iter()
        .map(|point| point.position)
        .collect();

    for (rows, apply) in motions {
        let mut moved = cadmpeg_ir::examples::unit_cube();
        prepare(&mut moved);
        moved.model.bodies[0].transform = Some(Transform { rows });
        let mut bytes = Vec::new();
        SldprtCodec
            .plan(cadmpeg_ir::codec::EncodeInput {
                ir: &moved,
                fidelity: None,
            })
            .and_then(|plan| plan.write_to(&mut bytes))
            .unwrap();
        let decoded = SldprtCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();

        for reference_point in &reference_points {
            let expected = apply(*reference_point);
            assert!(
                decoded.ir().model.points.iter().any(|point| {
                    (point.position.x - expected.x).abs() < 1e-9
                        && (point.position.y - expected.y).abs() < 1e-9
                        && (point.position.z - expected.z).abs() < 1e-9
                }),
                "rigid motion not preserved for point {reference_point:?}"
            );
        }
        assert!(decoded
            .ir()
            .model
            .bodies
            .iter()
            .all(|body| body.transform.is_none()));
    }
}

#[test]
fn decode_encode_decode_reaches_fixpoint() {
    let fixture = sldprt_with_body_and_history(&triangle_body());

    let first = SldprtCodec
        .decode(&mut Cursor::new(fixture), &DecodeOptions::default())
        .expect("first decode");
    assert!(first.report().geometry_transferred);

    let mut reencoded = Vec::new();
    SldprtCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: first.ir(),
            fidelity: Some(first.source_fidelity()),
        })
        .and_then(|plan| plan.write_to(&mut reencoded))
        .expect("re-encode");

    let second = SldprtCodec
        .decode(&mut Cursor::new(reencoded), &DecodeOptions::default())
        .expect("second decode");

    assert_eq!(
        first.ir().model.points,
        second.ir().model.points,
        "points diverged at the fixpoint"
    );
    assert_eq!(
        first.ir().model.surfaces,
        second.ir().model.surfaces,
        "surfaces diverged at the fixpoint"
    );
    assert_eq!(
        first.ir().model.faces,
        second.ir().model.faces,
        "faces diverged at the fixpoint"
    );
    assert_eq!(
        first.ir().model.edges,
        second.ir().model.edges,
        "edges diverged at the fixpoint"
    );
    assert_eq!(
        first.ir().model.coedges,
        second.ir().model.coedges,
        "coedges diverged at the fixpoint"
    );
    assert_eq!(
        first.report().geometry_transferred,
        second.report().geometry_transferred,
        "geometry-transferred flag diverged at the fixpoint"
    );
}

#[test]
fn decode_builds_valid_topology_and_plane() {
    use cadmpeg_ir::geometry::SurfaceGeometry;
    use cadmpeg_ir::math::Point3;

    let f = sldprt_with_body(&triangle_body());
    let mut cur = Cursor::new(f);
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert!(result.report().geometry_transferred);
    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.loops.len(), 1);
    assert_eq!(result.ir().model.coedges.len(), 3);
    assert_eq!(result.ir().model.edges.len(), 3);
    assert_eq!(result.ir().model.vertices.len(), 3);
    assert_eq!(result.ir().model.points.len(), 3);
    assert_eq!(result.ir().model.surfaces.len(), 1);

    match &result.ir().model.surfaces[0].geometry {
        SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        } => {
            assert_eq!(*origin, Point3::new(0.0, 0.0, 0.0));
            assert_eq!(normal.z, 1.0);
            assert_eq!(u_axis.x, 1.0);
        }
        other => panic!("expected plane, got {other:?}"),
    }

    let xs: Vec<f64> = result
        .ir()
        .model
        .points
        .iter()
        .map(|p| p.position.x)
        .collect();
    assert!(xs.contains(&1000.0));

    let report = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(report.is_ok(), "validation findings: {:?}", report.findings);
    assert_eq!(result.ir().model.loops[0].coedges.len(), 3);
    // Edges carry no analytic curve (their carriers were null), which is legal.
    assert!(result.ir().model.edges.iter().all(|e| e.curve.is_none()));
}

#[test]
fn strict_accepts_operator_requested_container_only() {
    let fixture = synthetic_sldprt();
    let mut options = strict_options();
    options.container_only = true;
    SldprtCodec
        .decode(&mut Cursor::new(fixture), &options)
        .expect("strict container-only decode is accepted");
}

#[test]
fn strict_rejects_unrepresentable_geometry_while_salvage_records_loss_codes() {
    use cadmpeg_ir::report::{LossTaxonomy, StrictConsequence};

    let fixture = synthetic_sldprt();

    let salvaged = SldprtCodec
        .decode(&mut Cursor::new(fixture.clone()), &DecodeOptions::default())
        .expect("salvage decode keeps the partial result");
    assert!(!salvaged.report().geometry_transferred);
    assert!(salvaged
        .report()
        .losses
        .iter()
        .any(|note| note.code.taxonomy() == LossTaxonomy::GeometryNotTransferred));
    assert!(salvaged
        .report()
        .losses
        .iter()
        .any(|note| note.code.taxonomy() == LossTaxonomy::TopologyNotTransferred));
    assert!(salvaged
        .report()
        .losses
        .iter()
        .any(|note| note.strict_consequence() == StrictConsequence::Reject));

    let strict = SldprtCodec.decode(&mut Cursor::new(fixture), &strict_options());
    match strict {
        Err(cadmpeg_core::CodecError::Malformed(message)) => {
            assert!(
                message.contains("strict mode rejects sldprt/"),
                "unexpected message: {message}"
            );
        }
        other => panic!("strict decode must reject unrepresentable geometry, got {other:?}"),
    }
}

#[test]
fn strict_accepts_tolerable_gauge_substitution_geometry() {
    use cadmpeg_ir::report::{LossTaxonomy, StrictConsequence};

    let fixture = sldprt_with_body_and_history(&triangle_body());
    let strict = SldprtCodec
        .decode(&mut Cursor::new(fixture), &strict_options())
        .expect("strict decode accepts a tolerable-loss geometry result");
    assert!(strict.report().geometry_transferred);
    assert!(strict
        .report()
        .losses
        .iter()
        .all(|note| note.strict_consequence() == StrictConsequence::Tolerate));
    assert!(strict
        .report()
        .losses
        .iter()
        .any(|note| note.code.taxonomy() == LossTaxonomy::TopologyGaugeSubstituted));
}

#[test]
fn decode_does_not_report_derived_pcurves_as_stored_geometry_loss() {
    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result
        .report()
        .losses
        .iter()
        .all(|loss| !loss.message.contains("curve-on-surface")));
}

#[test]
fn decode_merges_partition_and_deltas_records() {
    let body = triangle_body();
    let split = body.len() / 2;
    let f = sldprt_with_partition_and_deltas(&body[..split], &body[split..]);
    let mut cur = Cursor::new(f);

    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert!(result.report().geometry_transferred);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.points.len(), 3);
}

#[test]
fn decode_deduplicates_partition_and_deltas_face_bindings() {
    use cadmpeg_ir::appearance::AppearanceTarget;

    let mut partition = Vec::new();
    partition.extend(entity51(1, 700, 0x0015, &[0, 0, 0, 0, 0, 900]));
    partition.extend(entity53_color(900, [0.25, 0.5, 0.75]));
    partition.extend(owned_triangle(0, 700, 0.0));
    let mut deltas = Vec::new();
    deltas.extend(entity51(1, 700, 0x0015, &[0, 0, 0, 0, 0, 900]));
    deltas.extend(entity53_color(900, [0.25, 0.5, 0.75]));

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_partition_and_deltas(&partition, &deltas)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.appearances.len(), 1);
    assert_eq!(
        result
            .ir()
            .model
            .appearance_bindings
            .iter()
            .filter(|binding| matches!(binding.target, AppearanceTarget::Face(_)))
            .count(),
        1
    );
    assert!(cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).is_ok());
}

#[test]
fn decode_merges_colliding_configuration_sites_with_disjoint_identities() {
    let mut cur = Cursor::new(sldprt_with_colliding_sites());
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.faces.len(), 2);
    assert!(result
        .ir()
        .model
        .points
        .iter()
        .any(|point| point.position.x == 0.0));
    assert!(result
        .ir()
        .model
        .points
        .iter()
        .any(|point| point.position.x == 10_000.0));
    let ids: std::collections::HashSet<_> = result
        .ir()
        .model
        .points
        .iter()
        .map(|point| &point.id)
        .collect();
    assert_eq!(ids.len(), result.ir().model.points.len());
    assert!(result
        .ir()
        .model
        .points
        .iter()
        .all(|point| point.id.0.contains("@block@")));
    let report = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(report.is_ok(), "validation findings: {:?}", report.findings);
}

#[test]
fn decode_uses_the_active_configuration_source_site() {
    let mut source = sldprt_with_colliding_sites();
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="First" SourceIndex="0"/><Configuration Name="Second" SourceIndex="1"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x43,
        "Contents/SolidWorks",
        br#"<swSolidWorks><swModel swConfigurationName="Second"/></swSolidWorks>"#,
    ));

    let result = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();

    let active_points = result
        .ir()
        .model
        .points
        .iter()
        .filter(|point| !point.id.0.contains("@block@"))
        .collect::<Vec<_>>();
    assert_eq!(active_points.len(), 3);
    assert!(active_points
        .iter()
        .all(|point| point.position.x >= 10_000.0));
    assert_eq!(
        result.ir().source.as_ref().unwrap().attributes["active_parasolid_block"],
        "Contents/Config-1-Partition"
    );
    assert!(cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).is_ok());
}

#[test]
fn merged_opaque_geometry_retains_its_owning_site() {
    use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};

    let mut source = outer_header();
    source.extend(make_block(
        0x20,
        "Contents/Config-0-Partition",
        &parasolid_with_body(
            "partition body",
            "SCH_SW_33103_11000",
            &untyped_triangle(0.0),
        ),
    ));
    source.extend(make_block(
        0x21,
        "Contents/Config-1-Partition",
        &parasolid_with_body(
            "partition body",
            "SCH_SW_33103_11000",
            &untyped_triangle(10.0),
        ),
    ));
    let expected_records = container::scan_bytes(&source)
        .blocks
        .iter()
        .map(|block| cadmpeg_ir::ids::UnknownId(format!("sldprt:file:block#{}", block.offset)))
        .collect::<std::collections::BTreeSet<_>>();

    let result = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();

    let surface_bindings = result
        .ir()
        .model
        .surfaces
        .iter()
        .map(|surface| {
            let SurfaceGeometry::Unknown {
                record: Some(record),
            } = &surface.geometry
            else {
                panic!("site surface is not bound to opaque source bytes");
            };
            (surface.id.0.clone(), record.clone())
        })
        .collect::<Vec<_>>();
    let curve_bindings = result
        .ir()
        .model
        .curves
        .iter()
        .map(|curve| {
            let CurveGeometry::Unknown {
                record: Some(record),
            } = &curve.geometry
            else {
                panic!("site curve is not bound to opaque source bytes");
            };
            (curve.id.0.clone(), record.clone())
        })
        .collect::<Vec<_>>();
    assert_eq!(surface_bindings.len(), 2);
    assert_eq!(curve_bindings.len(), 2);
    assert_eq!(
        surface_bindings
            .iter()
            .map(|(_, record)| record.clone())
            .collect::<std::collections::BTreeSet<_>>(),
        expected_records
    );
    assert_eq!(
        curve_bindings
            .iter()
            .map(|(_, record)| record.clone())
            .collect::<std::collections::BTreeSet<_>>(),
        expected_records
    );
    let unknowns = result.ir().native_unknowns("sldprt").unwrap();
    for (geometry, record) in surface_bindings.into_iter().chain(curve_bindings) {
        assert!(unknowns
            .iter()
            .find(|unknown| unknown.id == record)
            .is_some_and(|unknown| unknown.links.contains(&geometry)));
    }
    assert!(cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).is_ok());
}

#[test]
fn deltas_full_record_overrides_partition_record() {
    let partition = triangle_body();
    let deltas = world_point(60, [2.0, 0.0, 0.0]);
    let f = sldprt_with_partition_and_deltas(&partition, &deltas);
    let mut cur = Cursor::new(f);

    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    let point = result
        .ir()
        .model
        .points
        .iter()
        .find(|point| point.id.0.ends_with("#60"))
        .expect("overridden point");

    assert_eq!(point.position.x, 2000.0);
}

#[test]
fn partition_topology_wins_when_deltas_reuse_a_bridge_identity() {
    let partition = triangle_body();
    let deltas = bridge_owned(10, 120, 200, 700);
    let partition_payload = parasolid_with_body("partition body", "SCH_SW_33103_11000", &partition);
    let deltas_payload = parasolid_with_body("deltas body", "SCH_SW_33103_11000", &deltas);
    let partition_header = crate::parasolid::stream_header(&partition_payload).unwrap();
    let deltas_header = crate::parasolid::stream_header(&deltas_payload).unwrap();

    let decoded = crate::brep::decode_bodies(
        &[
            (&deltas_payload, &deltas_header),
            (&partition_payload, &partition_header),
        ],
        "precedence",
    );

    assert_eq!(decoded.faces.len(), 1);
    assert_eq!(decoded.faces[0].id.0, "sldprt:brep:face#10");
    assert_eq!(decoded.faces[0].surface.0, "sldprt:brep:surf#10");
}

#[test]
fn decode_reports_and_withholds_faces_without_body_membership() {
    let mut body = owned_triangle(0, 700, 0.0);
    body.extend(owned_triangle(200, 701, 2.0));
    body.extend(entity51(2, 500, 0x0017, &[10, 0, 0, 0, 0, 0, 1]));
    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.faces[0].id.0, "sldprt:brep:face#10");
    assert!(result.report().losses.iter().any(|loss| {
        loss.code.taxonomy() == LossTaxonomy::TopologyNotTransferred
            && loss
                .message
                .contains("not claimed by an explicit body relation")
    }));
}

#[test]
fn class_root_index_selects_complete_cluster_body_relation() {
    let mut body = class_root_index(&[5, 32, 36, 500, 510, 520, 700, 701]);
    body.extend(entity51(2, 5, 0x0004, &[3, 32, 1, 1, 1, 1, 1]));
    body.extend(entity51(2, 32, 0x000f, &[3, 36, 5, 1, 1, 1, 1]));
    body.extend(entity51(2, 36, 0x0011, &[3, 1, 32, 1, 1, 1, 1]));
    body.extend(entity51(2, 500, 0x001a, &[510, 1, 1, 1, 1, 1, 1]));
    body.extend(entity51(2, 510, 0x0016, &[520, 1, 1, 1, 1, 1, 1]));
    body.extend(entity51(2, 520, 0x0020, &[1, 1, 700, 520, 1, 1, 1]));
    body.extend(entity51(1, 700, 0x0014, &[10, 1, 1, 1, 1, 1]));
    body.extend(entity51(1, 701, 0x0014, &[210, 1, 1, 1, 1, 1]));
    body.extend(owned_triangle(0, 700, 0.0));
    body.extend(owned_triangle(200, 701, 2.0));

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.bodies[0].id.0, "sldprt:brep:body#32");
    assert_eq!(result.ir().model.faces.len(), 2);
    assert!(result
        .report()
        .losses
        .iter()
        .all(|loss| loss.code.taxonomy() != LossTaxonomy::TopologyNotTransferred));
    assert!(cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).is_ok());
}

#[test]
fn class_root_body_relation_selects_missing_deltas_face() {
    let mut partition = class_root_index(&[5, 32, 36, 700]);
    partition.extend(entity51(2, 5, 0x0004, &[3, 32, 1, 1, 1, 1, 1]));
    partition.extend(entity51(2, 32, 0x000f, &[3, 36, 5, 1, 1, 1, 1]));
    partition.extend(entity51(2, 36, 0x0011, &[3, 1, 32, 1, 1, 1, 1]));
    partition.extend(entity51(1, 700, 0x0014, &[10, 1, 1, 1, 1, 1]));
    partition.extend(owned_triangle(0, 700, 0.0));

    let mut deltas = vec![0x00, 0x51];
    be32(&mut deltas, 2);
    be16(&mut deltas, 500);
    be32(&mut deltas, 1);
    be16(&mut deltas, 0x0017);
    for reference in [700, 701, 1, 1, 1, 1, 1] {
        deltas.push(1);
        be16(&mut deltas, reference);
    }
    deltas.push(0);
    deltas.extend(entity51(1, 701, 0x0014, &[210, 1, 1, 1, 1, 1]));
    deltas.extend(owned_triangle(200, 701, 2.0));

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_partition_and_deltas(&partition, &deltas)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.bodies[0].id.0, "sldprt:brep:body#32");
    assert_eq!(result.ir().model.faces.len(), 2);
    assert!(result
        .report()
        .losses
        .iter()
        .all(|loss| loss.code.taxonomy() != LossTaxonomy::TopologyNotTransferred));
    assert!(cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).is_ok());
}

#[test]
fn unselected_deltas_bridges_do_not_enter_partition_membership() {
    let partition = triangle_body();
    let deltas = owned_triangle(200, 900, 10.0);
    let mut cur = Cursor::new(sldprt_with_partition_and_deltas(&partition, &deltas));

    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir().model.faces.len(), 1);
    assert!(result
        .ir()
        .model
        .points
        .iter()
        .all(|point| point.position.x != 10_000.0));
    let report = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(report.is_ok(), "validation findings: {:?}", report.findings);
}

#[test]
fn duplicate_face_uses_emit_one_face() {
    let mut body = triangle_body();
    let first_bridge = body
        .windows(2)
        .position(|w| w == [0x00, 0x0e])
        .expect("bridge");
    body[first_bridge + 8..first_bridge + 10].copy_from_slice(&700u16.to_be_bytes());
    body.extend(bridge_owned(11, 20, 100, 700));
    let f = sldprt_with_body(&body);
    let mut cur = Cursor::new(f);

    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.faces[0].id.0, "sldprt:brep:face#10");
}

#[test]
fn decode_withholds_non_equivalent_face_uses_with_same_owner() {
    let mut body = triangle_body();
    let first_bridge = body
        .windows(2)
        .position(|w| w == [0x00, 0x0e])
        .expect("bridge");
    body[first_bridge + 8..first_bridge + 10].copy_from_slice(&700u16.to_be_bytes());
    body.extend(bridge_owned(11, 20, 200, 700));
    body.extend(owned_triangle(200, 701, 2.0));
    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.faces[0].id.0, "sldprt:brep:face#210");
    assert!(
        result
            .report()
            .losses
            .iter()
            .any(
                |loss| loss.code.taxonomy() == LossTaxonomy::TopologyGaugeSubstituted
                    && loss.message.contains("non-equivalent bridge uses")
            ),
        "losses: {:?}",
        result.report().losses
    );
}

#[test]
fn sheet_body_faces_are_retained_and_classified() {
    let mut body = Vec::new();
    body.extend(entity51(2, 500, 0x0017, &[510, 700, 0, 0, 0, 0]));
    body.extend(entity51(2, 501, 0x0017, &[511, 701, 0, 0, 0, 0]));
    body.extend(entity51(1, 510, 0x001b, &[700, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 511, 0x001d, &[701, 0, 0, 0, 0, 0]));
    body.extend(owned_triangle(0, 700, 0.0));
    body.extend(owned_triangle(200, 701, 10.0));
    let f = sldprt_with_body(&body);
    let mut cur = Cursor::new(f);

    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir().model.bodies.len(), 2);
    assert_eq!(result.ir().model.faces.len(), 2);
    assert_eq!(
        result
            .ir()
            .model
            .bodies
            .iter()
            .filter(|body| body.kind == cadmpeg_ir::topology::BodyKind::Solid)
            .count(),
        1
    );
    assert_eq!(
        result
            .ir()
            .model
            .bodies
            .iter()
            .filter(|body| body.kind == cadmpeg_ir::topology::BodyKind::Sheet)
            .count(),
        1
    );
}

#[test]
fn schema_33103_disc1d_flo2_is_not_a_sheet_region() {
    let mut body = Vec::new();
    body.extend(entity51(2, 501, 0x0017, &[511, 701, 0, 0, 0, 0]));
    body.extend(entity51(2, 511, 0x001d, &[701, 0, 0, 0, 0, 0]));
    body.extend(owned_triangle(0, 701, 0.0));
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(
        decoded.ir().model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Solid
    );
}

/// Phase 5 freeze: export precondition (:50) rejects shared broken IR; empty accepts.
#[test]
fn phase5_freeze_export_precondition_admissibility_fixtures() {
    let accepted = cadmpeg_ir::validate::admissibility_freeze::accepted_empty();
    // Empty IR has no B-rep; writer refuses later for missing B-rep, but the
    // :50 precondition is full validate — empty passes validate.
    assert!(cadmpeg_ir::validate_neutral(&accepted, Vec::new()).is_ok());
    let rejected =
        cadmpeg_ir::validate::admissibility_freeze::rejected_missing_point("sldprt:test");
    assert!(!cadmpeg_ir::validate_neutral(&rejected, Vec::new()).is_ok());
}

#[test]
fn closed_cylinder_gets_derived_seam() {
    use cadmpeg_ir::geometry::CurveGeometry;
    let f = sldprt_with_body(&closed_cylinder_body());
    let mut cur = Cursor::new(f);

    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir().model.faces[0].loops.len(), 1);
    assert_eq!(result.ir().model.loops[0].coedges.len(), 4);
    assert_eq!(result.ir().model.pcurves.len(), 4);
    assert!(result
        .ir()
        .model
        .coedges
        .iter()
        .all(|coedge| !coedge.pcurves.is_empty()));
    assert_eq!(result.ir().model.edges.len(), 3);
    assert!(result
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| matches!(curve.geometry, CurveGeometry::Line { .. })));
}

#[test]
fn closed_cylinder_anchors_sentinel_vertices_to_the_surface_branch() {
    let mut body = closed_cylinder_body();
    for coedge_attr in [30u16, 31] {
        let offset = body
            .windows(4)
            .position(|window| {
                window[0..2] == [0x00, 0x11] && window[2..4] == coedge_attr.to_be_bytes()
            })
            .expect("coedge");
        body[offset + 12..offset + 14].copy_from_slice(&1u16.to_be_bytes());
    }

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    let seam = decoded
        .ir()
        .model
        .edges
        .iter()
        .find(|edge| edge.id.0.contains("#seam:"))
        .expect("derived seam");
    let positions = [&seam.start, &seam.end].map(|vertex_id| {
        let vertex = decoded
            .ir()
            .model
            .vertices
            .iter()
            .find(|vertex| vertex.id == *vertex_id)
            .unwrap();
        decoded
            .ir()
            .model
            .points
            .iter()
            .find(|point| point.id == vertex.point)
            .unwrap()
            .position
    });
    assert_eq!(
        positions[0],
        cadmpeg_ir::math::Point3::new(-1000.0, 0.0, 0.0)
    );
    assert_eq!(
        positions[1],
        cadmpeg_ir::math::Point3::new(-1000.0, 0.0, 1000.0)
    );
}

#[test]
fn closed_circle_edge_gets_a_derived_seam_vertex() {
    let mut body = Vec::new();
    body.extend(plane_carrier(
        100,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    ));
    body.extend(circle_carrier(200, [1.0, 2.0, 0.0], [0.0, 0.0, 1.0], 0.5));
    body.extend(bridge(10, 20, 100));
    body.extend(loop_head(20, 30, 10));
    body.extend(coedge(30, 20, 30, 1, 0, 40, false));
    body.extend(edge_use(40, 200));

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(decoded.ir().model.faces.len(), 1);
    assert_eq!(decoded.ir().model.loops[0].coedges.len(), 1);
    let edge = &decoded.ir().model.edges[0];
    assert_eq!(edge.start, edge.end);
    let vertex = decoded
        .ir()
        .model
        .vertices
        .iter()
        .find(|vertex| vertex.id == edge.start)
        .unwrap();
    let point = decoded
        .ir()
        .model
        .points
        .iter()
        .find(|point| point.id == vertex.point)
        .unwrap();
    assert_eq!(
        [point.position.x, point.position.y, point.position.z],
        [1500.0, 2000.0, 0.0]
    );
    assert!(matches!(
        decoded.ir().model.pcurves[0].geometry,
        cadmpeg_ir::geometry::PcurveGeometry::Circle {
            center,
            radius: 500.0,
            y_axis: cadmpeg_ir::math::Point2 { u: 0.0, v: 1.0 },
            ..
        } if center == cadmpeg_ir::math::Point2::new(1000.0, 2000.0)
    ));
    assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new()).is_ok());
}

#[test]
fn oblique_cylinder_section_gets_an_exact_polar_harmonic_pcurve() {
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let mut body = Vec::new();
    body.extend(cylinder_carrier(100, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 1.0));
    body.extend(ellipse_carrier(
        200,
        [0.0, 0.0, 0.0],
        [-s, 0.0, s],
        [s, 0.0, s],
        std::f64::consts::SQRT_2,
        1.0,
    ));
    body.extend(bridge(10, 20, 100));
    body.extend(loop_head(20, 30, 10));
    body.extend(coedge(30, 20, 30, 50, 0, 40, false));
    body.extend(edge_use(40, 200));
    body.extend(vertex_use(50, 60));
    body.extend(world_point(60, [1.0, 0.0, 1.0]));

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(decoded.ir().model.pcurves.len(), 1);
    assert!(matches!(
        decoded.ir().model.pcurves[0].geometry,
        cadmpeg_ir::geometry::PcurveGeometry::PolarHarmonic {
            radial_center,
            radial_cos,
            radial_sin,
            axial_origin: 0.0,
            axial_sin: 0.0,
            ..
        } if radial_center == cadmpeg_ir::math::Point2::new(0.0, 0.0)
            && (radial_cos.u - 1000.0).abs() < 1e-9
            && radial_cos.v.abs() < 1e-9
            && radial_sin.u.abs() < 1e-9
            && (radial_sin.v - 1000.0).abs() < 1e-9
    ));
    assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new()).is_ok());
}

#[test]
fn coaxial_cone_circle_preserves_parameter_direction() {
    let mut body = Vec::new();
    body.extend(cone_carrier(
        100,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        1.0,
        std::f64::consts::FRAC_PI_4,
        [1.0, 0.0, 0.0],
    ));
    body.extend(circle_carrier(200, [0.0, 0.0, 1.0], [0.0, 0.0, -1.0], 2.0));
    body.extend(bridge(10, 20, 100));
    body.extend(loop_head(20, 30, 10));
    body.extend(coedge(30, 20, 30, 50, 0, 40, false));
    body.extend(edge_use(40, 200));
    body.extend(vertex_use(50, 60));
    body.extend(world_point(60, [2.0, 0.0, 1.0]));

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let cadmpeg_ir::geometry::PcurveGeometry::Line { origin, direction } =
        decoded.ir().model.pcurves[0].geometry
    else {
        panic!("expected line pcurve");
    };
    assert!(origin.u.abs() < 1e-12);
    assert!((origin.v - 1000.0).abs() < 1e-9);
    assert_eq!(direction, cadmpeg_ir::math::Point2::new(-1.0, 0.0));
    assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new()).is_ok());
}

#[test]
fn coaxial_torus_circle_gets_constant_minor_angle_pcurve() {
    let mut body = Vec::new();
    body.extend(torus_carrier(
        100,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        2.0,
        1.0,
        [1.0, 0.0, 0.0],
    ));
    body.extend(circle_carrier(200, [0.0, 0.0, 1.0], [0.0, 0.0, 1.0], 2.0));
    body.extend(bridge(10, 20, 100));
    body.extend(loop_head(20, 30, 10));
    body.extend(coedge(30, 20, 30, 50, 0, 40, false));
    body.extend(edge_use(40, 200));
    body.extend(vertex_use(50, 60));
    body.extend(world_point(60, [2.0, 0.0, 1.0]));

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let cadmpeg_ir::geometry::PcurveGeometry::Line { origin, direction } =
        decoded.ir().model.pcurves[0].geometry
    else {
        panic!("expected line pcurve");
    };
    assert!(origin.u.abs() < 1e-12);
    assert!((origin.v - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    assert_eq!(direction, cadmpeg_ir::math::Point2::new(1.0, 0.0));
    assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new()).is_ok());
}

#[test]
fn sphere_patch_gets_degenerate_meridian_seam() {
    let mut cur = Cursor::new(sldprt_with_body(&sphere_patch_body()));
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.edges.len(), 4);
    assert_eq!(result.ir().model.vertices.len(), 3);
    assert_eq!(result.ir().model.points.len(), 3);
    assert_eq!(result.ir().model.loops[0].coedges.len(), 4);
    assert_eq!(result.ir().model.pcurves.len(), 4);
    let pole = result
        .ir()
        .model
        .pcurves
        .iter()
        .find(|pcurve| pcurve.id.0.contains("sphere-seam"))
        .expect("sphere pole pcurve");
    assert!(matches!(
        pole.geometry,
        cadmpeg_ir::geometry::PcurveGeometry::Line { origin, direction }
            if origin == cadmpeg_ir::math::Point2::new(0.0, std::f64::consts::FRAC_PI_2)
                && direction == cadmpeg_ir::math::Point2::new(1.0, 0.0)
    ));
    assert_eq!(pole.parameter_range, Some([0.0, std::f64::consts::TAU]));
    let seam = result
        .ir()
        .model
        .edges
        .iter()
        .find(|edge| {
            result
                .source_fidelity()
                .annotations
                .provenance
                .get(&edge.id.0)
                .and_then(|note| note.tag.as_deref())
                == Some("derived_sphere_seam")
        })
        .expect("sphere seam");
    assert_eq!(seam.start, seam.end);
    let curve = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| seam.curve.as_ref() == Some(&curve.id))
        .expect("sphere seam curve");
    assert!(matches!(
        curve.geometry,
        cadmpeg_ir::geometry::CurveGeometry::Degenerate { point }
            if point == cadmpeg_ir::math::Point3::new(0.0, 0.0, 1000.0)
    ));
    let vertex = result
        .ir()
        .model
        .vertices
        .iter()
        .find(|vertex| vertex.id == seam.start)
        .unwrap();
    let point = result
        .ir()
        .model
        .points
        .iter()
        .find(|point| point.id == vertex.point)
        .unwrap();
    assert_eq!(
        [point.position.x, point.position.y, point.position.z],
        [0.0, 0.0, 1000.0]
    );
}

#[test]
fn existing_sphere_seam_endpoint_is_normalized_to_axis_pole() {
    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&sphere_existing_seam_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let seam_curve = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| {
            result
                .source_fidelity()
                .annotations
                .provenance
                .get(&curve.id.0)
                .and_then(|note| note.tag.as_deref())
                == Some("derived_sphere_seam")
        })
        .expect("existing sphere seam curve");
    let seam = result
        .ir()
        .model
        .edges
        .iter()
        .find(|edge| edge.curve.as_ref() == Some(&seam_curve.id))
        .expect("existing sphere seam edge");
    let vertex = result
        .ir()
        .model
        .vertices
        .iter()
        .find(|vertex| vertex.id == seam.start)
        .expect("sphere seam pole vertex");
    let point = result
        .ir()
        .model
        .points
        .iter()
        .find(|point| point.id == vertex.point)
        .expect("sphere seam pole point");

    assert_eq!(
        point.position,
        cadmpeg_ir::math::Point3::new(0.0, 0.0, 1000.0)
    );
}

#[test]
fn decode_recovers_overlapping_topology_records() {
    let f = sldprt_with_body(&triangle_body_with_overlapping_point());
    let mut cur = Cursor::new(f);

    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir().model.points.len(), 3);
    assert_eq!(result.ir().model.vertices.len(), 3);
}

#[test]
fn decode_recovers_tripled_deltas_topology() {
    let mut cur = Cursor::new(sldprt_with_body(&tripled_triangle_body()));
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.coedges.len(), 3);
    assert_eq!(result.ir().model.points.len(), 3);
    assert_eq!(result.ir().model.faces.len(), 1);
}

#[test]
fn decode_resolves_prefixed_deltas_edge_curve() {
    use cadmpeg_ir::geometry::CurveGeometry;
    let mut cur = Cursor::new(sldprt_with_body(&prefixed_edge_triangle_body()));
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert!(result
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| matches!(curve.geometry, CurveGeometry::Line { .. })));
}

#[test]
fn decode_resolves_suffix_prefixed_edge_curve_with_high_byte_one() {
    use cadmpeg_ir::geometry::CurveGeometry;
    let mut cur = Cursor::new(sldprt_with_body(&suffix_prefixed_edge_triangle_body()));
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert!(result
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| matches!(curve.geometry, CurveGeometry::Line { .. })));
}

#[test]
fn decode_preserves_explicit_body_membership() {
    let mut body = Vec::new();
    body.extend(entity51(2, 500, 0x0017, &[700, 0, 0, 0, 0, 0]));
    body.extend(entity51(2, 501, 0x0017, &[701, 0, 0, 0, 0, 0]));
    body.extend(owned_triangle(0, 700, 0.0));
    body.extend(owned_triangle(200, 701, 10.0));
    let f = sldprt_with_body(&body);
    let mut cur = Cursor::new(f);

    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir().model.bodies.len(), 2);
    assert_eq!(result.ir().model.shells.len(), 2);
    assert_eq!(result.ir().model.faces.len(), 2);
    assert_eq!(result.ir().model.bodies[0].id.0, "sldprt:brep:body#500");
    assert_eq!(result.ir().model.bodies[1].id.0, "sldprt:brep:body#501");
}

#[test]
fn decode_preserves_multiple_regions_and_shells_per_body() {
    let mut body = Vec::new();
    body.extend(entity51(2, 500, 0x0017, &[510, 511, 0, 0, 0, 0]));
    body.extend(entity51(1, 510, 0x001b, &[520, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 511, 0x001b, &[521, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 520, 0x001f, &[530, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 521, 0x001f, &[531, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 530, 0x0021, &[700, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 531, 0x0021, &[701, 0, 0, 0, 0, 0]));
    body.extend(owned_triangle(0, 700, 0.0));
    body.extend(owned_triangle(200, 701, 10.0));

    let mut result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.regions.len(), 2);
    assert_eq!(result.ir().model.shells.len(), 2);
    assert_eq!(result.ir().model.bodies[0].regions.len(), 2);
    assert!(result
        .ir()
        .model
        .regions
        .iter()
        .all(|region| region.shells.len() == 1));
    assert!(result
        .ir()
        .model
        .shells
        .iter()
        .all(|shell| shell.faces.len() == 1));
    let report = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(report.is_ok(), "validation findings: {:?}", report.findings);

    result.ir_mut().model.points[0].position.z += 1.0;
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(result.ir(), result.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(regenerated.ir().model.bodies.len(), 1);
    assert_eq!(regenerated.ir().model.regions.len(), 2);
    assert_eq!(regenerated.ir().model.shells.len(), 2);
    assert!(regenerated
        .ir()
        .model
        .shells
        .iter()
        .all(|shell| shell.faces.len() == 1));
}

#[test]
fn decode_follows_connector_region_lump_and_shell_chain() {
    let mut body = Vec::new();
    body.extend(entity51(2, 500, 0x0017, &[510, 0, 0, 0, 0, 0]));
    body.extend(entity51(2, 510, 0x0019, &[0, 520, 0, 0, 0, 0]));
    body.extend(entity51(1, 520, 0x001b, &[530, 0, 0, 0, 0, 0]));
    body.extend(entity51(2, 530, 0x001f, &[540, 0, 0, 0, 0, 0]));
    body.extend(entity51(2, 540, 0x0021, &[550, 0, 0, 0, 0, 0]));
    body.extend(entity51(2, 550, 0x0023, &[700, 0, 0, 0, 0, 0]));
    body.extend(owned_triangle(0, 700, 0.0));

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.regions[0].id.0, "sldprt:brep:region#520");
    assert_eq!(decoded.ir().model.shells[0].id.0, "sldprt:brep:shell#550");
    assert_eq!(decoded.ir().model.shells[0].faces.len(), 1);
    assert_eq!(
        decoded.ir().model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Solid
    );
}

#[test]
fn decode_binds_schema_32001_face_intervals_through_bridge_ids() {
    let mut body = Vec::new();
    body.extend(entity51(2, 500, 0x0017, &[0, 510, 600, 0, 0, 0]));
    body.extend(entity51(2, 510, 0x001b, &[520, 0, 0, 0, 0, 0]));
    body.extend(entity51(2, 520, 0x001f, &[530, 0, 0, 0, 0, 0]));
    body.extend(entity51(2, 530, 0x0021, &[540, 0, 0, 0, 0, 0]));
    body.extend(entity51(2, 540, 0x0023, &[0, 0, 0, 0, 0, 0]));
    body.extend(entity51(2, 600, 0x0015, &[0, 500, 0, 0, 0, 0]));
    body.extend(entity51(1, 700, 0x001f, &[10, 0, 0, 0, 0, 0]));
    body.extend(owned_triangle(0, 900, 0.0));

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(decoded.report().geometry_transferred);
    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.faces.len(), 1);
    assert_eq!(
        decoded.ir().model.shells[0].faces[0].0,
        "sldprt:brep:face#10"
    );
}

#[test]
fn decode_partitions_interleaved_schema_33103_faces_by_adjacency() {
    let mut body = Vec::new();
    body.extend(entity51(2, 500, 0x0017, &[90, 510, 0, 0, 0, 0]));
    body.extend(entity51(2, 501, 0x0017, &[91, 511, 0, 0, 0, 0]));
    body.extend(entity51(2, 510, 0x0019, &[90, 520, 0, 0, 0, 0]));
    body.extend(entity51(2, 511, 0x0019, &[91, 521, 0, 0, 0, 0]));
    for (region, lump, shell_link, shell) in [(520, 530, 540, 550), (521, 531, 541, 551)] {
        body.extend(entity51(1, region, 0x001b, &[lump, 0, 0, 0, 0, 0]));
        body.extend(entity51(2, lump, 0x001f, &[shell_link, 0, 0, 0, 0, 0]));
        body.extend(entity51(2, shell_link, 0x0021, &[shell, 0, 0, 0, 0, 0]));
        body.extend(entity51(2, shell, 0x0023, &[0, 0, 0, 0, 0, 0]));
    }
    body.extend(entity51(2, 600, 0x0013, &[90, 500, 0, 0, 0, 0]));
    body.extend(entity51(1, 700, 0x0015, &[701, 0, 0, 0, 0, 0]));
    body.extend(entity51(2, 601, 0x0013, &[91, 501, 0, 0, 0, 0]));
    body.extend(entity51(1, 800, 0x0015, &[801, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 701, 0x0015, &[700, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 801, 0x0015, &[800, 0, 0, 0, 0, 0]));
    body.extend(owned_triangle(0, 700, 0.0));
    body.extend(owned_triangle(200, 701, 2.0));
    body.extend(owned_triangle(400, 800, 10.0));
    body.extend(owned_triangle(600, 801, 12.0));

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(decoded.ir().model.shells.len(), 4);
    assert!(decoded
        .ir()
        .model
        .shells
        .iter()
        .all(|shell| shell.faces.len() == 1));
    for (native_shell, face_suffixes) in [(550, ["#10", "#210"]), (551, ["#410", "#610"])] {
        let prefix = format!("sldprt:brep:shell#{native_shell}");
        let faces = decoded
            .ir()
            .model
            .shells
            .iter()
            .filter(|shell| shell.id.0.starts_with(&prefix))
            .flat_map(|shell| &shell.faces)
            .collect::<Vec<_>>();
        assert_eq!(faces.len(), 2);
        assert!(face_suffixes
            .iter()
            .all(|suffix| faces.iter().any(|face| face.0.ends_with(suffix))));
    }
}

#[test]
fn decode_partitions_disc14_faces_by_native_shell_rings() {
    let mut body = Vec::new();
    body.extend(entity51(1, 900, 0x001a, &[500, 501, 0, 0, 0, 0]));
    body.extend(entity51(1, 500, 0x0016, &[550, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 501, 0x0016, &[602, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 550, 0x0012, &[600, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 600, 0x0020, &[0, 0, 609, 601, 0, 0]));
    body.extend(entity51(1, 601, 0x0020, &[0, 0, 701, 600, 0, 0]));
    body.extend(entity51(1, 602, 0x0020, &[0, 0, 612, 603, 0, 0]));
    body.extend(entity51(1, 603, 0x0020, &[0, 0, 613, 602, 0, 0]));
    body.extend(entity51(1, 609, 0x001e, &[0, 0, 610, 0, 0, 0]));
    for (geometry, face) in [(610, 700), (611, 701), (612, 800), (613, 801)] {
        body.extend(entity51(1, geometry, 0x0018, &[0, 0, face, 0, 0, 0]));
        body.extend(entity51(1, face, 0x0014, &[0, 0, 0, 0, 0, 0]));
    }
    body.extend(owned_triangle(0, 700, 0.0));
    body.extend(owned_triangle(200, 701, 2.0));
    body.extend(owned_triangle(400, 800, 10.0));
    body.extend(owned_triangle(600, 801, 12.0));

    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.regions.len(), 1);
    assert_eq!(decoded.ir().model.shells.len(), 4);
    assert!(decoded
        .ir()
        .model
        .shells
        .iter()
        .all(|shell| shell.faces.len() == 1));

    decoded.ir_mut().model.points[0].position.z += 1.0;
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(regenerated.ir().model.regions.len(), 1);
    assert_eq!(regenerated.ir().model.shells.len(), 4);
    assert!(regenerated
        .ir()
        .model
        .shells
        .iter()
        .all(|shell| shell.faces.len() == 1));
}

#[test]
fn decode_keeps_multiple_disc14_regions_as_separate_bodies() {
    let mut body = Vec::new();
    body.extend(entity51(1, 900, 0x001a, &[500, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 901, 0x001a, &[501, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 500, 0x0016, &[550, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 501, 0x0016, &[602, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 550, 0x0012, &[600, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 600, 0x0020, &[0, 0, 609, 601, 0, 0]));
    body.extend(entity51(1, 601, 0x0020, &[0, 0, 701, 600, 0, 0]));
    body.extend(entity51(1, 602, 0x0020, &[0, 0, 612, 603, 0, 0]));
    body.extend(entity51(1, 603, 0x0020, &[0, 0, 613, 602, 0, 0]));
    body.extend(entity51(1, 609, 0x001e, &[0, 0, 610, 0, 0, 0]));
    for (geometry, face) in [(610, 700), (611, 701), (612, 800), (613, 801)] {
        body.extend(entity51(1, geometry, 0x0018, &[0, 0, face, 0, 0, 0]));
        body.extend(entity51(1, face, 0x0014, &[0, 0, 0, 0, 0, 0]));
    }
    body.extend(owned_triangle(0, 700, 0.0));
    body.extend(owned_triangle(200, 701, 2.0));
    body.extend(owned_triangle(400, 800, 10.0));
    body.extend(owned_triangle(600, 801, 12.0));

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(decoded.ir().model.bodies.len(), 2);
    assert_eq!(decoded.ir().model.regions.len(), 2);
    for (body_attr, shell_prefix) in [
        (900, "sldprt:brep:shell#500"),
        (901, "sldprt:brep:shell#501"),
    ] {
        let body_id = format!("sldprt:brep:body#{body_attr}");
        let body = decoded
            .ir()
            .model
            .bodies
            .iter()
            .find(|body| body.id.0 == body_id)
            .unwrap();
        assert_eq!(body.regions.len(), 1);
        let region_id = &body.regions[0].0;
        assert_eq!(region_id, &format!("sldprt:brep:region#{body_attr}"));
        let region = decoded
            .ir()
            .model
            .regions
            .iter()
            .find(|region| region.id.0 == *region_id)
            .unwrap();
        assert_eq!(region.body.0, body_id);
        assert!(!region.shells.is_empty());
        assert!(region
            .shells
            .iter()
            .all(|shell| shell.0.starts_with(shell_prefix)));
    }
}

#[test]
fn decode_partitions_disc20_faces_by_native_single_shell_lattice() {
    let mut body = Vec::new();
    body.extend(entity51(2, 900, 0x001a, &[500, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 500, 0x0016, &[0, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 700, 0x0020, &[0, 710, 0, 701, 701, 0]));
    body.extend(entity51(1, 701, 0x0020, &[0, 711, 0, 700, 700, 0]));
    body.extend(entity51(
        4,
        710,
        0x0024,
        &[0, 720, 700, 711, 711, 0, 0, 0, 0],
    ));
    body.extend(entity51(
        4,
        711,
        0x0024,
        &[0, 721, 701, 710, 710, 0, 0, 0, 0],
    ));
    body.extend(entity51(3, 720, 0x0026, &[0, 0, 710, 721, 721, 0]));
    body.extend(entity51(3, 721, 0x0026, &[0, 0, 711, 720, 720, 0]));
    body.extend(owned_triangle(0, 700, 0.0));
    body.extend(owned_triangle(200, 701, 2.0));

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(decoded.ir().model.bodies[0].id.0, "sldprt:brep:body#900");
    assert_eq!(decoded.ir().model.regions[0].id.0, "sldprt:brep:region#900");
    assert_eq!(decoded.ir().model.shells[0].id.0, "sldprt:brep:shell#500");
    assert_eq!(decoded.ir().model.shells.len(), 2);
    assert!(decoded
        .ir()
        .model
        .shells
        .iter()
        .all(|shell| shell.faces.len() == 1));
    assert_eq!(decoded.ir().model.regions[0].shells.len(), 2);
    assert!(!decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("No body record")));
}

#[test]
fn edge_uses_decoded_line_curve() {
    use cadmpeg_ir::geometry::CurveGeometry;

    let mut body = Vec::new();
    body.extend(plane_carrier(
        100,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    ));
    body.extend(line_carrier(70, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]));
    body.extend(bridge(10, 20, 100));
    body.extend(loop_head(20, 30, 10));
    body.extend(coedge(30, 20, 31, 50, 0, 40, false));
    body.extend(coedge(31, 20, 32, 51, 0, 41, false));
    body.extend(coedge(32, 20, 30, 52, 0, 42, false));
    body.extend(edge_use(40, 70)); // curve = line carrier 70
    body.extend(edge_use(41, 0));
    body.extend(edge_use(42, 0));
    body.extend(vertex_use(50, 60));
    body.extend(vertex_use(51, 61));
    body.extend(vertex_use(52, 62));
    body.extend(world_point(60, [0.0, 0.0, 0.0]));
    body.extend(world_point(61, [1.0, 0.0, 0.0]));
    body.extend(world_point(62, [0.0, 1.0, 0.0]));

    let f = sldprt_with_body(&body);
    let mut cur = Cursor::new(f);
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir().model.curves.len(), 1);
    match &result.ir().model.curves[0].geometry {
        CurveGeometry::Line { direction, .. } => assert_eq!(direction.x, 1.0),
        other => panic!("expected line, got {other:?}"),
    }
    assert_eq!(
        result
            .ir()
            .model
            .edges
            .iter()
            .filter(|e| e.curve.is_some())
            .count(),
        1
    );
    assert_eq!(result.ir().model.pcurves.len(), 1);
    assert!(result
        .ir()
        .model
        .coedges
        .iter()
        .any(|coedge| !coedge.pcurves.is_empty()));
    let report = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(report.is_ok(), "findings: {:?}", report.findings);
}

#[test]
fn edge_uses_decode_nurbs_curve() {
    use cadmpeg_ir::geometry::CurveGeometry;

    let mut body = triangle_body();
    body.extend(nurbs_curve_carrier(170, 171));
    let edge = body
        .windows(2)
        .position(|w| w == [0x00, 0x10])
        .expect("edge-use");
    body[edge + 24..edge + 26].copy_from_slice(&170u16.to_be_bytes());
    let f = sldprt_with_body(&body);
    let mut cur = Cursor::new(f);

    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    let nurbs = result
        .ir()
        .model
        .curves
        .iter()
        .find_map(|curve| match &curve.geometry {
            CurveGeometry::Nurbs(nurbs) => Some(nurbs),
            _ => None,
        })
        .expect("NURBS curve");
    assert_eq!(nurbs.degree, 2);
    assert_eq!(nurbs.control_points.len(), 3);
    assert_eq!(nurbs.knots, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
}

#[test]
fn edge_uses_decode_typed_reference_nurbs_curve() {
    use cadmpeg_ir::geometry::CurveGeometry;

    let mut body = triangle_body();
    body.extend(typed_nurbs_curve_carrier(170, 171));
    let edge = body
        .windows(2)
        .position(|window| window == [0x00, 0x10])
        .expect("edge-use");
    body[edge + 24..edge + 26].copy_from_slice(&170u16.to_be_bytes());

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    let nurbs = result
        .ir()
        .model
        .curves
        .iter()
        .find_map(|curve| match &curve.geometry {
            CurveGeometry::Nurbs(nurbs) => Some(nurbs),
            _ => None,
        })
        .expect("NURBS curve");
    assert_eq!(nurbs.degree, 2);
    assert_eq!(nurbs.control_points.len(), 3);
}

#[test]
fn reused_carrier_attribute_resolves_by_geometry_kind() {
    use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};

    let mut body = triangle_body();
    let bridge = body
        .windows(2)
        .position(|window| window == [0x00, 0x0e])
        .expect("bridge");
    body[bridge + 26..bridge + 28].copy_from_slice(&70u16.to_be_bytes());
    let edge = body
        .windows(2)
        .position(|window| window == [0x00, 0x10])
        .expect("edge-use");
    body[edge + 24..edge + 26].copy_from_slice(&70u16.to_be_bytes());
    body.extend(line_carrier(70, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]));
    body.extend(plane_carrier(
        70,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    ));

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(matches!(
        result.ir().model.curves[0].geometry,
        CurveGeometry::Line { .. }
    ));
    assert!(matches!(
        result.ir().model.surfaces[0].geometry,
        SurfaceGeometry::Plane { .. }
    ));
}

#[test]
fn false_later_loop_candidate_does_not_replace_owned_loop() {
    let mut body = triangle_body();
    body.extend(loop_head(20, 30, 999));

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.loops.len(), 1);
    assert_eq!(result.ir().model.coedges.len(), 3);
    assert_eq!(result.ir().model.loops[0].id.0, "sldprt:brep:loop#20");
}

#[test]
fn faces_decode_nurbs_surface() {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let mut body = triangle_body();
    body.extend(nurbs_surface_carrier(180, 181, 10));
    let bridge = body
        .windows(2)
        .position(|w| w == [0x00, 0x0e])
        .expect("bridge");
    body[bridge + 26..bridge + 28].copy_from_slice(&180u16.to_be_bytes());
    let f = sldprt_with_body(&body);
    let mut cur = Cursor::new(f);

    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    let nurbs = result
        .ir()
        .model
        .surfaces
        .iter()
        .find_map(|surface| match &surface.geometry {
            SurfaceGeometry::Nurbs(nurbs) => Some(nurbs),
            _ => None,
        })
        .expect("NURBS surface");
    assert_eq!((nurbs.u_degree, nurbs.v_degree), (1, 1));
    assert_eq!((nurbs.u_count, nurbs.v_count), (2, 2));
    assert_eq!(nurbs.control_points.len(), 4);
}

#[test]
fn faces_decode_compact_counted_nurbs_surface_arrays() {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let mut body = triangle_body();
    body.extend(compact_counted_nurbs_surface_carrier(180, 181, 10));
    let bridge = body
        .windows(2)
        .position(|window| window == [0x00, 0x0e])
        .expect("bridge");
    body[bridge + 26..bridge + 28].copy_from_slice(&180u16.to_be_bytes());

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    let SurfaceGeometry::Nurbs(surface) = &result.ir().model.surfaces[0].geometry else {
        panic!("compact counted NURBS surface");
    };
    assert_eq!((surface.u_degree, surface.v_degree), (1, 1));
    assert_eq!((surface.u_count, surface.v_count), (2, 2));
    assert_eq!(surface.u_knots, [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(surface.v_knots, [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(surface.control_points.len(), 4);
    assert_eq!(surface.control_points[3].z, 500.0);
    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn conflicting_compact_counted_surface_array_is_rejected() {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let mut body = triangle_body();
    body.extend(compact_counted_nurbs_surface_carrier(180, 181, 10));
    body.extend(compact_f64_array(182, &[1.0; 12]));
    let bridge = body
        .windows(2)
        .position(|window| window == [0x00, 0x0e])
        .expect("bridge");
    body[bridge + 26..bridge + 28].copy_from_slice(&180u16.to_be_bytes());

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(matches!(
        result.ir().model.surfaces[0].geometry,
        SurfaceGeometry::Unknown { .. }
    ));
}

#[test]
fn short_compact_surface_knot_array_is_rejected_without_panicking() {
    let mut bytes = compact_counted_nurbs_surface_carrier(180, 181, 10);
    let multiplicity_attr = 183u16.to_be_bytes();
    let header = bytes
        .windows(4)
        .position(|window| window == [0, 4, multiplicity_attr[0], multiplicity_attr[1]])
        .expect("u multiplicity header");
    bytes[header + 1] = 1;

    assert!(!crate::brep::spline::scan_surface_carriers(&bytes).contains_key(&180));
}

#[test]
fn faces_decode_nested_offset_surface_with_hidden_support() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, SurfaceGeometry};

    let mut body = triangle_body();
    let bridge = body
        .windows(2)
        .position(|window| window == [0x00, 0x0e])
        .expect("bridge");
    body[bridge + 26..bridge + 28].copy_from_slice(&180u16.to_be_bytes());
    body.extend(offset_surface_carrier(180, 181, 0.002));
    body.extend(offset_surface_carrier(181, 100, 0.003));

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.procedural_surfaces.len(), 2);
    assert_eq!(result.ir().model.surfaces.len(), 3);
    assert!(result.ir().model.procedural_surfaces.iter().any(|surface| {
        matches!(
            surface.definition,
            ProceduralSurfaceDefinition::Offset { distance, .. }
                if (distance - 2.0).abs() < f64::EPSILON
        )
    }));
    assert!(result.ir().model.surfaces.iter().any(|surface| {
        matches!(surface.geometry, SurfaceGeometry::Plane { .. })
            && surface.id.0.contains("hidden-support-surf#100")
    }));

    let face_surface = &result.ir().model.faces[0].surface;
    let point = cadmpeg_ir::eval::model_surface_point_by_id(
        &cadmpeg_ir::index::ModelIndex::new(result.ir()),
        face_surface,
        0.0,
        0.0,
    )
    .expect("nested offset evaluation");
    assert!((point.z - 5.0).abs() < 1.0e-12);
    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn blend_emits_typed_and_opaque_hidden_support_surfaces() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, SurfaceGeometry};

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&blend_triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.procedural_surfaces.len(), 1);
    assert_eq!(result.ir().model.surfaces.len(), 3);
    let ProceduralSurfaceDefinition::Blend { supports, .. } =
        &result.ir().model.procedural_surfaces[0].definition
    else {
        panic!("rolling-ball construction");
    };
    let support_surfaces: Vec<_> = supports
        .iter()
        .flatten()
        .map(|support| {
            result
                .ir()
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == support.surface)
                .expect("materialized blend support")
        })
        .collect();
    assert!(matches!(
        support_surfaces[0].geometry,
        SurfaceGeometry::Plane { .. }
    ));
    assert!(matches!(
        support_surfaces[1].geometry,
        SurfaceGeometry::Unknown { .. }
    ));
    for surface in support_surfaces {
        assert!(surface.id.0.contains("hidden-support-surf#"));
    }
    assert!(result.report().losses.iter().any(|loss| {
        loss.message
            .contains("1 untyped surface carrier(s) are retained as opaque hidden supports")
    }));
    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn merged_sites_retain_procedural_surface_constructions() {
    let mut source = outer_header();
    for (type_id, section) in [
        (0x20, "Contents/Config-0-Partition"),
        (0x21, "Contents/Config-1-Partition"),
    ] {
        source.extend(make_block(
            type_id,
            section,
            &parasolid_with_body(
                "partition body",
                "SCH_SW_33103_11000",
                &blend_triangle_body(),
            ),
        ));
    }

    let result = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir().model.procedural_surfaces.len(), 2);
    assert!(result
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .all(|construction| {
            result.ir().model.surfaces.iter().any(|surface| {
                matches!(
                    &surface.geometry,
                    cadmpeg_ir::geometry::SurfaceGeometry::Procedural {
                        construction: candidate,
                    } if candidate == &construction.id
                )
            })
        }));
    assert!(result.report().losses.iter().any(|loss| {
        loss.message
            .contains("2 untyped surface carrier(s) are retained as opaque hidden supports")
    }));
    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn cyclic_offset_surface_graph_remains_unknown() {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let mut body = triangle_body();
    let bridge = body
        .windows(2)
        .position(|window| window == [0x00, 0x0e])
        .expect("bridge");
    body[bridge + 26..bridge + 28].copy_from_slice(&180u16.to_be_bytes());
    body.extend(offset_surface_carrier(180, 181, 0.002));
    body.extend(offset_surface_carrier(181, 180, 0.003));

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result.ir().model.procedural_surfaces.is_empty());
    assert!(matches!(
        result.ir().model.surfaces[0].geometry,
        SurfaceGeometry::Unknown { .. }
    ));
}

#[test]
fn surface_rejects_nonzero_terminal_multiplicity() {
    let bytes =
        nurbs_surface_carrier_with_v_knot_storage(180, 181, 10, &[2, 2, 1], &[0.0, 1.0, 2.0]);
    assert!(!crate::brep::spline::scan_surface_carriers(&bytes).contains_key(&180));
}

#[test]
fn surface_descriptor_uses_terminal_array_references() {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let mut bytes = nurbs_surface_carrier(180, 181, 10);
    let descriptor = bytes
        .windows(2)
        .position(|window| window == [0x00, 0x7e])
        .expect("surface descriptor");

    // Replace only the five terminal references. The complete fixed fields
    // remain valid, so a parser that uses any earlier descriptor window must
    // fail to recover this carrier.
    for (index, reference) in [190u16, 191, 192, 193, 194].into_iter().enumerate() {
        let at = descriptor + 34 + index * 2;
        bytes[at..at + 2].copy_from_slice(&reference.to_be_bytes());
    }
    bytes.extend(f64_array(
        0x2d,
        190,
        &[
            10.0, 0.0, 0.0, 10.0, 1.0, 0.0, 11.0, 0.0, 0.0, 11.0, 1.0, 0.0,
        ],
    ));
    bytes.extend(u16_array(191, &[2, 2]));
    bytes.extend(u16_array(192, &[2, 2]));
    bytes.extend(f64_array(0x80, 193, &[0.0, 1.0]));
    bytes.extend(f64_array(0x80, 194, &[0.0, 1.0]));

    let carrier = crate::brep::spline::scan_surface_carriers(&bytes)
        .remove(&180)
        .expect("surface carrier");
    let crate::brep::CarrierGeometry::Surface(SurfaceGeometry::Nurbs(surface)) = carrier.geometry
    else {
        panic!("expected NURBS surface");
    };
    assert_eq!(surface.control_points[0].x, 10_000.0);
    assert_eq!(surface.control_points[3].y, 1_000.0);
}

#[test]
fn faces_decode_markerless_nurbs_surface_arrays() {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let mut body = triangle_body();
    body.extend(markerless_nurbs_surface_carrier(180, 181, 10));
    let bridge = body
        .windows(2)
        .position(|window| window == [0x00, 0x0e])
        .expect("bridge");
    body[bridge + 26..bridge + 28].copy_from_slice(&180u16.to_be_bytes());

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    let nurbs = result
        .ir()
        .model
        .surfaces
        .iter()
        .find_map(|surface| match &surface.geometry {
            SurfaceGeometry::Nurbs(nurbs) => Some(nurbs),
            _ => None,
        })
        .expect("NURBS surface");
    assert_eq!((nurbs.u_count, nurbs.v_count), (2, 2));
}

#[test]
fn nurbs_boundary_curve_gets_isoparametric_pcurve() {
    let mut body = triangle_body();
    let bridge = body.windows(2).position(|w| w == [0x00, 0x0e]).unwrap();
    body[bridge + 26..bridge + 28].copy_from_slice(&180u16.to_be_bytes());
    let edge = body.windows(2).position(|w| w == [0x00, 0x10]).unwrap();
    body[edge + 24..edge + 26].copy_from_slice(&190u16.to_be_bytes());
    body.extend(nurbs_surface_carrier(180, 181, 10));
    body.extend(linear_nurbs_curve_carrier(190, 191));
    let mut cur = Cursor::new(sldprt_with_body(&body));
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert!(result.ir().model.pcurves.iter().any(|pcurve| {
        result
            .source_fidelity()
            .annotations
            .provenance
            .get(&pcurve.id.0)
            .and_then(|note| note.tag.as_deref())
            == Some("derived_nurbs_isoparametric_pcurve")
    }));
}

#[test]
fn linear_nurbs_surface_boundary_gets_affine_line_pcurve() {
    let mut body = triangle_body();
    let bridge = body.windows(2).position(|w| w == [0x00, 0x0e]).unwrap();
    body[bridge + 26..bridge + 28].copy_from_slice(&180u16.to_be_bytes());
    let edge = body.windows(2).position(|w| w == [0x00, 0x10]).unwrap();
    body[edge + 24..edge + 26].copy_from_slice(&192u16.to_be_bytes());
    body.extend(nurbs_surface_carrier(180, 181, 10));
    body.extend(line_carrier(190, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]));
    body.extend(bounded_curve_wrapper(
        192,
        190,
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        0.0,
        1.0,
    ));
    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result.ir().model.pcurves.iter().any(|pcurve| {
        result
            .source_fidelity()
            .annotations
            .provenance
            .get(&pcurve.id.0)
            .and_then(|note| note.tag.as_deref())
            == Some("derived_nurbs_isoparametric_pcurve")
            && matches!(
                pcurve.geometry,
                cadmpeg_ir::geometry::PcurveGeometry::Line { direction, .. }
                    if direction.v == 0.0 && direction.u != 0.0
            )
    }));
    assert_eq!(
        result
            .ir()
            .model
            .edges
            .iter()
            .find(|edge| edge.curve.as_ref().is_some_and(|id| id.0.ends_with("#192")))
            .and_then(|edge| edge.param_range),
        Some([0.0, 1000.0])
    );
    assert!(cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).is_ok());
}

#[test]
fn bounded_planar_line_pcurve_keeps_the_curve_parameterization() {
    let mut body = triangle_body();
    let edge = body
        .windows(2)
        .position(|window| window == [0x00, 0x10])
        .unwrap();
    body[edge + 24..edge + 26].copy_from_slice(&192u16.to_be_bytes());
    body.extend(line_carrier(190, [0.5, 0.0, 0.0], [1.0, 0.0, 0.0]));
    body.extend(bounded_curve_wrapper(
        192,
        190,
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        -0.5,
        0.5,
    ));
    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(
        result
            .ir()
            .model
            .edges
            .iter()
            .find(|edge| edge.curve.as_ref().is_some_and(|id| id.0.ends_with("#192")))
            .and_then(|edge| edge.param_range),
        Some([-500.0, 500.0])
    );
    assert!(cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).is_ok());
}

#[test]
fn rational_nurbs_surface_row_gets_isoparametric_pcurve() {
    let mut body = triangle_body();
    let bridge = body.windows(2).position(|w| w == [0x00, 0x0e]).unwrap();
    body[bridge + 26..bridge + 28].copy_from_slice(&180u16.to_be_bytes());
    let edge = body.windows(2).position(|w| w == [0x00, 0x10]).unwrap();
    body[edge + 24..edge + 26].copy_from_slice(&190u16.to_be_bytes());
    body.extend(rational_nurbs_surface_carrier(180, 181, 10));
    body.extend(rational_linear_nurbs_curve_carrier(190, 191));
    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result.ir().model.pcurves.iter().any(|pcurve| {
        result
            .source_fidelity()
            .annotations
            .provenance
            .get(&pcurve.id.0)
            .and_then(|note| note.tag.as_deref())
            == Some("derived_nurbs_isoparametric_pcurve")
    }));
}

#[test]
fn decode_transfers_body_material_color() {
    let f = sldprt_with_body_and_material(&triangle_body(), "Steel", [32, 64, 128]);
    let mut cur = Cursor::new(f);

    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    let color = result.ir().model.bodies[0].color.expect("body color");
    assert!((color.r - 32.0 / 255.0).abs() < 1e-6);
    assert!((color.g - 64.0 / 255.0).abs() < 1e-6);
    assert!((color.b - 128.0 / 255.0).abs() < 1e-6);
    assert_eq!(result.ir().model.appearances.len(), 1);
    assert_eq!(result.ir().model.appearance_bindings.len(), 1);
    assert_eq!(
        result.ir().model.appearances[0].name.as_deref(),
        Some("Steel")
    );
}

#[test]
fn decode_preserves_ambiguous_materials_without_fabricating_ownership() {
    let mut source = sldprt_with_body(&triangle_body());
    let mut materials = material_payload("Steel", [32, 64, 128]);
    materials.extend(material_payload("Aluminum", [160, 170, 180]));
    source.extend(make_block(0x40, "SWObjects", &materials));

    let mut result = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.appearances.len(), 2);
    assert!(result.ir().model.appearance_bindings.is_empty());
    assert!(result
        .ir()
        .model
        .bodies
        .iter()
        .all(|body| body.color.is_none() && body.name.is_none()));

    result.ir_mut().model.points[0].position.z += 1.0;
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(result.ir(), result.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(regenerated.ir().model.appearances.len(), 2);
    assert_eq!(
        regenerated
            .ir()
            .model
            .appearances
            .iter()
            .filter_map(|appearance| appearance.name.as_deref())
            .collect::<Vec<_>>(),
        vec!["Steel", "Aluminum"]
    );
    assert!(regenerated.ir().model.appearance_bindings.is_empty());
}

#[test]
fn decode_binds_entity53_color_to_face() {
    use cadmpeg_ir::appearance::AppearanceTarget;
    let mut body = Vec::new();
    body.extend(entity51(2, 500, 0x0017, &[700, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 700, 0x0015, &[0, 0, 0, 0, 0, 900]));
    body.extend(entity53_color(900, [0.25, 0.5, 0.75]));
    body.extend(owned_triangle(0, 700, 0.0));
    let mut cur = Cursor::new(sldprt_with_body(&body));
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        result.report().losses.len(),
        1,
        "{:#?}",
        result.report().losses
    );
    assert_eq!(
        result.report().losses[0].message,
        "1 configuration state(s) are inferred from geometry partitions without native configuration definitions."
    );
    let binding = result
        .ir()
        .model
        .appearance_bindings
        .iter()
        .find(|binding| matches!(binding.target, AppearanceTarget::Face(_)))
        .expect("face binding");
    let appearance = result
        .ir()
        .model
        .appearances
        .iter()
        .find(|appearance| appearance.id == binding.appearance)
        .unwrap();
    let color = appearance.base_color.unwrap();
    assert_eq!([color.r, color.g, color.b], [0.25, 0.5, 0.75]);
}

#[test]
fn decode_does_not_bind_color_to_an_unemitted_face() {
    use cadmpeg_ir::appearance::AppearanceTarget;

    let mut body = Vec::new();
    body.extend(entity51(1, 700, 0x0015, &[0, 0, 0, 0, 0, 900]));
    body.extend(entity53_color(900, [0.25, 0.5, 0.75]));
    body.extend(entity51(1, 701, 0x0015, &[0, 0, 0, 0, 0, 901]));
    body.extend(entity53_color(901, [0.75, 0.5, 0.25]));
    body.extend(owned_triangle(0, 700, 0.0));
    body.extend(plane_carrier(
        200,
        [2.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    ));
    body.extend(bridge_owned(110, 120, 200, 701));

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.appearances.len(), 2);
    assert_eq!(
        result
            .ir()
            .model
            .appearance_bindings
            .iter()
            .filter(|binding| matches!(binding.target, AppearanceTarget::Face(_)))
            .count(),
        1
    );
    assert!(cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).is_ok());
}

#[test]
fn decode_removes_edges_and_vertices_from_a_rejected_loop() {
    let mut body = triangle_body();
    body.extend(plane_carrier(
        200,
        [2.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    ));
    body.extend(bridge(110, 120, 200));
    body.extend(loop_head(120, 130, 110));
    body.extend(coedge(130, 120, 131, 150, 0, 140, false));
    body.extend(coedge(131, 120, 132, 151, 0, 141, false));
    body.extend(coedge(132, 120, 130, 152, 0, 142, false));
    body.extend(edge_use(140, 0));
    body.extend(edge_use(141, 0));
    body.extend(edge_use(142, 0));
    body.extend(vertex_use(150, 160));
    body.extend(vertex_use(151, 161));
    body.extend(vertex_use(152, 162));
    body.extend(world_point(160, [2.0, 0.0, 0.0]));
    body.extend(world_point(161, [3.0, 0.0, 0.0]));

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.loops.len(), 1);
    assert_eq!(result.ir().model.coedges.len(), 3);
    assert_eq!(result.ir().model.edges.len(), 3);
    assert_eq!(result.ir().model.vertices.len(), 3);
    assert_eq!(result.ir().model.points.len(), 3);
    assert!(cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).is_ok());
}

#[test]
fn partition_point_refs_do_not_select_deltas_framing() {
    let mut body = triangle_body();
    let point = body
        .windows(4)
        .position(|window| window == [0x00, 0x1d, 0x00, 0x3c])
        .expect("point 60");
    for (index, reference) in [1u16, 378, 379, 373].into_iter().enumerate() {
        let at = point + 8 + index * 2;
        body[at..at + 2].copy_from_slice(&reference.to_be_bytes());
    }

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.vertices.len(), 3);
    assert_eq!(result.ir().model.points.len(), 3);
    assert!(cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).is_ok());
}

#[test]
fn deltas_point_index_does_not_replace_partition_coordinates() {
    let partition = triangle_body();
    let mut deltas = Vec::new();
    for attr in 60u16..80 {
        deltas.extend_from_slice(&[0x00, 0x1d]);
        deltas.extend_from_slice(&attr.to_be_bytes());
    }

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_partition_and_deltas(&partition, &deltas)),
            &DecodeOptions::default(),
        )
        .unwrap();

    let point = result
        .ir()
        .model
        .points
        .iter()
        .find(|point| point.id.0.ends_with("#60"))
        .unwrap();
    assert_eq!(point.position, cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0));
}

#[test]
fn decode_binds_adjacent_entity53_color_to_disc14_face() {
    use cadmpeg_ir::appearance::AppearanceTarget;
    let mut body = Vec::new();
    body.extend(entity51(2, 500, 0x0017, &[700, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 700, 0x0014, &[0, 0, 0, 0, 0, 0]));
    body.extend(entity53_color(901, [1.0, 0.125, 0.0]));
    body.extend(owned_triangle(0, 700, 0.0));
    let mut cur = Cursor::new(sldprt_with_body(&body));
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    let binding = result
        .ir()
        .model
        .appearance_bindings
        .iter()
        .find(|binding| matches!(binding.target, AppearanceTarget::Face(_)))
        .expect("face binding");
    let color = result
        .ir()
        .model
        .appearances
        .iter()
        .find(|appearance| appearance.id == binding.appearance)
        .unwrap()
        .base_color
        .unwrap();
    assert_eq!([color.r, color.g, color.b], [1.0, 0.125, 0.0]);
}

#[test]
fn decode_reports_display_list_geometry() {
    let f = sldprt_with_body_and_display_list(&triangle_body());
    let mut cur = Cursor::new(f);

    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    let source = result.ir().source.as_ref().expect("source metadata");

    assert_eq!(
        source
            .attributes
            .get("displaylist_vertices")
            .map(String::as_str),
        Some("3")
    );
    assert_eq!(
        source
            .attributes
            .get("displaylist_triangles")
            .map(String::as_str),
        Some("1")
    );
    assert_eq!(result.ir().model.tessellations.len(), 1);
    assert_eq!(result.ir().model.tessellations[0].vertices.len(), 3);
    assert_eq!(result.ir().model.tessellations[0].vertices[1].x, 1000.0);
    assert_eq!(
        result.ir().model.tessellations[0].triangles,
        vec![[0, 1, 2]]
    );
    assert_eq!(result.ir().model.tessellations[0].strip_lengths, vec![3]);
    assert_eq!(result.ir().model.tessellations[0].normals.len(), 3);
    assert_eq!(result.ir().model.tessellations[0].channels.len(), 6);
    assert_eq!(
        result.ir().model.tessellations[0].faces,
        [result.ir().model.faces[0].id.clone()]
    );
    assert_eq!(
        result.ir().model.tessellations[0].body.as_ref(),
        Some(&result.ir().model.bodies[0].id)
    );
    assert!(!result.report().losses.iter().any(|loss| {
        loss.code.taxonomy() == LossTaxonomy::ReferenceGraphNotClosed
            && loss.message.contains("DisplayLists tessellation")
    }));
    assert!(result
        .ir()
        .native_unknowns("sldprt")
        .unwrap()
        .iter()
        .any(|record| {
            result
                .source_fidelity()
                .annotations
                .provenance
                .get(&record.id.0)
                .and_then(|note| note.tag.as_deref())
                == Some("displaylist_tessellation")
                && result
                    .source_fidelity()
                    .retained_record(&record.id.0)
                    .is_some_and(|source| source.data.is_some())
        }));
}

#[test]
fn decode_reports_extended_header_display_list_geometry() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x41,
        "Contents/DisplayLists",
        &extended_display_list_payload(),
    ));
    let result = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.tessellations.len(), 1);
    assert_eq!(result.ir().model.tessellations[0].triangles, [[0, 1, 2]]);
}

#[test]
fn decode_rejects_incoherent_display_list_header_counts() {
    let mut payload = display_list_payload();
    let marker = b"uoTempFaceTessData_c";
    let header = payload
        .windows(marker.len())
        .position(|bytes| bytes == marker)
        .expect("face tessellation class")
        + marker.len();
    payload[header..header + 4].copy_from_slice(&2_u32.to_le_bytes());
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x41, "Contents/DisplayLists", &payload));

    let result = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(result.ir().model.tessellations.is_empty());
}

#[test]
fn decode_rejects_inconsistent_display_list_table() {
    let mut payload = display_list_payload();
    let marker = b"uoTempFaceTessData_c";
    let at = payload
        .windows(marker.len())
        .position(|bytes| bytes == marker)
        .unwrap()
        + marker.len()
        + 8
        + 16;
    payload[at..at + 4].copy_from_slice(&4u32.to_le_bytes());
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x41, "Contents/DisplayLists", &payload));

    let result = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(result.ir().model.tessellations.is_empty());
    assert!(!result
        .ir()
        .source
        .as_ref()
        .unwrap()
        .attributes
        .contains_key("displaylist_vertices"));
}

#[test]
fn decode_rejects_nonfinite_display_list_values() {
    let mut payload = display_list_payload();
    let marker = b"uoTempFaceTessData_c";
    let position_data = payload
        .windows(marker.len())
        .position(|bytes| bytes == marker)
        .unwrap()
        + marker.len()
        + 8
        + 16
        + 4
        + 16;
    payload[position_data..position_data + 4].copy_from_slice(&f32::NAN.to_le_bytes());
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x41, "Contents/DisplayLists", &payload));

    let result = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(result.ir().model.tessellations.is_empty());
}

#[test]
fn decode_extracts_parametric_history() {
    let f = sldprt_with_body_and_history(&triangle_body());
    let mut cur = Cursor::new(f);
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(result.ir());
    let history = &native.feature_histories[0];
    assert_eq!(history.part_name.as_deref(), Some("Bracket"));
    assert_eq!(history.configurations[0].material.as_deref(), Some("Steel"));
    assert_eq!(result.ir().model.configurations.len(), 1);
    assert_eq!(result.ir().model.configurations[0].name, "Default");
    assert_eq!(
        result.ir().model.configurations[0].material.as_deref(),
        Some("Steel")
    );
    assert_eq!(
        result.ir().model.configurations[0].native_ref.as_deref(),
        Some(history.configurations[0].id.as_str())
    );
    assert_eq!(history.features[0].kind, "BossExtrude");
    assert_eq!(history.features[0].xml_tag, "Extrusion");
    assert_eq!(history.features[0].parameters["Depth"], "12.5mm");
    assert_eq!(history.features[0].properties["Scope"], "Body1");
    assert_eq!(history.features[1].parent_source_id.as_deref(), Some("7"));
    assert_eq!(history.features[1].xml_tag, "EquationDrivenCurve");
    assert_eq!(result.ir().model.features.len(), 2);
    let neutral = &result.ir().model.features[0];
    assert_eq!(neutral.name.as_deref(), Some("Boss"));
    assert_eq!(
        neutral.native_ref.as_deref(),
        Some(history.features[0].id.as_str())
    );
    assert!(matches!(
        &neutral.definition,
        cadmpeg_ir::features::FeatureDefinition::Extrude {
            profile: cadmpeg_ir::features::ProfileRef::Unresolved(profile),
            direction: cadmpeg_ir::features::ExtrudeDirection::ProfileNormal,
            start: cadmpeg_ir::features::ExtrudeStart::ProfilePlane,
            extent: cadmpeg_ir::features::ExtrudeExtent::OneSided {
                side: cadmpeg_ir::features::ExtrudeSide {
                    termination: cadmpeg_ir::features::Termination::Blind {
                        length: cadmpeg_ir::features::Length(12.5),
                    },
                    draft: None,
                    ..
                }
            },
            op: cadmpeg_ir::features::BooleanOp::Join,
            ..
        } if profile == &history.features[0].id
    ));
    assert_eq!(
        result.ir().model.features[1].parent.as_ref(),
        Some(&neutral.id)
    );
}

#[test]
fn decode_uses_plain_numeric_config_as_legacy_feature_input_lane() {
    let legacy = resolved_feature_classes_with_ids(&[("Fillet_c", "Round", 41)]);
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x42, "Contents/Config-7", &legacy));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let lanes = &sldprt_native(decoded.ir()).feature_input_lanes;
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].configuration.as_deref(), Some("7"));
    assert_eq!(lanes[0].native_payload, legacy);
}

#[test]
fn decode_prefers_explicit_feature_input_lanes_over_plain_config_streams() {
    let legacy = resolved_feature_classes_with_ids(&[("Fillet_c", "Round", 41)]);
    let explicit = resolved_feature_classes_with_ids(&[("Chamfer_c", "Bevel", 42)]);
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x42, "Contents/Config-7", &legacy));
    source.extend(make_block(
        0x42,
        "Contents/Config-7-ResolvedFeatures",
        &explicit,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let lanes = &sldprt_native(decoded.ir()).feature_input_lanes;
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].configuration.as_deref(), Some("7"));
    assert_eq!(lanes[0].native_payload, explicit);
}

#[test]
fn decode_types_non_modeling_feature_tree_nodes() {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureTreeNodeRole};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Feature Name="Annotations" Type="Annotations" id="101"/>
            <Feature Name="Ecuaciones" Type="Ecuaciones" id="102"/>
            <Feature Name="Bodies" Type="Solid Bodies" id="103"/>
            <Feature Name="Light" Type="Direccional" id="104"/>
            <Feature Name="Unknown" Type="CustomOperation" id="105"/>
            <Sketch Name="Origen" Type="Croquis localizado" id="106"/>
        </Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[
            ("moDetailCabinet_c", "Annotations", 101),
            ("moEqnFolder_c", "Ecuaciones", 102),
            ("moSolidBodyFolder_c", "Bodies", 103),
            ("moOriginProfileFeature_c", "Origen", 106),
        ]),
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let definitions = decoded
        .ir()
        .model
        .features
        .iter()
        .map(|feature| &feature.definition)
        .collect::<Vec<_>>();
    assert!(matches!(
        definitions[0],
        FeatureDefinition::TreeNode {
            role: FeatureTreeNodeRole::Annotations,
            ..
        }
    ));
    assert!(matches!(
        definitions[1],
        FeatureDefinition::TreeNode {
            role: FeatureTreeNodeRole::Equations,
            ..
        }
    ));
    assert!(matches!(
        definitions[2],
        FeatureDefinition::TreeNode {
            role: FeatureTreeNodeRole::SolidBodies,
            ..
        }
    ));
    assert!(matches!(definitions[3], FeatureDefinition::Native { .. }));
    assert!(matches!(definitions[4], FeatureDefinition::Native { .. }));
    assert!(matches!(
        definitions[5],
        FeatureDefinition::TreeNode {
            role: FeatureTreeNodeRole::ModelOrigin,
            ..
        }
    ));
    assert!(!decoded
        .ir()
        .model
        .features
        .iter()
        .any(|feature| matches!(feature.definition, FeatureDefinition::Sketch { .. })));
    decoded.ir_mut().model.features[0].name = Some("Document annotations".into());
    let mut encoded = Vec::new();
    SldprtCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: decoded.ir(),
            fidelity: Some(decoded.source_fidelity()),
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        regenerated.ir().model.features[0].definition,
        FeatureDefinition::TreeNode {
            role: FeatureTreeNodeRole::Annotations,
            ..
        }
    ));
}

#[test]
fn decode_leaves_position_allocated_tree_nodes_untyped() {
    use cadmpeg_ir::features::FeatureDefinition;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Feature Name="Luces y camaras" Type="Localized" id="6"/>
            <Feature Name="Ambiental" Type="Localized" id="12"/>
            <Feature Name="Direccional uno" Type="Localized" id="13"/>
            <Feature Name="Direccional dos" Type="Localized" id="14"/>
            <Feature Name="Direccional tres" Type="Localized" id="15"/>
            <Feature Name="Vistas" Type="Localized" id="19"/>
        </Keywords>"#,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(decoded
        .ir()
        .model
        .features
        .iter()
        .all(|feature| matches!(feature.definition, FeatureDefinition::Native { .. })));
}

#[test]
fn reserved_tree_node_ids_require_builtin_record_shape() {
    use cadmpeg_ir::features::{ExtrudeExtent, ExtrudeSide, FeatureDefinition, Termination};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Extrusion Name="Operation" Type="Localized" id="12"/>
            <Feature Name="Attributed" Type="Localized" id="19" State="custom"/>
        </Keywords>"#,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Unresolved,
                    ..
                }
            },
            ..
        }
    ));
    assert!(matches!(
        decoded.ir().model.features[1].definition,
        FeatureDefinition::Native { .. }
    ));
}

#[test]
fn decode_binds_duplicate_feature_names_by_native_object_id() {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureTreeNodeRole};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Feature Name="Folder" Type="Custom" id="41"/>
            <Feature Name="Folder" Type="Custom" id="42"/>
        </Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[
            ("moEqnFolder_c", "Folder", 41),
            ("moSolidBodyFolder_c", "Folder", 42),
        ]),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::TreeNode {
            role: FeatureTreeNodeRole::Equations,
            ..
        }
    ));
    assert!(matches!(
        decoded.ir().model.features[1].definition,
        FeatureDefinition::TreeNode {
            role: FeatureTreeNodeRole::SolidBodies,
            ..
        }
    ));
}

#[test]
fn decode_propagates_unique_object_class_by_serialized_type_token() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Feature Name="Redondeo1" Type="Redondeo" id="41"/>
            <Feature Name="Redondeo2" Type="Redondeo" id="42"/>
        </Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[("Fillet_c", "Redondeo2", 42)]),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(decoded.ir());
    assert!(native.feature_histories[0]
        .features
        .iter()
        .all(|feature| feature.input_class.as_deref() == Some("Fillet_c")));
}

#[test]
fn decode_binds_repeated_instances_by_class_token() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Feature Name="Seed" Type="LocalizedFillet" id="41"/>
            <Feature Name="TokenSeed" Type="LocalizedFillet" id="42"/>
            <Feature Name="TokenOnly" Type="OpaqueType" id="43"/>
        </Keywords>"#,
    ));
    let mut payload = resolved_feature_classes_with_ids(&[("Fillet_c", "Seed", 41)]);
    for (name, object_id) in [("TokenSeed", 42u32), ("TokenOnly", 43)] {
        payload.extend_from_slice(&0x37a5u16.to_le_bytes());
        payload.extend_from_slice(&[0x04, 0x80, 0xff, 0xfe, 0xff, name.len() as u8]);
        for unit in name.encode_utf16() {
            payload.extend_from_slice(&unit.to_le_bytes());
        }
        payload.extend_from_slice(&[0; 8]);
        payload.extend_from_slice(&object_id.to_le_bytes());
    }
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &payload,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(decoded.ir());
    assert!(native.feature_histories[0]
        .features
        .iter()
        .all(|feature| feature.input_class.as_deref() == Some("Fillet_c")));
}

#[test]
fn decode_does_not_propagate_ambiguous_object_class_by_type_token() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Feature Name="First" Type="Custom" id="41"/>
            <Feature Name="Second" Type="Custom" id="42"/>
            <Feature Name="Third" Type="Custom" id="43"/>
        </Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[
            ("Fillet_c", "First", 41),
            ("moRefPlane_c", "Second", 42),
        ]),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(decoded.ir());
    assert_eq!(native.feature_histories[0].features[2].input_class, None);
}

#[test]
fn decode_does_not_bind_ambiguous_repeated_class_token() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Feature Name="FilletSeed" Type="FilletType" id="41"/>
            <Feature Name="PlaneSeed" Type="PlaneType" id="42"/>
            <Feature Name="FilletToken" Type="FilletType" id="43"/>
            <Feature Name="PlaneToken" Type="PlaneType" id="44"/>
            <Feature Name="Unknown" Type="UnknownType" id="45"/>
        </Keywords>"#,
    ));
    let mut payload = resolved_feature_classes_with_ids(&[
        ("Fillet_c", "FilletSeed", 41),
        ("moRefPlane_c", "PlaneSeed", 42),
    ]);
    for (name, object_id) in [("FilletToken", 43u32), ("PlaneToken", 44), ("Unknown", 45)] {
        payload.extend_from_slice(&0x37a5u16.to_le_bytes());
        payload.extend_from_slice(&[0x04, 0x80, 0xff, 0xfe, 0xff, name.len() as u8]);
        for unit in name.encode_utf16() {
            payload.extend_from_slice(&unit.to_le_bytes());
        }
        payload.extend_from_slice(&[0; 8]);
        payload.extend_from_slice(&object_id.to_le_bytes());
    }
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &payload,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(decoded.ir());
    assert_eq!(native.feature_histories[0].features[4].input_class, None);
}

#[test]
fn decode_does_not_bind_object_class_by_display_name() {
    use cadmpeg_ir::features::FeatureDefinition;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Plane" Type="Custom" id="41"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[("moRefPlane_c", "Plane", 42)]),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::Native { .. }
    ));
    assert_eq!(
        sldprt_native(decoded.ir()).feature_histories[0].features[0].input_class,
        None
    );
}

#[test]
fn keywords_root_id_does_not_create_feature_parentage() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords id="document"><Feature Name="Root" Type="Folder" id="1"><Feature Name="Nested" Type="Custom" id="2"/></Feature></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(decoded.ir());
    let history = &native.feature_histories[0];
    assert_eq!(history.properties["id"], "document");
    assert_eq!(history.features[0].parent_source_id, None);
    assert_eq!(history.features[1].parent_source_id.as_deref(), Some("1"));
    assert!(crate::validate_native(decoded.ir()).is_empty());
}

#[test]
fn decode_projects_every_dimension_as_a_neutral_parameter() {
    use cadmpeg_ir::features::{Angle, DimensionDisplay, Length, ParameterValue};

    let mut source = sldprt_with_body(&triangle_body());
    let keywords = format!(
        r#"<Keywords><Feature Name="Inputs" Type="EquationDriven" id="16">
            <Dimension Name="Angle">90deg</Dimension>
            <Dimension Name="DisplayAngle">45.00{degree}</Dimension>
            <Dimension Name="Count">4</Dimension>
            <Dimension Name="Diameter">{diameter}2.5</Dimension>
            <Dimension Name="ModifiedDiameter">&lt;MOD-DIAM&gt;3.18</Dimension>
            <Dimension Name="Enabled">true</Dimension>
            <Dimension Name="Expression">D1@Sketch1 * 2</Dimension>
            <Dimension Name="Length">0.5in</Dimension>
            <Dimension Name="Radius">R0.5</Dimension>
            <Dimension Name="Ratio">1.25</Dimension>
        </Feature></Keywords>"#,
        degree = '\u{00b0}',
        diameter = '\u{2300}',
    );
    source.extend(make_block(0x42, "Contents/Keywords", keywords.as_bytes()));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let parameters = &decoded.ir().model.parameters;
    assert_eq!(parameters.len(), 10);
    assert_eq!(
        parameters
            .iter()
            .map(|parameter| (parameter.ordinal, parameter.name.as_str()))
            .collect::<Vec<_>>(),
        vec![
            (0, "Angle"),
            (1, "DisplayAngle"),
            (2, "Count"),
            (3, "Diameter"),
            (4, "ModifiedDiameter"),
            (5, "Enabled"),
            (6, "Expression"),
            (7, "Length"),
            (8, "Radius"),
            (9, "Ratio"),
        ]
    );
    let value = |name: &str| {
        parameters
            .iter()
            .find(|parameter| parameter.name == name)
            .and_then(|parameter| parameter.value.as_ref())
    };
    assert!(matches!(
        value("Angle"),
        Some(ParameterValue::Angle(Angle(angle)))
            if (*angle - std::f64::consts::FRAC_PI_2).abs() < 1e-12
    ));
    assert!(matches!(
        value("DisplayAngle"),
        Some(ParameterValue::Angle(Angle(angle)))
            if (*angle - std::f64::consts::FRAC_PI_4).abs() < 1e-12
    ));
    assert_eq!(value("Count"), Some(&ParameterValue::Integer(4)));
    assert_eq!(
        value("Diameter"),
        Some(&ParameterValue::Length(Length(2.5)))
    );
    assert_eq!(
        parameters
            .iter()
            .find(|parameter| parameter.name == "Diameter")
            .and_then(|parameter| parameter.display),
        Some(DimensionDisplay::Diameter)
    );
    assert_eq!(
        value("ModifiedDiameter"),
        Some(&ParameterValue::Length(Length(3.18)))
    );
    assert_eq!(
        parameters
            .iter()
            .find(|parameter| parameter.name == "ModifiedDiameter")
            .and_then(|parameter| parameter.display),
        Some(DimensionDisplay::Diameter)
    );
    assert_eq!(value("Enabled"), Some(&ParameterValue::Boolean(true)));
    assert_eq!(value("Expression"), None);
    assert_eq!(value("Length"), Some(&ParameterValue::Length(Length(12.7))));
    assert_eq!(value("Radius"), Some(&ParameterValue::Length(Length(0.5))));
    assert_eq!(
        parameters
            .iter()
            .find(|parameter| parameter.name == "Radius")
            .and_then(|parameter| parameter.display),
        Some(DimensionDisplay::Radius)
    );
    assert_eq!(value("Ratio"), Some(&ParameterValue::Real(1.25)));
    assert!(parameters
        .iter()
        .all(|parameter| parameter.owner.as_ref() == Some(&decoded.ir().model.features[0].id)));

    let radius = decoded
        .ir_mut()
        .model
        .parameters
        .iter_mut()
        .find(|parameter| parameter.name == "Radius")
        .unwrap();
    radius.expression = "R2".into();
    radius.value = Some(ParameterValue::Length(Length(2.0)));
    let modified_diameter = decoded
        .ir_mut()
        .model
        .parameters
        .iter_mut()
        .find(|parameter| parameter.name == "ModifiedDiameter")
        .unwrap();
    modified_diameter.expression = "<MOD-DIAM>4".into();
    modified_diameter.value = Some(ParameterValue::Length(Length(4.0)));
    let display_angle = decoded
        .ir_mut()
        .model
        .parameters
        .iter_mut()
        .find(|parameter| parameter.name == "DisplayAngle")
        .unwrap();
    display_angle.expression = format!("30{}", '\u{00b0}');
    display_angle.value = Some(ParameterValue::Angle(Angle(30.0_f64.to_radians())));

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native_parameters =
        &sldprt_native(regenerated.ir()).feature_histories[0].features[0].parameters;
    assert_eq!(native_parameters["Radius"], "R2");
    assert_eq!(native_parameters["ModifiedDiameter"], "<MOD-DIAM>4");
    assert_eq!(
        native_parameters["DisplayAngle"],
        format!("30{}", '\u{00b0}')
    );
    assert!(matches!(
        regenerated
            .ir()
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == "Radius")
            .and_then(|parameter| parameter.value.as_ref()),
        Some(ParameterValue::Length(Length(2.0)))
    ));
    assert_eq!(
        regenerated
            .ir()
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == "Radius")
            .and_then(|parameter| parameter.display),
        Some(DimensionDisplay::Radius)
    );
    assert!(matches!(
        regenerated
            .ir()
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == "DisplayAngle")
            .and_then(|parameter| parameter.value.as_ref()),
        Some(ParameterValue::Angle(Angle(angle)))
            if (*angle - std::f64::consts::FRAC_PI_6).abs() < 1e-12
    ));
    assert_eq!(
        regenerated
            .ir()
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == "ModifiedDiameter")
            .map(|parameter| (parameter.display, parameter.value.as_ref())),
        Some((
            Some(DimensionDisplay::Diameter),
            Some(&ParameterValue::Length(Length(4.0)))
        ))
    );
}

#[test]
fn parameter_references_distinguish_reserved_expression_syntax() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Equations" Type="EquationDriven" id="7"><Dimension Name="sin">1</Dimension><Dimension Name="pi">2</Dimension><Dimension Name="iif">3</Dimension><Dimension Name="Width">4mm</Dimension><Dimension Name="Driven">sin(30deg) + pi + iif(Width = 4mm, 1, 2) + &quot;sin&quot; + &quot;pi&quot; + &quot;iif&quot;</Dimension></Feature></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let parameter_id = |name: &str| {
        decoded
            .ir()
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == name)
            .unwrap()
            .id
            .clone()
    };
    let expected_dependencies = vec![
        parameter_id("Width"),
        parameter_id("sin"),
        parameter_id("pi"),
        parameter_id("iif"),
    ];
    assert_eq!(
        decoded
            .ir()
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == "Driven")
            .unwrap()
            .dependencies,
        expected_dependencies
    );

    for (old_name, new_name) in [
        ("sin", "Sine input"),
        ("pi", "Pi input"),
        ("iif", "Choice input"),
    ] {
        decoded
            .ir_mut()
            .model
            .parameters
            .iter_mut()
            .find(|parameter| parameter.name == old_name)
            .unwrap()
            .name = new_name.into();
    }
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let driven = regenerated
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Driven")
        .unwrap();
    assert_eq!(
        driven.expression,
        "sin(30deg) + pi + iif(Width = 4mm, 1, 2) + \"Sine input\" + \"Pi input\" + \"Choice input\""
    );
    assert_eq!(driven.dependencies.len(), 4);
}

#[test]
fn decode_evaluates_parameter_dependency_expressions() {
    use cadmpeg_ir::features::{Length, ParameterValue};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Equations" Type="EquationDriven" id="7"><Dimension Name="Width">4mm</Dimension><Dimension Name="Copies">3</Dimension><Dimension Name="Double width">Width * 2</Dimension><Dimension Name="Per copy">&quot;Double width&quot; / Copies</Dimension><Dimension Name="Forward">Later + 1mm</Dimension><Dimension Name="Later">2mm</Dimension><Dimension Name="Scientific">1e-3 * Width</Dimension><Dimension Name="Mixed units">1ft + 1in + 1mil + 1uin + 1um + 1nm + 1&#197;</Dimension><Dimension Name="Power">2^3^2</Dimension><Dimension Name="Sine">sin(30deg)</Dimension><Dimension Name="Inverse sine">arcsin(0.5)</Dimension><Dimension Name="Absolute">abs(-2mm)</Dimension><Dimension Name="Root">sqr(9)</Dimension><Dimension Name="Sign negative">sgn(-2)</Dimension><Dimension Name="Sign zero">sgn(0)</Dimension><Dimension Name="Sign positive">sgn(2)</Dimension><Dimension Name="Pi">pi</Dimension><Dimension Name="Conditional">iif(Width >= 4mm, Width * 2, 1mm)</Dimension><Dimension Name="Leading equals">=iif(Copies&lt;&gt;3, 1, 2)</Dimension><Dimension Name="Comparison">Width = 4mm</Dimension><Dimension Name="Invalid">Width + Copies</Dimension><Dimension Name="Invalid area">Width^2</Dimension><Dimension Name="Invalid branches">iif(true, Width, Copies)</Dimension><Dimension Name="Invalid nested domain">sgn(arcsin(2))</Dimension></Feature></Keywords>"#,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let values = decoded
        .ir()
        .model
        .parameters
        .iter()
        .map(|parameter| (parameter.name.as_str(), parameter.value.clone()))
        .collect::<std::collections::HashMap<_, _>>();
    assert_eq!(
        values["Double width"],
        Some(ParameterValue::Length(Length(8.0)))
    );
    assert_eq!(
        values["Per copy"],
        Some(ParameterValue::Length(Length(8.0 / 3.0)))
    );
    assert_eq!(values["Forward"], Some(ParameterValue::Length(Length(3.0))));
    assert_eq!(
        values["Scientific"],
        Some(ParameterValue::Length(Length(0.004)))
    );
    assert_eq!(
        values["Mixed units"],
        Some(ParameterValue::Length(Length(
            304.8 + 25.4 + 0.0254 + 25.4e-6 + 1.0e-3 + 1.0e-6 + 1.0e-7
        )))
    );
    assert_eq!(values["Power"], Some(ParameterValue::Integer(512)));
    assert!(
        matches!(values["Sine"], Some(ParameterValue::Real(value)) if (value - 0.5).abs() < 1e-12)
    );
    assert!(matches!(
        values["Inverse sine"],
        Some(ParameterValue::Angle(cadmpeg_ir::features::Angle(value)))
            if (value - std::f64::consts::FRAC_PI_6).abs() < 1e-12
    ));
    assert_eq!(
        values["Absolute"],
        Some(ParameterValue::Length(Length(2.0)))
    );
    assert_eq!(values["Root"], Some(ParameterValue::Real(3.0)));
    assert_eq!(values["Sign negative"], Some(ParameterValue::Integer(-1)));
    assert_eq!(values["Sign zero"], Some(ParameterValue::Integer(0)));
    assert_eq!(values["Sign positive"], Some(ParameterValue::Integer(1)));
    assert_eq!(
        values["Pi"],
        Some(ParameterValue::Real(std::f64::consts::PI))
    );
    assert_eq!(
        values["Conditional"],
        Some(ParameterValue::Length(Length(8.0)))
    );
    assert_eq!(values["Leading equals"], Some(ParameterValue::Integer(2)));
    assert_eq!(values["Comparison"], Some(ParameterValue::Boolean(true)));
    assert_eq!(values["Invalid"], None);
    assert_eq!(values["Invalid area"], None);
    assert_eq!(values["Invalid branches"], None);
    assert_eq!(values["Invalid nested domain"], None);
    let ordinal = |name: &str| {
        decoded
            .ir()
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == name)
            .unwrap()
            .ordinal
    };
    assert!(ordinal("Later") < ordinal("Forward"));
    assert!(!cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message.contains("parameter dependency")));
}

#[test]
fn decode_projects_evaluated_equations_into_feature_semantics() {
    use cadmpeg_ir::features::{
        BooleanOp, ExtrudeExtent, ExtrudeSide, FeatureDefinition, Length, Termination,
    };

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Equation boss" Type="BossExtrude" id="7" Operation="Join" EndCondition="Blind"><Dimension Name="Base">4mm</Dimension><Dimension Name="Depth">Base * 2</Dimension></Extrusion></Keywords>"#,
    ));

    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(8.0)
                    },
                    ..
                }
            },
            op: BooleanOp::Join,
            ..
        }
    ));
    let depth = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Depth")
        .expect("depth parameter");
    assert_eq!(depth.expression, "Base * 2");
    assert_eq!(
        depth.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(Length(8.0)))
    );
    let native = &sldprt_native(decoded.ir()).feature_histories[0].features[0];
    assert_eq!(native.parameters["Depth"], "Base * 2");

    decoded.ir_mut().model.features[0].name = Some("Renamed equation boss".into());
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        sldprt_native(regenerated.ir()).feature_histories[0].features[0].parameters["Depth"],
        "Base * 2"
    );
    assert!(matches!(
        regenerated.ir().model.features[0].definition,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(8.0)
                    },
                    ..
                }
            },
            ..
        }
    ));
}

#[test]
fn equations_container_projects_a_typed_tree_node_owning_global_parameters() {
    use cadmpeg_ir::features::{
        ExtrudeExtent, ExtrudeSide, FeatureDefinition, FeatureTreeNodeRole, Length, ParameterValue,
        Termination,
    };

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Equations" Type="EquationDriven" id="7"><Dimension Name="Width">4mm</Dimension></Feature><Extrusion Name="Equation boss" Type="BossExtrude" id="8" Operation="Join" EndCondition="Blind"><Dimension Name="Depth">Width * 2</Dimension></Extrusion></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let equations = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Equations"))
        .expect("equations node");
    assert!(matches!(
        equations.definition,
        FeatureDefinition::TreeNode {
            role: FeatureTreeNodeRole::Equations,
            ..
        }
    ));
    let width = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Width")
        .expect("width parameter");
    assert_eq!(width.owner.as_ref(), Some(&equations.id));
    let depth = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Depth")
        .expect("depth parameter");
    assert_eq!(depth.dependencies, vec![width.id.clone()]);
    assert_eq!(depth.value, Some(ParameterValue::Length(Length(8.0))));

    let extrusion = decoded
        .ir()
        .model
        .features
        .iter()
        .position(|feature| feature.name.as_deref() == Some("Equation boss"))
        .expect("extrusion");
    decoded.ir_mut().model.features[extrusion].name = Some("Renamed equation boss".into());
    let FeatureDefinition::Extrude { extent, .. } =
        &mut decoded.ir_mut().model.features[extrusion].definition
    else {
        panic!("typed extrusion");
    };
    *extent = ExtrudeExtent::OneSided {
        side: ExtrudeSide {
            termination: Termination::Blind {
                length: Length(12.0),
            },
            draft: None,
            offset: None,
        },
    };
    let depth = decoded
        .ir_mut()
        .model
        .parameters
        .iter_mut()
        .find(|parameter| parameter.name == "Depth")
        .expect("depth parameter");
    depth.expression = "Width * 3".into();
    depth.value = Some(ParameterValue::Length(Length(12.0)));

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let equations = regenerated
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Equations"))
        .expect("equations node");
    assert!(matches!(
        equations.definition,
        FeatureDefinition::TreeNode {
            role: FeatureTreeNodeRole::Equations,
            ..
        }
    ));
    let depth = regenerated
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Depth")
        .expect("depth parameter");
    assert_eq!(depth.expression, "Width * 3");
    assert_eq!(depth.value, Some(ParameterValue::Length(Length(12.0))));
    assert_eq!(depth.dependencies.len(), 1);
    let extrusion = regenerated
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Renamed equation boss"))
        .expect("extrusion");
    assert!(matches!(
        extrusion.definition,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(12.0)
                    },
                    ..
                }
            },
            ..
        }
    ));
}

#[test]
fn feature_rename_rewrites_only_its_qualified_parameter_references() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Sketch1" Type="Sketch" id="10"><Dimension Name="D1">2mm</Dimension></Feature><Feature Name="Sketch2" Type="Sketch" id="11"><Dimension Name="D1">3mm</Dimension></Feature><Feature Name="Equations" Type="EquationDriven" id="12"><Dimension Name="Result">D1@Sketch1 + D1@Sketch2</Dimension></Feature></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    decoded
        .ir_mut()
        .model
        .features
        .iter_mut()
        .find(|feature| feature.name.as_deref() == Some("Sketch1"))
        .unwrap()
        .name = Some("Profile".into());

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let result = regenerated
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Result")
        .unwrap();
    assert_eq!(result.expression, "D1@Profile + D1@Sketch2");
    assert_eq!(result.dependencies.len(), 2);
}

#[test]
fn decode_projects_cut_extrude_with_canonical_length() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Cut" Type="CutExtrude" id="9"><Dimension Name="Depth">0.5in</Dimension></Extrusion></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();

    assert!(matches!(
        &decoded.ir().model.features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::Extrude {
            extent: cadmpeg_ir::features::ExtrudeExtent::OneSided {
                side: cadmpeg_ir::features::ExtrudeSide {
                    termination: cadmpeg_ir::features::Termination::Blind {
                        length: cadmpeg_ir::features::Length(12.7),
                    },
                    ..
                }
            },
            op: cadmpeg_ir::features::BooleanOp::Cut,
            ..
        }
    ));
}

#[test]
fn decode_projects_compact_extrusion_with_unresolved_extent() {
    use cadmpeg_ir::features::{
        BooleanOp, ExtrudeExtent, ExtrudeSide, FeatureDefinition, ProfileRef, Termination,
    };

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Compact" Type="Extrusion" id="9"/></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Unresolved(_),
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Unresolved,
                    ..
                }
            },
            op: BooleanOp::Unresolved,
            ..
        }
    ));

    decoded.ir_mut().model.features[0].name = Some("Renamed compact extrusion".into());
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        regenerated.ir().model.features[0].definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Unresolved(_),
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Unresolved,
                    ..
                }
            },
            ..
        }
    ));
}

#[test]
fn decode_does_not_globalize_configuration_local_extrusion_termination() {
    use cadmpeg_ir::features::{ExtrudeExtent, ExtrudeSide, FeatureDefinition, Termination};

    fn compact_extrusion_payload(through_all: bool) -> Vec<u8> {
        let mut payload = resolved_feature_classes_with_ids(&[("moExtrusion_c", "Boss", 9)]);
        let offset = payload.len();
        payload.resize(offset + 104, 0);
        if through_all {
            payload[offset..offset + 2].copy_from_slice(&[0x0c, 0x8e]);
            payload[offset + 4] = 1;
            payload[offset + 18] = 1;
            payload[offset + 30..offset + 34].copy_from_slice(&[1, 0, 0, 1]);
            payload[offset + 92] = 1;
        }
        payload
    }

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="Default"/><Configuration Name="Blind"/><Extrusion Name="Boss" Type="Extrusion" id="9"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &compact_extrusion_payload(true),
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-1-ResolvedFeatures",
        &compact_extrusion_payload(false),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let feature = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Boss"))
        .unwrap();
    assert!(matches!(
        feature.definition,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Unresolved,
                    ..
                }
            },
            ..
        }
    ));
    let feature_id = feature.id.clone();
    assert!(matches!(
        decoded.ir().model.configurations[0]
            .feature_states
            .get(&feature_id)
            .map(|state| &state.definition),
        Some(FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::ThroughAll,
                    ..
                }
            },
            ..
        })
    ));
    assert!(matches!(
        decoded.ir().model.configurations[1]
            .feature_states
            .get(&feature_id)
            .map(|state| &state.definition),
        Some(FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Unresolved,
                    ..
                }
            },
            ..
        })
    ));
    assert!(decoded
        .ir()
        .model
        .configurations
        .iter()
        .all(
            |configuration| configuration.feature_states.len() == decoded.ir().model.features.len()
        ));
    let round_trip =
        cadmpeg_ir::CadIr::from_json(&serde_json::to_string(decoded.ir()).unwrap()).unwrap();
    assert_eq!(
        round_trip.model.configurations[0]
            .feature_states
            .get(&feature_id),
        decoded.ir().model.configurations[0]
            .feature_states
            .get(&feature_id)
    );

    let mut edited = decoded.ir().clone();
    let replacement = edited.model.configurations[0].feature_states[&feature_id].clone();
    edited.model.configurations[1]
        .feature_states
        .insert(feature_id, replacement);
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(&edited, decoded.source_fidelity(), &mut Vec::new())
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("configuration design-state edit has no complete native lane encoding"),
        "unexpected error: {error}"
    );
}

#[test]
fn decode_binds_adjacent_profile_feature_to_extrusion() {
    use cadmpeg_ir::features::{FeatureDefinition, ProfileRef};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Sketch Name="Profile" Type="Sketch" id="8"/>
            <Extrusion Name="Boss" Type="Extrusion" id="9"/>
            <Sketch Name="Following" Type="Sketch" id="10"/>
        </Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[
            ("moProfileFeature_c", "Profile", 8),
            ("moExtrusion_c", "Boss", 9),
            ("moProfileFeature_c", "Following", 10),
        ]),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let profile = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Profile"))
        .unwrap();
    let extrusion = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Boss"))
        .unwrap();
    assert!(matches!(
        &extrusion.definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Feature(feature),
            ..
        } if feature == &profile.id
    ));
    assert_eq!(extrusion.dependencies, vec![profile.id.clone()]);
}

#[test]
fn decode_does_not_globalize_configuration_local_adjacent_profile() {
    use cadmpeg_ir::features::{FeatureDefinition, ProfileRef};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="Default"/><Configuration Name="Alternate"/><Sketch Name="Profile A" Type="Sketch" id="7"/><Sketch Name="Profile B" Type="Sketch" id="8"/><Extrusion Name="Boss" Type="Extrusion" id="9"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[
            ("moProfileFeature_c", "Profile A", 7),
            ("moExtrusion_c", "Boss", 9),
        ]),
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-1-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[
            ("moProfileFeature_c", "Profile B", 8),
            ("moExtrusion_c", "Boss", 9),
        ]),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let extrusion = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Boss"))
        .unwrap();
    assert!(matches!(
        &extrusion.definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Unresolved(owner),
            ..
        } if owner == extrusion.native_ref.as_deref().unwrap()
    ));
    let extrusion_id = extrusion.id.clone();
    let profile_a = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Profile A"))
        .unwrap();
    let profile_b = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Profile B"))
        .unwrap();
    let state_a = &decoded.ir().model.configurations[0].feature_states[&extrusion_id];
    let state_b = &decoded.ir().model.configurations[1].feature_states[&extrusion_id];
    assert!(matches!(
        &state_a.definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Feature(profile),
            ..
        } if profile == &profile_a.id
    ));
    assert!(matches!(
        &state_b.definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Feature(profile),
            ..
        } if profile == &profile_b.id
    ));
    assert_eq!(state_a.dependencies, vec![profile_a.id.clone()]);
    assert_eq!(state_b.dependencies, vec![profile_b.id.clone()]);
}

#[test]
fn decode_binds_following_profile_marked_as_dissected_child() {
    use cadmpeg_ir::features::{FeatureDefinition, ProfileRef};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Sketch Name="Previous" Type="Sketch" id="7"/>
            <Extrusion Name="Boss" Type="Extrusion" id="9"/>
            <Sketch Name="Profile&lt;3&gt;" Type="Sketch" id="8" Description="Profile&lt;3&gt;"/>
        </Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[
            ("moProfileFeature_c", "Previous", 7),
            ("moICE_c", "Boss", 9),
            ("moProfileFeature_c", "Profile<3>", 8),
        ]),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let profile = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Profile<3>"))
        .unwrap();
    let extrusion = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Boss"))
        .unwrap();
    assert!(matches!(
        &extrusion.definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Feature(feature),
            ..
        } if feature == &profile.id
    ));
    assert_eq!(extrusion.dependencies, vec![profile.id.clone()]);
}

#[test]
fn decode_binds_profile_to_inline_extrusion_with_ambiguous_class_token() {
    use cadmpeg_ir::features::{BooleanOp, FeatureDefinition, ProfileRef};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Sketch Name="Profile" Type="Sketch" id="8"/>
            <Extrusion Name="Cut" Type="Localized" id="9"/>
        </Keywords>"#,
    ));
    let mut payload = resolved_feature_classes_with_ids(&[("moProfileFeature_c", "Profile", 8)]);
    payload.extend_from_slice(&0x84c5u16.to_le_bytes());
    payload.extend_from_slice(&[0x04, 0x80, 0xff, 0xfe, 0xff, 3]);
    for unit in "Cut".encode_utf16() {
        payload.extend_from_slice(&unit.to_le_bytes());
    }
    payload.extend_from_slice(&[0; 4]);
    payload.extend_from_slice(&[0xca, 1, 2, 0x40]);
    payload.extend_from_slice(&9u32.to_le_bytes());
    payload.extend_from_slice(&[0; 4]);
    payload.extend_from_slice(&[0xff, 0xfe, 0xff]);
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &payload,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let profile = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Profile"))
        .unwrap();
    let extrusion = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Cut"))
        .unwrap();
    assert!(matches!(
        &extrusion.definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Feature(feature),
            op: BooleanOp::Cut,
            ..
        } if feature == &profile.id
    ));
}

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
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
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
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
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
fn decode_projects_generic_extrusion_with_explicit_operation() {
    use cadmpeg_ir::features::{
        BooleanOp, ExtrudeExtent, ExtrudeSide, FeatureDefinition, Length, Termination,
    };

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Generic" Type="Extrusion" id="10" Operation="NewBody"><Dimension Name="Depth">6mm</Dimension></Extrusion></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(6.0),
                    },
                    ..
                }
            },
            op: BooleanOp::NewBody,
            ..
        }
    ));
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
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
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

    let FeatureDefinition::Fillet { groups } = &mut decoded.ir_mut().model.features[2].definition
    else {
        panic!("typed custom fillet");
    };
    let RadiusSpec::Constant { radius } = &mut groups[0].radius else {
        panic!("constant fillet");
    };
    *radius = Length(2.5);
    decoded.ir_mut().model.features[2]
        .source_properties
        .insert("Algorithm".into(), "FaceBlend".into());
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let mut regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
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
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
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
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
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
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
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
    assert!(decoded.ir().model.configurations[0].active.is_active());
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
fn decode_projects_fixed_reference_plane_frame() {
    use cadmpeg_ir::features::FeatureDefinition;
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut resolved = resolved_feature_classes_with_ids(&[("moRefPlane_c", "Plano", 42)]);
    resolved.extend_from_slice(&[0xff, 0xff, 0x01, 0x00]);
    resolved.extend_from_slice(&("moFixedRefPlnData_c".len() as u16).to_le_bytes());
    resolved.extend_from_slice(b"moFixedRefPlnData_c");
    let mut frame = [0u8; 97];
    frame[0..8].copy_from_slice(&2.5f64.to_le_bytes());
    frame[8..16].copy_from_slice(&(-0.25f64).to_le_bytes());
    frame[16..24].copy_from_slice(&1.5f64.to_le_bytes());
    frame[24..32].copy_from_slice(&(-1.0f64).to_le_bytes());
    frame[32..40].copy_from_slice(&0.0f64.to_le_bytes());
    frame[40..48].copy_from_slice(&0.0f64.to_le_bytes());
    frame[48] = 1;
    frame[49..57].copy_from_slice(&0.0f64.to_le_bytes());
    frame[57..65].copy_from_slice(&0.0f64.to_le_bytes());
    frame[65..73].copy_from_slice(&(-1.0f64).to_le_bytes());
    frame[73..81].copy_from_slice(&0.0f64.to_le_bytes());
    frame[81..89].copy_from_slice(&(-1.0f64).to_le_bytes());
    frame[89..97].copy_from_slice(&0.0f64.to_le_bytes());
    resolved.extend_from_slice(&frame);

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Plano" Type="Plano" id="42"/></Keywords>"#,
    ));
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
        FeatureDefinition::DatumPlane {
            origin: Point3 {
                x: 2500.0,
                y: -250.0,
                z: 1500.0,
            },
            normal: Vector3 {
                x: -1.0,
                y: 0.0,
                z: 0.0,
            },
            u_axis: Vector3 {
                x: 0.0,
                y: 0.0,
                z: -1.0,
            },
        }
    ));
}

#[test]
fn decode_rejects_nonorthogonal_fixed_reference_plane_frame() {
    use cadmpeg_ir::features::FeatureDefinition;

    let mut resolved = resolved_feature_classes_with_ids(&[("moRefPlane_c", "Plane", 42)]);
    resolved.extend_from_slice(&[0xff, 0xff, 0x01, 0x00]);
    resolved.extend_from_slice(&("moFixedRefPlnData_c".len() as u16).to_le_bytes());
    resolved.extend_from_slice(b"moFixedRefPlnData_c");
    let mut frame = [0u8; 97];
    frame[24..32].copy_from_slice(&1.0f64.to_le_bytes());
    frame[48] = 1;
    frame[49..57].copy_from_slice(&1.0f64.to_le_bytes());
    frame[73..81].copy_from_slice(&1.0f64.to_le_bytes());
    resolved.extend_from_slice(&frame);

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Plane" Type="Plane" id="42"/></Keywords>"#,
    ));
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
        FeatureDefinition::DatumPlaneUnresolved
    ));
}

#[test]
fn incomplete_coordinate_system_projects_as_typed_unresolved() {
    use cadmpeg_ir::features::FeatureDefinition;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Fixture" Type="Coordinate System" id="28"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();

    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::DatumCoordinateSystemUnresolved
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
        } if (angle.0 - std::f64::consts::PI).abs() < 1e-12
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
        br#"<Keywords><Feature Name="Sweep" Type="Localized" id="137"/></Keywords>"#,
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

    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
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
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
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
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
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

    SldprtCodec
        .write_preserved_with_source_fidelity(
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
fn decode_projects_hyphenated_extrusion_operations() {
    for (kind, expected) in [
        ("Boss-Extrude", cadmpeg_ir::features::BooleanOp::Join),
        ("Cut-Extrude", cadmpeg_ir::features::BooleanOp::Cut),
    ] {
        let mut source = sldprt_with_body(&triangle_body());
        source.extend(make_block(
            0x42,
            "Contents/Keywords",
            format!(
                "<Keywords><Extrusion Name=\"Extrude1\" Type=\"{kind}\"><Dimension Name=\"D1\">25</Dimension></Extrusion></Keywords>"
            )
            .as_bytes(),
        ));

        let decoded = SldprtCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap();
        let feature = decoded
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some("Extrude1"))
            .expect("projected extrusion feature");
        assert!(matches!(
            &feature.definition,
            cadmpeg_ir::features::FeatureDefinition::Extrude { op, .. } if *op == expected
        ));
    }
}

#[test]
fn decode_binds_generic_extrusion_to_its_dissectable_sketch_child() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Extrude1" Type="Extrusion" id="8" DissectableChildren="3"><Dimension Name="D1">25</Dimension></Extrusion><Sketch Name="Sketch1" Type="Sketch" id="3"/></Keywords>"#,
    ));
    let original = source.clone();

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let extrusion = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Extrude1"))
        .expect("projected extrusion feature");
    let sketch = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Sketch1"))
        .expect("projected sketch feature");
    assert_eq!(extrusion.dependencies, vec![sketch.id.clone()]);
    assert!(sketch.ordinal < extrusion.ordinal);
    assert!(matches!(
        &extrusion.definition,
        cadmpeg_ir::features::FeatureDefinition::Extrude {
            profile: cadmpeg_ir::features::ProfileRef::Feature(profile),
            ..
        } if profile == &sketch.id
    ));
    cadmpeg_test_support::roundtrip::verbatim_replay_holds(
        &SldprtCodec,
        "decode_projects_sketch_feature_dependencies",
        &original,
    );
}

#[test]
fn decode_projects_feature_input_extrusion_operations() {
    fn operation_payload(
        code: u32,
        object_id: u32,
        name: &str,
        class_name: &str,
        direct_class: bool,
        padding: usize,
    ) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&code.to_le_bytes());
        payload.extend(std::iter::repeat_n(0, padding));
        if direct_class {
            payload.extend_from_slice(&[0xff, 0xff, 0x01, 0x00]);
            payload.extend_from_slice(&(class_name.len() as u16).to_le_bytes());
            payload.extend_from_slice(class_name.as_bytes());
        } else {
            payload.extend_from_slice(&0x84d8u16.to_le_bytes());
        }
        payload.extend_from_slice(&[0x04, 0x80, 0xff, 0xfe, 0xff]);
        payload.push(name.encode_utf16().count() as u8);
        for unit in name.encode_utf16() {
            payload.extend_from_slice(&unit.to_le_bytes());
        }
        payload.extend_from_slice(&[0; 8]);
        payload.extend_from_slice(&object_id.to_le_bytes());
        payload
    }

    fn inline_operation_payload(family: u8, operation: u8, object_id: u32) -> Vec<u8> {
        let class_name = if family == 0x40 {
            "moExtrusion_c"
        } else {
            "moICE_c"
        };
        let mut payload = operation_payload(14, object_id, "Extrude1", class_name, true, 8);
        payload.truncate(payload.len() - 12);
        payload.extend_from_slice(&[0; 4]);
        payload.extend_from_slice(&[family, 1, operation, 0x40]);
        payload.extend_from_slice(&object_id.to_le_bytes());
        payload.extend_from_slice(&[0; 4]);
        payload.extend_from_slice(&[0xff, 0xfe, 0xff]);
        payload
    }

    for (code, expected, class_name, layouts) in [
        (
            3,
            cadmpeg_ir::features::BooleanOp::Join,
            "moICE_c",
            &[(true, 8), (true, 4), (false, 8), (false, 4)][..],
        ),
        (
            1,
            cadmpeg_ir::features::BooleanOp::Join,
            "moExtrusion_c",
            &[(true, 8), (true, 4)][..],
        ),
        (
            1,
            cadmpeg_ir::features::BooleanOp::Cut,
            "moICE_c",
            &[(true, 8), (true, 4)][..],
        ),
        (
            2,
            cadmpeg_ir::features::BooleanOp::Cut,
            "moICE_c",
            &[(true, 8), (true, 4)][..],
        ),
        (
            10,
            cadmpeg_ir::features::BooleanOp::Cut,
            "moICE_c",
            &[(true, 8), (true, 4)][..],
        ),
        (
            0,
            cadmpeg_ir::features::BooleanOp::Cut,
            "moICE_c",
            &[(true, 8), (true, 4)][..],
        ),
        (
            22_993,
            cadmpeg_ir::features::BooleanOp::Cut,
            "moICE_c",
            &[(true, 8), (true, 4)][..],
        ),
    ] {
        for &(direct_class, padding) in layouts {
            let mut source = sldprt_with_body(&triangle_body());
            add_solidworks_version(&mut source, if padding == 8 { 17_000 } else { 11_000 });
            source.extend(make_block(
                0x42,
                "Contents/Keywords",
                br#"<Keywords><Extrusion Name="Extrude1" Type="Extrusion" id="8"><Dimension Name="D1">25</Dimension></Extrusion></Keywords>"#,
            ));
            source.extend(make_block(
                0x45,
                "Contents/Config-0-ResolvedFeatures",
                &operation_payload(code, 8, "Extrude1", class_name, direct_class, padding),
            ));

            let decoded = SldprtCodec
                .decode(&mut Cursor::new(source), &DecodeOptions::default())
                .unwrap();
            let feature = decoded
                .ir()
                .model
                .features
                .iter()
                .find(|feature| feature.name.as_deref() == Some("Extrude1"))
                .expect("projected extrusion feature");
            assert!(
                matches!(
                    &feature.definition,
                    cadmpeg_ir::features::FeatureDefinition::Extrude { op, .. } if *op == expected
                ),
                "code {code}, class {class_name}, direct {direct_class}, padding {padding}: {:?}",
                feature.definition
            );
        }
    }

    for code in [4, 11, 20] {
        let mut source = sldprt_with_body(&triangle_body());
        source.extend(make_block(
            0x42,
            "Contents/Keywords",
            br#"<Keywords><Extrusion Name="Extrude1" Type="Extrusion" id="8"><Dimension Name="D1">25</Dimension></Extrusion></Keywords>"#,
        ));
        source.extend(make_block(
            0x45,
            "Contents/Config-0-ResolvedFeatures",
            &operation_payload(code, 8, "Extrude1", "moICE_c", true, 8),
        ));
        let decoded = SldprtCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap();
        let feature = decoded
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some("Extrude1"))
            .expect("projected extrusion feature");
        assert!(matches!(
            &feature.definition,
            cadmpeg_ir::features::FeatureDefinition::Extrude {
                op: cadmpeg_ir::features::BooleanOp::Unresolved,
                ..
            }
        ));
        if code == 11 {
            assert!(decoded
                .report()
                .losses
                .iter()
                .any(|loss| loss.message.contains(
                    "typed feature(s) retain native or unresolved required operation operands"
                )));
        }
    }

    for (kind, expected) in [
        ("BossExtrude", cadmpeg_ir::features::BooleanOp::Join),
        ("CutExtrude", cadmpeg_ir::features::BooleanOp::Cut),
    ] {
        let mut source = sldprt_with_body(&triangle_body());
        add_solidworks_version(&mut source, 17_000);
        source.extend(make_block(
            0x42,
            "Contents/Keywords",
            format!(
                "<Keywords><Extrusion Name=\"Extrude1\" Type=\"{kind}\" id=\"8\"><Dimension Name=\"D1\">25</Dimension></Extrusion></Keywords>"
            )
            .as_bytes(),
        ));
        source.extend(make_block(
            0x45,
            "Contents/Config-0-ResolvedFeatures",
            &operation_payload(11, 8, "Extrude1", "moICE_c", true, 8),
        ));
        let decoded = SldprtCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap();
        let feature = decoded
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some("Extrude1"))
            .expect("projected extrusion feature");
        assert!(matches!(
            &feature.definition,
            cadmpeg_ir::features::FeatureDefinition::Extrude { op, .. } if *op == expected
        ));
    }

    for (family, operation, expected) in [
        (0x40, 0, cadmpeg_ir::features::BooleanOp::Join),
        (0xca, 2, cadmpeg_ir::features::BooleanOp::Cut),
    ] {
        let mut source = sldprt_with_body(&triangle_body());
        source.extend(make_block(
            0x42,
            "Contents/Keywords",
            br#"<Keywords><Extrusion Name="Extrude1" Type="Extrusion" id="8"><Dimension Name="D1">25</Dimension></Extrusion></Keywords>"#,
        ));
        source.extend(make_block(
            0x45,
            "Contents/Config-0-ResolvedFeatures",
            &inline_operation_payload(family, operation, 8),
        ));
        let decoded = SldprtCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap();
        let feature = decoded
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some("Extrude1"))
            .expect("projected extrusion feature");
        assert!(matches!(
            &feature.definition,
            cadmpeg_ir::features::FeatureDefinition::Extrude { op, .. } if *op == expected
        ));
    }
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

#[test]
fn decode_projects_owned_native_sketch_relation() {
    use cadmpeg_ir::sketches::SketchConstraintDefinition;

    let mut source = sldprt_with_nested_sketch_profile(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
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
    let cadmpeg_ir::features::FeatureDefinition::Sketch {
        space: cadmpeg_ir::features::SketchSpace::Planar,
        sketch: Some(sketch),
        ..
    } = &feature.definition
    else {
        panic!("bound sketch feature");
    };
    let native = sldprt_native(decoded.ir());
    assert!(native.feature_input_lanes[0]
        .sketch_entities
        .iter()
        .all(|entity| entity.feature_ref.as_deref() == feature.native_ref.as_deref()));
    let parameter = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.owner.as_ref() == Some(&feature.id) && parameter.name == "D1")
        .expect("projected relation parameter");
    assert_eq!(parameter.expression, "25mm");
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(25.0)
        ))
    );
    let constraint = decoded
        .ir()
        .model
        .sketch_constraints
        .iter()
        .find(|constraint| constraint.native_ref.is_some())
        .expect("projected native relation");
    assert_eq!(&constraint.sketch, sketch);
    assert!(constraint
        .native_ref
        .as_deref()
        .is_some_and(|id| id.starts_with("sldprt:feature-input:relation-instance#")));
    assert!(matches!(
        &constraint.definition,
        SketchConstraintDefinition::Native {
            native_kind,
            entities,
            parameter: Some(relation_parameter),
            operands,
            ..
        } if native_kind == "sgPntPntDist"
            && entities.is_empty()
            && relation_parameter == &parameter.id
            && operands.len() == 2
            && operands[0].native_kind == "d6"
            && operands[0].object_index == 0
            && operands[0].native_ref.is_some()
            && operands[1].native_kind == "d6"
            && operands[1].object_index == 2
            && operands[1].native_ref.is_none()
    ));
    let findings = cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new()).findings;
    assert!(findings.is_empty(), "{findings:#?}");
    SldprtCodec
        .write_preserved_with_source_fidelity(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut Vec::new(),
        )
        .unwrap();
}

#[test]
fn decode_groups_compact_relation_scalar_pair() {
    use cadmpeg_ir::sketches::SketchConstraintDefinition;

    let mut source = sldprt_with_compact_relation_pair(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(decoded.ir());
    let [relation] = native.feature_input_lanes[0].relation_instances.as_slice() else {
        panic!("one compact relation instance");
    };
    assert_eq!(relation.scalar_refs.len(), 2);
    let driving = native.feature_input_lanes[0]
        .scalars
        .iter()
        .find(|scalar| scalar.role == crate::records::FeatureInputScalarRole::Driving)
        .expect("driving scalar");
    let display = native.feature_input_lanes[0]
        .scalars
        .iter()
        .find(|scalar| scalar.role == crate::records::FeatureInputScalarRole::Display)
        .expect("display scalar");
    assert_eq!(
        relation.parameter_scalar_ref.as_deref(),
        Some(driving.id.as_str())
    );
    assert_eq!(
        relation.display_scalar_ref.as_deref(),
        Some(display.id.as_str())
    );
    assert_eq!(relation.operands.len(), 2);
    assert_eq!(relation.operands[0].entity_index, 0);
    assert_eq!(relation.operands[1].entity_index, 2);

    let constraint = decoded
        .ir()
        .model
        .sketch_constraints
        .iter()
        .find(|constraint| constraint.native_ref.as_deref() == Some(relation.id.as_str()))
        .expect("projected compact relation");
    assert!(matches!(
        &constraint.definition,
        SketchConstraintDefinition::Native {
            native_kind,
            parameter: Some(parameter),
            ..
        } if native_kind == "sgPntPntDist"
            && decoded.ir().model.parameters.iter().any(|candidate| {
                &candidate.id == parameter
                    && candidate.native_ref.as_deref() == Some(driving.id.as_str())
            })
    ));
    SldprtCodec
        .write_preserved_with_source_fidelity(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut Vec::new(),
        )
        .unwrap();
}

#[test]
fn decode_starts_another_relation_after_two_repeated_operand_scalars() {
    let mut source = sldprt_with_tagged_compact_relation_names(
        &triangle_body(),
        "sgPntPntDist",
        [[0xd6, 0x80]; 2],
        &["Sketch1", "D1", "D2", "D3"],
    );
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(decoded.ir());
    assert_eq!(native.feature_input_lanes[0].relation_instances.len(), 2);
    assert_eq!(
        native.feature_input_lanes[0]
            .relation_instances
            .iter()
            .map(|relation| relation.scalar_refs.len())
            .collect::<Vec<_>>(),
        vec![2, 1]
    );
}

#[test]
fn decode_groups_native_tagged_point_line_relations() {
    use cadmpeg_ir::sketches::SketchConstraintDefinition;

    let mut source = sldprt_with_tagged_compact_relation(
        &triangle_body(),
        "sgPntLineDist",
        [[0x7b, 0x83], [0x86, 0x83]],
    );
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let parameter = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D2")
        .expect("driving point-line parameter");
    assert_eq!(parameter.expression, "25mm");
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(25.0)
        ))
    );
    let native = sldprt_native(decoded.ir());
    let lane = &native.feature_input_lanes[0];
    assert_eq!(lane.references.len(), 4);
    assert!(lane
        .references
        .iter()
        .enumerate()
        .all(|(ordinal, reference)| {
            reference.kind
                == crate::records::FeatureInputOperandKind::Native(if ordinal % 2 == 0 {
                    0x837b
                } else {
                    0x8386
                })
        }));
    let [relation] = lane.relation_instances.as_slice() else {
        panic!("one point-line relation instance");
    };
    assert_eq!(
        relation.family,
        crate::records::FeatureInputRelationFamily::PointLineDistance
    );
    let constraint = decoded
        .ir()
        .model
        .sketch_constraints
        .iter()
        .find(|constraint| constraint.native_ref.as_deref() == Some(relation.id.as_str()))
        .expect("projected point-line relation");
    assert!(matches!(
        &constraint.definition,
        SketchConstraintDefinition::Native {
            native_kind,
            operands,
            ..
        } if native_kind == "sgPntLineDist"
            && operands[0].native_kind == "7b83"
            && operands[1].native_kind == "8683"
    ));
    SldprtCodec
        .write_preserved_with_source_fidelity(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut Vec::new(),
        )
        .unwrap();
}

#[test]
fn decode_uses_relation_units_for_bare_integer_dimensions() {
    use cadmpeg_ir::features::{Length, ParameterValue};

    let mut source = sldprt_with_tagged_compact_relation(
        &triangle_body(),
        "sgPntPntVertDist",
        [[0xcb, 0x8d]; 2],
    );
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"><Dimension Name="D2">25</Dimension></Sketch></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let parameter = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D2")
        .expect("driving vertical-distance parameter");
    assert_eq!(parameter.expression, "25");
    assert_eq!(parameter.value, Some(ParameterValue::Length(Length(25.0))));
    assert!(parameter.native_ref.is_some());
}

#[test]
fn decode_uses_relation_units_for_boolean_shaped_dimensions() {
    use cadmpeg_ir::features::{Length, ParameterValue};

    let mut source = sldprt_with_tagged_compact_relation_scalar(
        &triangle_body(),
        "sgPntPntDist",
        [[0xd6, 0x80]; 2],
        0.001,
    );
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"><Dimension Name="D2">1</Dimension></Sketch></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let parameter = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D2")
        .expect("driving distance parameter");
    assert_eq!(parameter.expression, "1");
    assert_eq!(parameter.value, Some(ParameterValue::Length(Length(1.0))));
    assert!(parameter.native_ref.is_some());
}

#[test]
fn decode_uses_relation_units_for_bare_integer_angles() {
    use cadmpeg_ir::features::{Angle, ParameterValue};

    let mut source =
        sldprt_with_tagged_compact_relation(&triangle_body(), "sgAnglDim", [[0xda, 0x8d]; 2]);
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"><Dimension Name="D2">25</Dimension></Sketch></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let parameter = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D2")
        .expect("driving angle parameter");
    assert_eq!(parameter.expression, "25");
    assert_eq!(parameter.value, Some(ParameterValue::Angle(Angle(0.025))));
    assert!(parameter.native_ref.is_some());
}

#[test]
fn decode_groups_unary_circle_diameter_relations() {
    use cadmpeg_ir::sketches::SketchConstraintDefinition;

    let mut source = sldprt_with_tagged_compact_relation(
        &triangle_body(),
        "sgCircleDim",
        [[0xfe, 0x83], [0, 0]],
    );
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"><Dimension Name="D2">&lt;MOD-DIAM&gt;25mm</Dimension></Sketch></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(decoded.ir());
    let [relation] = native.feature_input_lanes[0].relation_instances.as_slice() else {
        panic!("one circle-diameter relation instance");
    };
    assert_eq!(
        relation.family,
        crate::records::FeatureInputRelationFamily::CircleDiameter
    );
    assert_eq!(relation.operands.len(), 1);
    assert_eq!(
        relation.operands[0].kind,
        crate::records::FeatureInputOperandKind::Native(0x83fe)
    );
    let parameter = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D2")
        .expect("diameter parameter");
    assert_eq!(
        relation.parameter_scalar_ref.as_deref(),
        parameter.native_ref.as_deref()
    );
    assert!(decoded
        .ir()
        .model
        .sketch_constraints
        .iter()
        .any(|constraint| {
            constraint.native_ref.as_deref() == Some(relation.id.as_str())
                && matches!(
                    &constraint.definition,
                    SketchConstraintDefinition::Native {
                        native_kind,
                        parameter: Some(bound_parameter),
                        operands,
                        ..
                    } if native_kind == "sgCircleDim"
                        && bound_parameter == &parameter.id
                        && operands.len() == 1
                        && operands[0].native_kind == "fe83"
                )
        }));
    SldprtCodec
        .write_preserved_with_source_fidelity(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut Vec::new(),
        )
        .unwrap();
}

#[test]
fn decode_groups_each_circle_dimension_operand_tag() {
    for tag in [
        [0xcc, 0x80],
        [0xfe, 0x83],
        [0xb6, 0x8a],
        [0x9d, 0x92],
        [0x69, 0xbd],
        [0x46, 0x81],
    ] {
        let mut source =
            sldprt_with_tagged_compact_relation(&triangle_body(), "sgCircleDim", [tag, [0, 0]]);
        source.extend(make_block(
            0x42,
            "Contents/Keywords",
            br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
        ));
        let decoded = SldprtCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap();
        let native = sldprt_native(decoded.ir());
        let [relation] = native.feature_input_lanes[0].relation_instances.as_slice() else {
            panic!("one circle-diameter relation for tag {tag:02x?}");
        };
        assert_eq!(
            relation.family,
            crate::records::FeatureInputRelationFamily::CircleDiameter
        );
        let [operand] = relation.operands.as_slice() else {
            panic!("one circle-diameter operand for tag {tag:02x?}");
        };
        assert_eq!(
            operand.kind,
            crate::records::FeatureInputOperandKind::Native(u16::from_le_bytes(tag))
        );
        assert_eq!(operand.entity_index, 0);
    }
}

#[test]
fn decode_uses_declaration_to_disambiguate_native_relation_tags() {
    let cases = [
        (
            "sgPntPntDist",
            [0x7b, 0x83],
            crate::records::FeatureInputRelationFamily::PointPointDistance,
        ),
        (
            "sgLLDist",
            [0x86, 0x83],
            crate::records::FeatureInputRelationFamily::LineLineDistance,
        ),
        (
            "sgPntPntDist",
            [0x7c, 0xbc],
            crate::records::FeatureInputRelationFamily::PointPointDistance,
        ),
        (
            "sgLLDist",
            [0x87, 0xbc],
            crate::records::FeatureInputRelationFamily::LineLineDistance,
        ),
        (
            "sgPntPntHorDist",
            [0xcb, 0x8d],
            crate::records::FeatureInputRelationFamily::PointPointHorizontalDistance,
        ),
        (
            "sgPntPntVertDist",
            [0xcb, 0x8d],
            crate::records::FeatureInputRelationFamily::PointPointVerticalDistance,
        ),
        (
            "sgAnglDim",
            [0xda, 0x8d],
            crate::records::FeatureInputRelationFamily::Angle,
        ),
    ];
    for (class, tag, family) in cases {
        let mut source = sldprt_with_tagged_compact_relation(&triangle_body(), class, [tag; 2]);
        source.extend(make_block(
            0x42,
            "Contents/Keywords",
            br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
        ));
        let decoded = SldprtCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap();
        let parameter = decoded
            .ir()
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == "D2")
            .expect("driving relation parameter");
        if family == crate::records::FeatureInputRelationFamily::Angle {
            assert_eq!(parameter.expression, "0.025rad");
            assert_eq!(
                parameter.value,
                Some(cadmpeg_ir::features::ParameterValue::Angle(
                    cadmpeg_ir::features::Angle(0.025)
                ))
            );
        } else {
            assert_eq!(parameter.expression, "25mm");
            assert_eq!(
                parameter.value,
                Some(cadmpeg_ir::features::ParameterValue::Length(
                    cadmpeg_ir::features::Length(25.0)
                ))
            );
        }
        let native = sldprt_native(decoded.ir());
        let [relation] = native.feature_input_lanes[0].relation_instances.as_slice() else {
            panic!("one native-tagged relation instance for {class}");
        };
        assert_eq!(relation.family, family);
        assert!(relation.operands.iter().all(|operand| operand.kind
            == crate::records::FeatureInputOperandKind::Native(u16::from_le_bytes(tag))));
        assert!(decoded
            .ir()
            .model
            .sketch_constraints
            .iter()
            .any(|constraint| {
                constraint.native_ref.as_deref() == Some(relation.id.as_str())
                    && matches!(
                        &constraint.definition,
                        cadmpeg_ir::sketches::SketchConstraintDefinition::Native {
                            native_kind,
                            ..
                        } if native_kind == class
                    )
            }));
    }
}

#[test]
fn decode_and_validate_compact_delete_body_selection() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Body-Delete/Keep 1" Type="Body-Delete/Keep " id="41"/></Keywords>"#,
    ));
    let mut payload =
        resolved_feature_classes_with_ids(&[("moDeleteBody_c", "Body-Delete/Keep 1", 41)]);
    payload.extend([0xff, 0xff, 0x01, 0x00]);
    payload.extend(18u16.to_le_bytes());
    payload.extend(b"moDeleteBodyData_c");
    payload.extend([0x08, 0x00]);
    let token = 0x89a4u16;
    let mut state = [0u8; 83];
    state[0..2].copy_from_slice(&token.to_le_bytes());
    state[2..11].copy_from_slice(&[0x2b, 0x80, 0x02, 0, 0, 0, 0, 0, 0]);
    state[11..15].copy_from_slice(&287u32.to_le_bytes());
    state[15..19].copy_from_slice(&287u32.to_le_bytes());
    state[47..63].fill(0xff);
    payload.extend(state);
    payload.extend([0x30, 0x80]);
    payload.extend(1u32.to_le_bytes());
    payload.extend([0; 4]);
    payload.extend(11000u32.to_le_bytes());
    payload.extend([0; 8]);
    payload.extend(2u32.to_le_bytes());
    payload.extend(287u32.to_le_bytes());
    payload.extend(115u32.to_le_bytes());
    payload.extend(u32::MAX.to_le_bytes());
    payload.extend([0; 12]);
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &payload,
    ));

    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(!decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("body delete/keep feature(s)")));
    let mut native = sldprt_native(decoded.ir());
    let [selection] = native.feature_input_lanes[0].body_selections.as_slice() else {
        panic!("one compact body selection");
    };
    assert_eq!(selection.local_body_ids, [287, 115]);
    assert_eq!(selection.body_state_ids, [287]);
    assert_eq!(
        selection.mode,
        Some(cadmpeg_ir::features::BodyRetentionMode::DeleteSelected)
    );

    let mut legacy = decoded.ir().native.namespace("sldprt").unwrap().clone();
    legacy.version = 5;
    for record in legacy
        .arenas
        .get_mut("feature_input_body_selections")
        .unwrap()
    {
        let mut fields = record.fields();
        fields.remove("mode");
        *record = cadmpeg_ir::NativeRecord::new(record.id().to_string(), fields);
    }
    let migrated = crate::native::SldprtNative::load(&legacy).unwrap();
    assert_eq!(
        migrated.feature_input_lanes[0].body_selections[0].mode,
        Some(cadmpeg_ir::features::BodyRetentionMode::DeleteSelected)
    );
    assert!(selection.feature_ref.starts_with("sldprt:history:feature#"));
    let delete_feature = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Body-Delete/Keep 1"))
        .expect("delete-body feature");
    assert!(matches!(
        &delete_feature.definition,
        cadmpeg_ir::features::FeatureDefinition::DeleteBody { bodies, mode }
            if bodies == &cadmpeg_ir::features::BodySelection::Local {
                bodies: vec!["287".into(), "115".into()],
                native: "sldprt:feature-input:body-ids:287,115".into(),
            } && *mode == cadmpeg_ir::features::BodyRetentionMode::DeleteSelected
    ));
    SldprtCodec
        .write_preserved_with_source_fidelity(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut Vec::new(),
        )
        .unwrap();

    decoded
        .ir_mut()
        .model
        .features
        .iter_mut()
        .find(|feature| feature.name.as_deref() == Some("Body-Delete/Keep 1"))
        .expect("delete-body feature")
        .name = Some("Renamed Delete Body".into());
    let mut renamed = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut renamed)
        .unwrap();
    let renamed = SldprtCodec
        .decode(&mut Cursor::new(renamed), &DecodeOptions::default())
        .unwrap();
    let renamed_native = sldprt_native(renamed.ir());
    assert!(!renamed_native.feature_histories[0].features[0]
        .properties
        .contains_key("Bodies"));
    assert_eq!(
        renamed_native.feature_input_lanes[0].body_selections[0].local_body_ids,
        [287, 115]
    );

    {
        let delete_feature = decoded
            .ir_mut()
            .model
            .features
            .iter_mut()
            .find(|feature| feature.name.as_deref() == Some("Renamed Delete Body"))
            .expect("delete-body feature");
        let cadmpeg_ir::features::FeatureDefinition::DeleteBody { bodies, .. } =
            &mut delete_feature.definition
        else {
            panic!("typed delete-body feature");
        };
        *bodies =
            cadmpeg_ir::features::BodySelection::Native("sldprt:feature-input:body-ids:287".into());
    }
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("changes a compact body selection"));

    {
        let delete_feature = decoded
            .ir_mut()
            .model
            .features
            .iter_mut()
            .find(|feature| feature.name.as_deref() == Some("Renamed Delete Body"))
            .expect("delete-body feature");
        let cadmpeg_ir::features::FeatureDefinition::DeleteBody { bodies, mode } =
            &mut delete_feature.definition
        else {
            unreachable!("typed delete-body feature");
        };
        *bodies = cadmpeg_ir::features::BodySelection::Local {
            bodies: vec!["287".into(), "115".into()],
            native: "sldprt:feature-input:body-ids:287,115".into(),
        };
        *mode = cadmpeg_ir::features::BodyRetentionMode::KeepSelected;
    }
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("changes a compact body retention mode"));

    native.feature_input_lanes[0].body_selections[0]
        .body_state_ids
        .push(287);
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    let error = native.store(&mut namespace).unwrap_err();
    assert!(
        error.to_string().contains("body selection")
            && error.to_string().contains("inconsistent ownership")
    );
    native.feature_input_lanes[0].body_selections[0].body_state_ids = vec![287];

    native.feature_input_lanes[0].body_selections[0].mode =
        Some(cadmpeg_ir::features::BodyRetentionMode::KeepSelected);
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    let error = native.store(&mut namespace).unwrap_err();
    assert!(
        error.to_string().contains("body selection")
            && error.to_string().contains("inconsistent ownership")
    );
    native.feature_input_lanes[0].body_selections[0].mode =
        Some(cadmpeg_ir::features::BodyRetentionMode::DeleteSelected);

    native.feature_input_lanes[0].body_selections[0].local_body_ids[0] = 288;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    let error = native.store(&mut namespace).unwrap_err();
    assert!(
        error.to_string().contains("body selection")
            && error.to_string().contains("inconsistent ownership")
    );
}

#[test]
fn decode_extracts_pmi_semantic_dimension() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x49,
        "Contents/PMISemanticDataDB",
        &pmi_semantic_payload(),
    ));

    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(decoded.ir());
    let [dimension] = native.pmi_dimensions.as_slice() else {
        panic!("one PMI dimension");
    };
    assert_eq!(dimension.guid, "01234567-89ab-cdef-0123-456789abcdef");
    assert_eq!(dimension.cad_text, "D1@Sketch1");
    assert_eq!(dimension.subtype, "Linear");
    assert_eq!(dimension.value, 0.025);
    assert_eq!(dimension.precision, 3);
    assert_eq!(dimension.display_text.as_deref(), Some("25.000 mm"));
    assert!(dimension.basic);
    assert!(!dimension.inspection);
    assert!(dimension.reference_only);
    assert_eq!(
        decoded.source_fidelity().annotations.provenance[&dimension.id].offset,
        dimension.offset
    );
    let parameter = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D1")
        .expect("PMI-backed parameter");
    assert_eq!(parameter.expression, "25mm");
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(25.0)
        ))
    );
    let semantic = parameter.pmi.as_ref().expect("PMI semantics");
    assert_eq!(
        semantic.subtype,
        cadmpeg_ir::features::PmiDimensionSubtype::Linear
    );
    assert_eq!(semantic.native_ref, dimension.id);
    SldprtCodec
        .write_preserved_with_source_fidelity(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut Vec::new(),
        )
        .unwrap();

    let parameter = decoded
        .ir_mut()
        .model
        .parameters
        .iter_mut()
        .find(|parameter| parameter.name == "D1")
        .expect("editable PMI-backed parameter");
    parameter.expression = "50mm".into();
    parameter.value = Some(cadmpeg_ir::features::ParameterValue::Length(
        cadmpeg_ir::features::Length(50.0),
    ));
    let semantic = parameter.pmi.as_mut().expect("editable PMI semantics");
    semantic.precision = 4;
    semantic.display_text = Some("50.000 mm".into());
    semantic.basic = false;
    semantic.inspection = true;
    semantic.reference_only = false;

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(regenerated.ir());
    let [dimension] = native.pmi_dimensions.as_slice() else {
        panic!("one regenerated PMI dimension");
    };
    assert_eq!(dimension.value, 0.05);
    assert_eq!(dimension.precision, 4);
    assert_eq!(dimension.display_text.as_deref(), Some("50.000 mm"));
    assert!(!dimension.basic);
    assert!(dimension.inspection);
    assert!(!dimension.reference_only);
}

#[test]
fn decode_extracts_array16_and_reordered_pmi_maps() {
    let items = vec![("Linear", 0.025); 16];
    let array16 = pmi_semantic_payload_record_with_items(
        "D1@Sketch1",
        "01234567-89ab-cdef-0123-456789abcdef",
        &items,
        "25.000 mm",
    );
    let reordered = pmi_semantic_payload_record_configured(
        "D1@Sketch1",
        "fedcba98-7654-3210-fedc-ba9876543210",
        &[("Linear", 0.030)],
        "30.000 mm",
        PmiPayloadOptions {
            reorder_and_extra_key: true,
            ..PmiPayloadOptions::default()
        },
    );
    for (payload, guid, value, item_count) in [
        (
            array16,
            "01234567-89ab-cdef-0123-456789abcdef",
            0.025,
            16_u32,
        ),
        (reordered, "fedcba98-7654-3210-fedc-ba9876543210", 0.030, 1),
    ] {
        let mut source = sldprt_with_body(&triangle_body());
        source.extend(make_block(0x49, "Contents/PMISemanticDataDB", &payload));
        let decoded = SldprtCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap();
        let native = sldprt_native(decoded.ir());
        let dimension = native
            .pmi_dimensions
            .iter()
            .find(|record| record.guid == guid)
            .expect("PMI dimension");
        assert_eq!(dimension.value, value);
        assert_eq!(dimension.item_count, item_count);
        assert!(decoded.report().losses.iter().all(|loss| {
            !loss.message.contains("semantic-record-malformed")
                && !loss.message.contains("failed to parse MessagePack map")
        }));
    }
}

#[test]
fn decode_reports_malformed_pmi_semantic_map() {
    let payload = pmi_semantic_payload_record_configured(
        "D1@Sketch1",
        "01234567-89ab-cdef-0123-456789abcdef",
        &[("Linear", 0.025)],
        "25.000 mm",
        PmiPayloadOptions {
            truncate_after_dim_items_key: true,
            ..PmiPayloadOptions::default()
        },
    );
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x49, "Contents/PMISemanticDataDB", &payload));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(sldprt_native(decoded.ir()).pmi_dimensions.is_empty());
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.message
            .contains("01234567-89ab-cdef-0123-456789abcdef")
            && loss.message.contains("failed to parse MessagePack map")
    }));
}

#[test]
fn multi_item_pmi_dimension_is_not_bound() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x49,
        "Contents/PMISemanticDataDB",
        &pmi_semantic_payload_record_with_items(
            "D1@Sketch1",
            "01234567-89ab-cdef-0123-456789abcdef",
            &[("Linear", 0.025), ("Linear", 0.025)],
            "25.000 mm",
        ),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(decoded.ir());
    let [dimension] = native.pmi_dimensions.as_slice() else {
        panic!("one native PMI dimension");
    };
    assert_eq!(dimension.item_count, 2);
    assert!(!decoded
        .ir()
        .model
        .parameters
        .iter()
        .any(|parameter| parameter.name == "D1" && parameter.pmi.is_some()));
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.message
            == "1 semantic dimension record(s) are not bound to parameters; 0 parameter dimension(s) retain native subtypes."
    }));
}

#[test]
fn decode_reports_unbound_pmi_semantic_dimension() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x49,
        "Contents/PMISemanticDataDB",
        &pmi_semantic_payload_for("D1@MissingFeature"),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();

    assert!(decoded.report().losses.iter().any(|loss| {
        loss.message
            == "1 semantic dimension record(s) are not bound to parameters; 0 parameter dimension(s) retain native subtypes."
    }));
}

#[test]
fn duplicate_pmi_records_share_one_parameter_and_round_trip_edits() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    for guid in [
        "01234567-89ab-cdef-0123-456789abcdef",
        "fedcba98-7654-3210-fedc-ba9876543210",
    ] {
        source.extend(make_block(
            0x49,
            "Contents/PMISemanticDataDB",
            &pmi_semantic_payload_for_with_guid("D1@Sketch1", guid),
        ));
    }

    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(sldprt_native(decoded.ir()).pmi_dimensions.len(), 2);
    assert_eq!(decoded.ir().model.parameters.len(), 1);
    assert!(decoded.report().losses.iter().all(|loss| !loss
        .message
        .contains("semantic dimension record(s) are not bound")));

    let parameter = &mut decoded.ir_mut().model.parameters[0];
    parameter.expression = "50mm".into();
    parameter.value = Some(cadmpeg_ir::features::ParameterValue::Length(
        cadmpeg_ir::features::Length(50.0),
    ));

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(regenerated.ir());
    assert_eq!(native.pmi_dimensions.len(), 2);
    assert!(native
        .pmi_dimensions
        .iter()
        .all(|dimension| dimension.value == 0.05));
}

#[test]
fn semantically_distinct_pmi_records_remain_unbound() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    for (guid, value) in [
        ("01234567-89ab-cdef-0123-456789abcdef", 0.025),
        ("fedcba98-7654-3210-fedc-ba9876543210", 0.030),
    ] {
        source.extend(make_block(
            0x49,
            "Contents/PMISemanticDataDB",
            &pmi_semantic_payload_for_with_guid_and_value("D1@Sketch1", guid, value),
        ));
    }

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.message
            == "2 semantic dimension record(s) are not bound to parameters; 0 parameter dimension(s) retain native subtypes."
    }));
}

#[test]
fn ordinate_pmi_dimensions_round_trip_typed_values() {
    use cadmpeg_ir::features::{Length, ParameterValue, PmiDimensionSubtype};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    let payload = pmi_semantic_payload_record(
        "D1@Sketch1",
        "01234567-89ab-cdef-0123-456789abcdef",
        "Ordinate",
        0.025,
        "<DIM>",
    );
    source.extend(make_block(0x49, "Contents/PMISemanticDataDB", &payload));

    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let ordinate = decoded
        .ir_mut()
        .model
        .parameters
        .iter_mut()
        .find(|parameter| parameter.name == "D1")
        .expect("ordinate parameter");
    assert_eq!(ordinate.value, Some(ParameterValue::Length(Length(25.0))));
    assert_eq!(
        ordinate.pmi.as_ref().map(|pmi| &pmi.subtype),
        Some(&PmiDimensionSubtype::Ordinate)
    );
    ordinate.expression = "50mm".into();
    ordinate.value = Some(ParameterValue::Length(Length(50.0)));

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(regenerated.ir());
    assert_eq!(
        native
            .pmi_dimensions
            .iter()
            .find(|dimension| dimension.cad_text == "D1@Sketch1")
            .map(|dimension| dimension.value),
        Some(0.05)
    );
}

#[test]
fn decode_uses_pmi_dimension_to_project_sparse_extrusion() {
    use cadmpeg_ir::features::{
        BooleanOp, ExtrudeExtent, ExtrudeSide, FeatureDefinition, Length, ProfileRef, Termination,
    };

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Boss" Type="Localized" id="42"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[("moExtrusion_c", "Boss", 42)]),
    ));
    source.extend(make_block(
        0x49,
        "Contents/PMISemanticDataDB",
        &pmi_semantic_payload_for("D1@Boss"),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Unresolved(_),
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(25.0)
                    },
                    ..
                }
            },
            op: BooleanOp::Unresolved,
            ..
        }
    ));
    let parameter = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.owner.as_ref() == Some(&decoded.ir().model.features[0].id))
        .expect("PMI extrusion parameter");
    assert_eq!(parameter.name, "D1");
    assert_eq!(parameter.expression, "25mm");
    assert!(parameter.pmi.is_some());
}

#[test]
fn decode_applies_owned_feature_units_to_resolved_scalar() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Fillet Name="Round1" Type="Fillet"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_features_payload_with_names(&[0], &["Round1", "D1"]),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let feature = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Round1"))
        .expect("projected fillet feature");
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
    assert!(parameter.native_ref.is_some());
}

#[test]
fn decode_preserves_configuration_local_parameter_values() {
    use cadmpeg_ir::features::{FeatureDefinition, Length, ParameterValue, RadiusSpec};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="Default"/><Configuration Name="Large"/><Fillet Name="Round1" Type="Fillet"><Dimension Name="D1">30mm</Dimension><Dimension Name="D2">D1 * 2</Dimension></Fillet></Keywords>"#,
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_features_payload_with_names_relation_and_scalar(
            &[0],
            &["Round1", "D1"],
            "sgPntPntDist",
            0.025,
        ),
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-1-ResolvedFeatures",
        &resolved_features_payload_with_names_relation_and_scalar(
            &[0],
            &["Round1", "D1"],
            "sgPntPntDist",
            0.050,
        ),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(!decoded.report().losses.iter().any(|loss| loss
        .message
        .contains("parameter expression(s) cannot regenerate")));
    let parameter = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D1")
        .unwrap();
    let dependent = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D2")
        .unwrap();
    assert_eq!(parameter.value, Some(ParameterValue::Length(Length(30.0))));
    assert_eq!(parameter.native_ref, None);
    assert_eq!(
        decoded.ir().model.configurations[0]
            .parameter_values
            .get(&parameter.id),
        Some(&ParameterValue::Length(Length(25.0)))
    );
    assert_eq!(
        decoded.ir().model.configurations[1]
            .parameter_values
            .get(&parameter.id),
        Some(&ParameterValue::Length(Length(50.0)))
    );
    assert_eq!(
        decoded.ir().model.configurations[0]
            .parameter_values
            .get(&dependent.id),
        Some(&ParameterValue::Length(Length(50.0)))
    );
    assert_eq!(
        decoded.ir().model.configurations[1]
            .parameter_values
            .get(&dependent.id),
        Some(&ParameterValue::Length(Length(100.0)))
    );
    let round_trip =
        cadmpeg_ir::CadIr::from_json(&serde_json::to_string(decoded.ir()).unwrap()).unwrap();
    assert_eq!(
        round_trip.model.configurations[1]
            .parameter_values
            .get(&parameter.id),
        Some(&ParameterValue::Length(Length(50.0)))
    );

    let parameter_id = parameter.id.clone();
    let dependent_id = dependent.id.clone();
    let feature_id = parameter.owner.clone();
    let mut incoherent = decoded.ir().clone();
    incoherent.model.configurations[1]
        .parameter_values
        .insert(parameter_id.clone(), ParameterValue::Length(Length(75.0)));
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            &incoherent,
            decoded.source_fidelity(),
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("configuration parameter values are inconsistent with their expressions"),
        "unexpected error: {error}"
    );

    let mut edited = decoded.ir().clone();
    edited.model.configurations[1]
        .parameter_values
        .insert(parameter_id.clone(), ParameterValue::Length(Length(75.0)));
    edited.model.configurations[1]
        .parameter_values
        .insert(dependent_id, ParameterValue::Length(Length(150.0)));
    let FeatureDefinition::Fillet { groups, .. } = &mut edited.model.configurations[1]
        .feature_states
        .get_mut(feature_id.as_ref().expect("feature-owned parameter"))
        .unwrap()
        .definition
    else {
        panic!("configuration fillet state");
    };
    groups[0].radius = RadiusSpec::Constant {
        radius: Length(75.0),
    };

    let mut conflicting = edited.clone();
    update_sldprt_native(&mut conflicting, |native| {
        let lane = native
            .feature_input_lanes
            .iter_mut()
            .find(|lane| lane.configuration.as_deref() == Some("1"))
            .unwrap();
        let scalar = &mut lane.scalars[0];
        scalar.value = 0.060;
        let offset = usize::try_from(scalar.offset).unwrap();
        lane.native_payload[offset..offset + 8].copy_from_slice(&0.060f64.to_le_bytes());
    });
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            &conflicting,
            decoded.source_fidelity(),
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("conflicting neutral and native SLDPRT configuration design-state edits"));

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&edited, decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let regenerated_parameter = regenerated
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D1")
        .unwrap();
    let regenerated_feature = regenerated
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Round1"))
        .unwrap();
    assert_eq!(
        regenerated.ir().model.configurations[1]
            .parameter_values
            .get(&regenerated_parameter.id),
        Some(&ParameterValue::Length(Length(75.0)))
    );
    assert!(matches!(
        regenerated.ir().model.configurations[1].feature_states[&regenerated_feature.id].definition,
        FeatureDefinition::Fillet {
            ref groups,
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            radius: RadiusSpec::Constant {
                radius: Length(75.0)
            },
            ..
        }])
    ));
}

#[test]
fn decode_separates_document_expression_from_evaluated_feature_scalar() {
    use cadmpeg_ir::features::{
        BooleanOp, ExtrudeExtent, ExtrudeSide, FeatureDefinition, Length, ParameterValue,
        Termination,
    };

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Boss" Type="BossExtrude" id="42"><Dimension Name="D1">2.5</Dimension></Extrusion></Keywords>"#,
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_features_payload_with_names(&[0], &["Boss", "D1"]),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let feature = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Boss"))
        .expect("projected extrusion");
    assert!(matches!(
        feature.definition,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(25.0)
                    },
                    ..
                }
            },
            op: BooleanOp::Join,
            ..
        }
    ));
    let parameter = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.owner.as_ref() == Some(&feature.id) && parameter.name == "D1")
        .expect("projected D1 parameter");
    assert_eq!(parameter.expression, "2.5");
    assert_eq!(parameter.value, Some(ParameterValue::Length(Length(25.0))));
    assert!(parameter.native_ref.is_some());
}

#[test]
fn decode_projects_nested_feature_input_profile_as_a_sketch() {
    use cadmpeg_ir::sketches::{SketchConstraintDefinition, SketchGeometry, SketchLocus};

    let source = sldprt_with_nested_sketch_profile(&triangle_body());
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();

    assert_eq!(decoded.ir().model.sketches.len(), 1);
    assert_eq!(decoded.ir().model.sketch_entities.len(), 3);
    assert_eq!(decoded.ir().model.sketch_constraints.len(), 3);
    let sketch = &decoded.ir().model.sketches[0];
    assert_eq!(sketch.configuration.as_deref(), Some("0"));
    let (origin, normal, _) = sketch
        .resolved_placement()
        .expect("resolved sketch placement");
    assert_eq!(origin, cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0));
    assert_eq!(normal, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0));
    assert_eq!(sketch.profiles.len(), 1);
    assert_eq!(sketch.profiles[0].len(), 3);
    assert!(decoded
        .ir()
        .model
        .sketch_entities
        .iter()
        .all(|entity| matches!(entity.geometry, SketchGeometry::Line { .. })));
    assert!(decoded.ir().model.sketch_entities.iter().all(|entity| {
        entity
            .native_ref
            .as_deref()
            .is_some_and(|id| id.contains(":sldprt:brep:edge#"))
            && entity.endpoint_refs.len() == 2
            && entity
                .endpoint_refs
                .iter()
                .all(|id| id.contains(":sldprt:brep:point#"))
    }));
    assert!(decoded
        .ir()
        .model
        .sketch_constraints
        .iter()
        .all(|constraint| {
            matches!(
                &constraint.definition,
                SketchConstraintDefinition::CoincidentLoci { loci }
                    if loci.len() == 2
                        && loci.iter().all(|locus| matches!(
                            locus,
                            SketchLocus::Start(_) | SketchLocus::End(_)
                        ))
            )
        }));
    assert!(sketch.native_ref.as_deref().is_some_and(|native_ref| {
        native_ref.starts_with("sldprt:feature-input:resolved-features#")
    }));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new());
    assert!(validation.is_ok(), "{:?}", validation.findings);
}

#[test]
fn decode_binds_profile_stream_by_feature_object_interval() {
    let mut source = sldprt_with_nested_sketch_profile(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let sketch = decoded
        .ir()
        .model
        .sketches
        .iter()
        .find(|sketch| sketch.name.as_deref() == Some("Sketch1"))
        .expect("named feature-input sketch");
    let feature = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Sketch1"))
        .expect("sketch history feature");
    assert!(matches!(
        &feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Planar,
            sketch: Some(id),
            ..
        } if id == &sketch.id
    ));
}

#[test]
fn decode_binds_uniquely_enclosed_profile_stream_to_sweep() {
    use cadmpeg_ir::features::{FeatureDefinition, ProfileRef};

    let mut source = sldprt_with_nested_sketch_profile(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sweep Name="Sketch1" Type="Sweep"/></Keywords>"#,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let [sketch] = decoded.ir().model.sketches.as_slice() else {
        panic!("one enclosed sweep profile stream");
    };
    let feature = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Sketch1"))
        .expect("sweep history feature");
    assert!(matches!(
        &feature.definition,
        FeatureDefinition::Sweep {
            section: cadmpeg_ir::features::SweepSection::Profile(ProfileRef::Sketch(id)),
            ..
        } if id == &sketch.id
    ));
}

#[test]
fn decode_does_not_bind_ambiguous_enclosed_profile_streams_to_sweep() {
    use cadmpeg_ir::features::FeatureDefinition;

    let mut source = sldprt_with_nested_sketch_profiles(&triangle_body(), 2);
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sweep Name="Sketch1" Type="Sweep"/></Keywords>"#,
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
        .expect("sweep history feature");
    assert!(matches!(
        &feature.definition,
        FeatureDefinition::Sweep {
            section: cadmpeg_ir::features::SweepSection::Unresolved(_),
            ..
        }
    ));
}

#[test]
fn decode_binds_uniquely_enclosed_profile_stream_to_extrusion() {
    use cadmpeg_ir::features::{FeatureDefinition, ProfileRef};

    let mut source = sldprt_with_nested_sketch_profile(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Sketch1" Type="Boss-Extrude"><Dimension Name="D1">25</Dimension></Extrusion></Keywords>"#,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let [sketch] = decoded.ir().model.sketches.as_slice() else {
        panic!("one enclosed extrusion profile stream");
    };
    let feature = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Sketch1"))
        .expect("extrusion history feature");
    assert!(matches!(
        &feature.definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Sketch(id),
            ..
        } if id == &sketch.id
    ));
}

#[test]
fn decode_binds_configuration_sketch_state_after_geometry_projection() {
    use cadmpeg_ir::features::FeatureDefinition;

    let mut source = sldprt_with_nested_sketch_profile(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="Default" id="0"/><Sketch Name="Sketch1" Type="Sketch" id="0"/></Keywords>"#,
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
        &decoded.ir().model.configurations[0].feature_states[&feature.id].definition,
        FeatureDefinition::Sketch {
            sketch: Some(configuration_sketch),
            ..
        } if decoded.ir().model.sketches.iter().any(|sketch| &sketch.id == configuration_sketch)
    ));
}

#[test]
fn decode_does_not_bind_ambiguous_enclosed_profile_streams_to_extrusion() {
    use cadmpeg_ir::features::{FeatureDefinition, ProfileRef};

    let mut source = sldprt_with_nested_sketch_profiles(&triangle_body(), 2);
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Sketch1" Type="Boss-Extrude"><Dimension Name="D1">25</Dimension></Extrusion></Keywords>"#,
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
        .expect("extrusion history feature");
    assert!(matches!(
        &feature.definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Unresolved(_),
            ..
        }
    ));
}

#[test]
fn decode_binds_unique_sketch_history_to_profile_consumers() {
    use cadmpeg_ir::features::{FeatureDefinition, ProfileRef};

    let mut source = sldprt_with_nested_sketch_profile(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Profile" Type="Sketch" id="21"/><Rib Name="Web" Type="Rib" id="22" Profile="21" Direction="0,1,0" BothSides="false" Operation="Join"><Dimension Name="Thickness">2mm</Dimension></Rib></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let sketch_id = decoded.ir().model.sketches[0].id.clone();
    assert!(decoded.ir().model.features.iter().any(|feature| matches!(
        &feature.definition,
        FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Planar,
            sketch: Some(value), ..
        } if value == &sketch_id
    )));
    assert!(decoded.ir().model.features.iter().any(|feature| matches!(
        &feature.definition,
        FeatureDefinition::Rib {
            construction: cadmpeg_ir::features::RibConstruction {
                profile: Some(ProfileRef::Sketch(value)),
                ..
            },
            ..
        } if value == &sketch_id
    )));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new());
    assert!(validation.is_ok(), "{:?}", validation.findings);
    let mut written = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut written)
        .unwrap();
    let round_trip = SldprtCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert!(round_trip
        .ir()
        .model
        .features
        .iter()
        .any(|feature| matches!(
            feature.definition,
            FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: Some(_),
                ..
            }
        )));
}

#[test]
fn matching_numbered_sketch_alias_binds_the_base_geometry() {
    use std::collections::BTreeMap;

    use cadmpeg_ir::features::{
        BooleanOp, ExtrudeExtent, ExtrudeSide, FeatureDefinition, FeatureId, ProfileRef,
        Termination,
    };
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::sketches::{Sketch, SketchId};

    let sketch_id = SketchId("sketch".into());
    let sketch = Sketch {
        id: sketch_id.clone(),
        name: Some("Profile".into()),
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: vec![vec![cadmpeg_ir::sketches::SketchEntityUse {
            entity: cadmpeg_ir::sketches::SketchEntityId("sketch:entity".into()),
            reversed: false,
        }]],
        native_ref: None,
    };
    let neutral =
        |id: &str, name: &str, native_ref: &str, definition| cadmpeg_ir::features::Feature {
            id: FeatureId(id.into()),
            ordinal: 0,
            name: Some(name.into()),
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: Some("Sketch".into()),
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition,
            native_ref: Some(native_ref.into()),
        };
    let mut features = vec![
        neutral(
            "base",
            "Profile",
            "native-base",
            FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: None,
            },
        ),
        neutral(
            "alias",
            "Profile<3>",
            "native-alias",
            FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: None,
            },
        ),
        neutral(
            "different",
            "Profile<4>",
            "native-different",
            FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: None,
            },
        ),
        neutral(
            "consumer",
            "Boss",
            "native-consumer",
            FeatureDefinition::Extrude {
                profile: ProfileRef::Native("native-alias".into()),
                direction: cadmpeg_ir::features::ExtrudeDirection::ProfileNormal,
                start: cadmpeg_ir::features::ExtrudeStart::ProfilePlane,
                extent: ExtrudeExtent::OneSided {
                    side: ExtrudeSide {
                        termination: Termination::Unresolved,
                        draft: None,
                        offset: None,
                    },
                },
                op: BooleanOp::Join,
                direction_source: None,
                solid: None,
                face_maker: None,
                inner_wire_taper: None,
                length_along_profile_normal: None,
                allow_multi_profile_faces: None,
            },
        ),
    ];
    let native = |id: &str, name: &str, depth: &str| crate::records::Feature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: "Sketch".into(),
        tree_parent: None,
        source_id: None,
        parent_source_id: None,
        ordinal: 0,
        name: name.into(),
        kind: "Sketch".into(),
        input_class: Some("moProfileFeature_c".into()),
        suppressed: false,
        parameters: BTreeMap::from([("Depth".into(), depth.into())]),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: vec![crate::records::FeatureContent::Dimension("Depth".into())],
    };
    let history = crate::records::FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![
            native("native-base", "Profile", "2mm"),
            native("native-alias", "Profile<3>", "2mm"),
            native("native-different", "Profile<4>", "3mm"),
        ],
    };

    crate::history::bind_unique_sketch_feature(&mut features, &[sketch], &[history]);

    assert!(matches!(
        &features[1].definition,
        FeatureDefinition::Sketch { sketch: None, .. }
    ));
    assert_eq!(features[1].dependencies, vec![FeatureId("base".into())]);
    assert!(matches!(
        &features[2].definition,
        FeatureDefinition::Sketch { sketch: None, .. }
    ));
    assert!(matches!(
        &features[3].definition,
        FeatureDefinition::Extrude { profile: ProfileRef::Sketch(id), .. } if id == &sketch_id
    ));
    assert_eq!(features[3].dependencies, vec![FeatureId("base".into())]);
}

#[test]
fn decode_binds_multiple_sketch_history_nodes_by_exact_name() {
    use cadmpeg_ir::features::{FeatureDefinition, PathRef, ProfileRef};

    let mut source = sldprt_with_nested_nurbs_sketches(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="feature input spline sketch" Type="Sketch" id="21"/><Sketch Name="feature input rational spline sketch" Type="Sketch" id="22"/><Sweep Name="Pipe" Type="Sweep" id="23" Profile="21" Path="22" Operation="NewBody"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let bound = decoded
        .ir()
        .model
        .features
        .iter()
        .filter_map(|feature| match &feature.definition {
            FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: Some(sketch),
                ..
            } => Some(sketch.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(bound.len(), 2);
    let sweep = decoded
        .ir()
        .model
        .features
        .iter()
        .find_map(|feature| match &feature.definition {
            FeatureDefinition::Sweep {
                section: cadmpeg_ir::features::SweepSection::Profile(ProfileRef::Sketch(profile)),
                path: Some(PathRef::Sketch(path)),
                ..
            } => Some((profile, path)),
            _ => None,
        })
        .expect("bound sweep");
    assert_ne!(sweep.0, sweep.1);
    assert!(bound.contains(sweep.0) && bound.contains(sweep.1));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new());
    assert!(validation.is_ok(), "{:?}", validation.findings);
}

#[test]
fn decode_does_not_bind_duplicate_sketch_names_by_order() {
    use cadmpeg_ir::features::FeatureDefinition;

    let mut source = sldprt_with_body(&triangle_body());
    let mut payload = resolved_features_payload(&[1, 1]);
    for _ in 0..2 {
        payload.extend(parasolid_with_body(
            "Duplicate",
            "SCH_SW_33103_11000",
            &nurbs_sketch_body(false),
        ));
    }
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &payload,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Duplicate" Type="Sketch" id="21"/><Sketch Name="Duplicate" Type="Sketch" id="22"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.sketches.len(), 2);
    assert!(decoded.ir().model.features.iter().all(|feature| matches!(
        feature.definition,
        FeatureDefinition::Sketch { sketch: None, .. }
    )));
}

#[test]
fn decode_distinguishes_full_circle_sketch_geometry() {
    use cadmpeg_ir::features::Length;
    use cadmpeg_ir::sketches::SketchGeometry;

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_nested_circular_sketch(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(decoded.ir().model.sketches[0].profiles[0].len(), 1);
    assert!(matches!(
        decoded.ir().model.sketch_entities[0].geometry,
        SketchGeometry::Circle {
            center: cadmpeg_ir::math::Point2 { u: 0.0, v: 0.0 },
            radius: Length(1000.0),
        }
    ));
}

#[test]
fn decode_projects_full_ellipse_sketch_geometry() {
    use cadmpeg_ir::features::{Angle, Length};
    use cadmpeg_ir::sketches::SketchGeometry;

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_nested_elliptical_sketch(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(matches!(
        decoded.ir().model.sketch_entities[0].geometry,
        SketchGeometry::Ellipse {
            center: cadmpeg_ir::math::Point2 { u: 0.0, v: 0.0 },
            major_angle: Angle(value),
            major_radius: Length(2000.0),
            minor_radius: Length(1000.0),
            start_angle: None,
            end_angle: None,
        } if (value - std::f64::consts::FRAC_PI_2).abs() < 1e-12
    ));
}

#[test]
fn decode_projects_non_rational_and_rational_nurbs_sketch_geometry() {
    use cadmpeg_ir::sketches::SketchGeometry;

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_nested_nurbs_sketches(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let splines = decoded
        .ir()
        .model
        .sketch_entities
        .iter()
        .filter_map(|entity| match &entity.geometry {
            SketchGeometry::Nurbs {
                degree,
                knots,
                control_points,
                weights,
                periodic,
            } => Some((degree, knots, control_points, weights, periodic)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(splines.len(), 2);
    assert!(splines.iter().all(|(degree, knots, points, _, periodic)| {
        **degree == 2
            && knots.as_slice() == [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]
            && points.len() == 3
            && !**periodic
    }));
    assert!(splines
        .iter()
        .any(|(_, _, _, weights, _)| weights.is_none()));
    assert!(splines
        .iter()
        .any(|(_, _, _, weights, _)| { weights.as_deref() == Some(&[1.0, 0.5, 1.0]) }));
}

#[test]
fn face_on_untyped_surface_keeps_topology() {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let f = sldprt_with_body(&untyped_triangle(0.0));
    let mut cur = Cursor::new(f);
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir().model.faces.len(), 1);
    let SurfaceGeometry::Unknown {
        record: Some(record),
    } = &result.ir().model.surfaces[0].geometry
    else {
        panic!("opaque surface has no replay record");
    };
    let unknowns = result.ir().native_unknowns("sldprt").unwrap();
    let retained = unknowns
        .iter()
        .find(|unknown| unknown.id == *record)
        .expect("opaque surface record");
    assert!(retained.links.contains(&result.ir().model.surfaces[0].id.0));
    assert!(result
        .report()
        .losses
        .iter()
        .any(|l| l.code.taxonomy() == LossTaxonomy::GeometryNotTransferred));
    let report = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(report.is_ok(), "findings: {:?}", report.findings);
}

#[test]
fn strict_rejects_topology_decode_resting_on_untyped_surface() {
    use cadmpeg_ir::report::{LossTaxonomy, StrictConsequence};

    let mut body = Vec::new();
    body.extend(bridge(10, 20, 999));
    body.extend(loop_head(20, 30, 10));
    body.extend(coedge(30, 20, 31, 50, 0, 40, false));
    body.extend(coedge(31, 20, 32, 51, 0, 41, false));
    body.extend(coedge(32, 20, 30, 52, 0, 42, false));
    body.extend(edge_use(40, 0));
    body.extend(edge_use(41, 0));
    body.extend(edge_use(42, 0));
    body.extend(vertex_use(50, 60));
    body.extend(vertex_use(51, 61));
    body.extend(vertex_use(52, 62));
    body.extend(world_point(60, [0.0, 0.0, 0.0]));
    body.extend(world_point(61, [1.0, 0.0, 0.0]));
    body.extend(world_point(62, [0.0, 1.0, 0.0]));
    let fixture = sldprt_with_body(&body);

    let salvaged = SldprtCodec
        .decode(&mut Cursor::new(fixture.clone()), &DecodeOptions::default())
        .expect("salvage keeps the topology decode");
    assert_eq!(salvaged.ir().model.faces.len(), 1);
    let census = salvaged
        .report()
        .losses
        .iter()
        .find(|l| l.code.taxonomy() == LossTaxonomy::GeometryNotTransferred)
        .expect("untyped support surface raises a census note");
    assert_eq!(census.strict_consequence(), StrictConsequence::Reject);

    let error = SldprtCodec
        .decode(&mut Cursor::new(fixture), &strict_options())
        .expect_err("strict refuses the untyped-surface census");
    assert!(error.to_string().contains("strict mode rejects sldprt/"));
}

#[test]
fn compact_carrier_shapes_decode() {
    use crate::brep::{parse_carrier, CarrierGeometry};
    use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};

    // Cylinder (tag 00 33, 10 f64): origin, axis, radius, refdir.
    let mut cyl = vec![0x00, 0x33];
    be16(&mut cyl, 5);
    be32(&mut cyl, 0);
    for _ in 0..5 {
        be16(&mut cyl, 0);
    }
    cyl.push(0x2b);
    for v in [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.05, 1.0, 0.0, 0.0] {
        bef64(&mut cyl, v);
    }
    match parse_carrier(&cyl, 0).unwrap().geometry {
        CarrierGeometry::Surface(SurfaceGeometry::Cylinder { radius, axis, .. }) => {
            assert_eq!(radius, 50.0); // 0.05 m ×1000
            assert_eq!(axis.z, 1.0);
        }
        other => panic!("expected cylinder, got {other:?}"),
    }

    // Circle (tag 00 1f, 10 f64): radius is the tenth value.
    let mut circ = vec![0x00, 0x1f];
    be16(&mut circ, 6);
    be32(&mut circ, 0);
    for _ in 0..5 {
        be16(&mut circ, 0);
    }
    circ.push(0x2d);
    for v in [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.003] {
        bef64(&mut circ, v);
    }
    match parse_carrier(&circ, 0).unwrap().geometry {
        CarrierGeometry::Curve(CurveGeometry::Circle { radius, .. }) => assert_eq!(radius, 3.0),
        other => panic!("expected circle, got {other:?}"),
    }

    // A bad marker (not 2b/2d) rejects the candidate.
    let mut bad = cyl.clone();
    bad[2 + 2 + 4 + 10] = 0x00;
    assert!(parse_carrier(&bad, 0).is_none());
}

#[test]
fn compact_carriers_reject_zero_direction_frames() {
    use crate::brep::parse_carrier;

    let line = line_carrier(5, [0.0, 0.0, 0.0], [0.0, 0.0, 0.0]);
    assert!(parse_carrier(&line, 0).is_none());

    let cylinder = cylinder_carrier(6, [0.0, 0.0, 0.0], [0.0, 0.0, 0.0], 1.0);
    assert!(parse_carrier(&cylinder, 0).is_none());
}

/// Metamorphic property: a rigid translation of the input produces the same
/// rigid translation of the decoded output (equivariance), and topology is
/// invariant. Reader and writer cannot silently drop or reorient geometry
/// without breaking one of these relations.
#[test]
fn decode_is_equivariant_under_rigid_translation() {
    let base = source_less_cube();
    let t = [3.5, -7.25, 11.0];

    let mut moved = base.clone();
    translate_model(&mut moved, t);

    let base_out = encode_decode(&base);
    let moved_out = encode_decode(&moved);

    // Topology is invariant under a rigid motion.
    assert_eq!(base_out.model.faces.len(), moved_out.model.faces.len());
    assert_eq!(base_out.model.edges.len(), moved_out.model.edges.len());
    assert_eq!(
        base_out.model.vertices.len(),
        moved_out.model.vertices.len()
    );

    // Point positions of the moved decode equal the base decode shifted by t.
    let base_positions = sorted_point_positions(&base_out);
    let moved_positions = sorted_point_positions(&moved_out);
    assert_eq!(base_positions.len(), moved_positions.len());
    for (b, m) in base_positions.iter().zip(&moved_positions) {
        for axis in 0..3 {
            assert!(
                (b[axis] + t[axis] - m[axis]).abs() < 1e-6,
                "axis {axis}: {b:?} + {t:?} != {m:?}"
            );
        }
    }
}

/// Decode → encode → decode fixpoint: once through the writer, a source-less
/// model reaches a fixed point whose semantic hash and topology no longer
/// change. Paired with the value golden below so a shared reader/writer
/// misconception cannot hide behind a self-consistent round trip.
#[test]
fn source_less_cube_reaches_encode_decode_fixpoint() {
    let first = encode_decode_result(&source_less_cube());
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(first.ir(), first.source_fidelity(), &mut encoded)
        .unwrap();
    let second = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap()
        .into_parts()
        .0;

    let first_hash = crate::decode::document_local_sha256(first.ir());
    let second_hash = crate::decode::document_local_sha256(&second);
    assert_eq!(first_hash, second_hash, "round trip is not a fixed point");

    // Value golden: the cube's record families and counts, asserted directly.
    assert_eq!(first.ir().model.bodies.len(), 1);
    assert_eq!(first.ir().model.faces.len(), 6);
    assert_eq!(first.ir().model.edges.len(), 12);
    assert_eq!(first.ir().model.vertices.len(), 8);
    assert_eq!(first.ir().model.coedges.len(), 24);
    assert_eq!(first.ir().model.loops.len(), 6);
    assert_eq!(
        sorted_point_positions(first.ir()),
        sorted_point_positions(&second)
    );
}

#[path = "integration_tests.rs"]
mod integration_tests;
