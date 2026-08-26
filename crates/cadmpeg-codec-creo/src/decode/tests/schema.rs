// SPDX-License-Identifier: Apache-2.0
//! Synthetic byte-literal tests for the container framing and honest decode.
//!
//! No external CAD file is used; every fixture is a hand-built PSB byte image
//! exercising the `#UGC:2` framing, the `#\n#<name>\n` section-boundary rule, the
//! persistence-layout signals, and the `srf_array`/`crv_array` count headers.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::container::{self};
use crate::test_support::*;
use crate::CreoCodec;

#[test]
fn decode_types_class_911_as_unresolved_hole() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x24, 4, 0x01, 0, 0]);
    let allfeatur = vec![
        4, 0xeb, 0x04, 0, 0x10, 1, 0x80, 0x80, 0, 0xe4, 0xe3, 0xf6, 0x83, 0x8f, 0xe1,
    ];
    let data = build_prt(
        "c",
        &[
            ("VisibGeom", geometry),
            ("AllFeatur", allfeatur),
            ("MdlStatus", b"Hole id 4\0".to_vec()),
        ],
    );

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let feature = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#4")
        .expect("hole feature");
    assert!(matches!(
        feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Hole {
            face: None,
            position: None,
            direction: None,
            kind: cadmpeg_ir::features::HoleKind::Unresolved { form: None, .. },
            diameter: None,
            extent: None,
            ..
        }
    ));
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_FEATURE_COUNT),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_TYPED_FEATURE_COUNT),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_NATIVE_FEATURE_COUNT),
        0
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_HOLE_FEATURE_COUNT),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_INCOMPLETE_HOLE_FEATURE_COUNT),
        1
    );
    for key in [
        "transferred_unresolved_hole_location_feature_count",
        "transferred_unresolved_hole_direction_feature_count",
        "transferred_unresolved_hole_kind_feature_count",
        "transferred_unresolved_hole_diameter_feature_count",
        "transferred_incomplete_hole_termination_feature_count",
    ] {
        assert_eq!(
            result.report().coverage.get(key).copied().unwrap_or(0),
            1,
            "{key}"
        );
    }
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_UNRESOLVED_HOLE_PROFILE_FEATURE_COUNT),
        0
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_NATIVE_HOLE_PROFILE_FEATURE_COUNT),
        0
    );
    assert_eq!(
        result.report().coverage_count(
            crate::coverage::TRANSFERRED_UNRESOLVED_HOLE_FACE_SELECTION_FEATURE_COUNT
        ),
        0
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_NATIVE_HOLE_FACE_SELECTION_FEATURE_COUNT),
        0
    );
}

#[test]
fn decode_types_class_914_as_unresolved_chamfer() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let allfeatur = vec![
        4, 0xeb, 0x04, 0, 0x10, 1, 0x80, 0x80, 0, 0xe4, 0xe3, 0xf6, 0x83, 0x92, 0xe1,
    ];
    let data = build_prt(
        "c",
        &[
            ("VisibGeom", geometry),
            ("AllFeatur", allfeatur),
            ("MdlStatus", b"Chamfer id 4\0".to_vec()),
        ],
    );

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let feature = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#4")
        .expect("chamfer feature");
    assert!(matches!(
        feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Chamfer {
            ref groups,
            ..
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::ChamferGroup {
            edges: cadmpeg_ir::features::EdgeSelection::Unresolved,
            spec: cadmpeg_ir::features::ChamferSpec::Unresolved { form: None },
        }])
    ));
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_CHAMFER_FEATURE_COUNT),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_INCOMPLETE_CHAMFER_FEATURE_COUNT),
        1
    );
    assert_eq!(
        result.report().coverage_count(
            crate::coverage::TRANSFERRED_UNRESOLVED_CHAMFER_EDGE_SELECTION_FEATURE_COUNT
        ),
        1
    );
    assert_eq!(
        result.report().coverage_count(
            crate::coverage::TRANSFERRED_NATIVE_CHAMFER_EDGE_SELECTION_FEATURE_COUNT
        ),
        0
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_UNRESOLVED_CHAMFER_SPEC_FEATURE_COUNT),
        1
    );
}

#[test]
fn decode_uses_stored_family_when_row_schema_is_not_registered() {
    let allfeatur = vec![
        4, 0xeb, 0x04, 0, 0x10, 1, 0x80, 0x80, 0, 0xe4, 0xe3, 0xf6, 0x84, 0x50, 0xe1,
    ];
    let data = build_prt(
        "c",
        &[
            ("AllFeatur", allfeatur),
            ("MdlStatus", b"Round id 4\0".to_vec()),
        ],
    );

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let feature = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#4")
        .expect("round feature");

    assert!(matches!(
        feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Fillet { .. }
    ));
    assert_eq!(feature.source_properties["featdefs_schema_class"], "1104");
}

#[test]
fn decode_uses_reference_name_family_when_operation_name_is_generic() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let allfeatur = vec![
        4, 0xeb, 0x04, 0, 0x10, 1, 0x80, 0x80, 0, 0xe4, 0xe3, 0xf6, 0x83, 0xae, 0xe1,
    ];
    let reference_name = b"\xf7\x71\x09\x01\x04Extrude 7\0\x09\x09".to_vec();
    let data = build_prt(
        "c",
        &[
            ("VisibGeom", geometry),
            ("AllFeatur", allfeatur),
            ("MdlRefInfo", reference_name),
            ("MdlStatus", b"Surface id 4\0".to_vec()),
        ],
    );

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let feature = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#4")
        .expect("surface feature");

    assert_eq!(feature.name.as_deref(), Some("Surface id 4"));
    assert!(
        matches!(
            feature.definition,
            cadmpeg_ir::features::FeatureDefinition::Extrude {
                op: cadmpeg_ir::features::BooleanOp::NewBody,
                solid: Some(false),
                ..
            }
        ),
        "{:#?}\n{:#?}",
        feature.definition,
        feature.source_properties
    );
}

#[test]
fn decode_types_default_part_coordinate_system() {
    let allfeatur = vec![
        7, 0xeb, 0x04, 0, 0x10, 1, 0x80, 0x80, 0, 0xe4, 0xe3, 0xf6, 0x83, 0xd3, 0xe1,
    ];
    let reference_name = b"\xf7\x71\x09\x01\x07PRT_CSYS_DEF\0\x09\x09".to_vec();
    let data = build_prt(
        "c",
        &[("AllFeatur", allfeatur), ("MdlRefInfo", reference_name)],
    );

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let feature = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#7")
        .expect("coordinate-system feature");

    assert_eq!(feature.name.as_deref(), Some("PRT_CSYS_DEF"));
    assert!(matches!(
        feature.definition,
        cadmpeg_ir::features::FeatureDefinition::DatumCoordinateSystemUnresolved
    ));
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_EXPLICITLY_UNRESOLVED_FEATURE_COUNT),
        1
    );
    assert_eq!(
        result.report().coverage_count(
            crate::coverage::TRANSFERRED_UNRESOLVED_DATUM_COORDINATE_SYSTEM_FEATURE_COUNT
        ),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_UNRESOLVED_DATUM_PLANE_FEATURE_COUNT),
        0
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_UNRESOLVED_BOUNDARY_SURFACE_FEATURE_COUNT),
        0
    );
    assert!(result.report().losses.iter().any(|loss| {
        loss.message
            .contains("1 typed history feature definition(s)")
    }));
}

#[test]
fn decode_types_class_946_as_unresolved_surface_merge() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let allfeatur = vec![
        4, 0xeb, 0x04, 0, 0x10, 1, 0x80, 0x80, 0, 0xe4, 0xe3, 0xf6, 0x83, 0xb2, 0xe1,
    ];
    let data = build_prt("c", &[("VisibGeom", geometry), ("AllFeatur", allfeatur)]);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let feature = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#4")
        .expect("surface merge feature");
    assert!(matches!(
        feature.definition,
        cadmpeg_ir::features::FeatureDefinition::KnitSurface {
            faces: cadmpeg_ir::features::FaceSelection::Unresolved,
            merge_entities: Some(true),
            create_solid: Some(false),
            gap_tolerance: None,
        }
    ));
    assert_eq!(feature.name.as_deref(), Some("Surface Merge id 4"));
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_KNIT_SURFACE_FEATURE_COUNT),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_INCOMPLETE_KNIT_SURFACE_FEATURE_COUNT),
        1
    );
    assert_eq!(
        result.report().coverage_count(
            crate::coverage::TRANSFERRED_UNRESOLVED_KNIT_SURFACE_FACES_FEATURE_COUNT
        ),
        1
    );
    assert_eq!(
        result.report().coverage_count(
            crate::coverage::TRANSFERRED_UNRESOLVED_KNIT_SURFACE_MERGE_FEATURE_COUNT
        ),
        0
    );
    assert_eq!(
        result.report().coverage_count(
            crate::coverage::TRANSFERRED_UNRESOLVED_KNIT_SURFACE_SOLID_FEATURE_COUNT
        ),
        0
    );
    assert_eq!(
        result.report().coverage_count(
            crate::coverage::TRANSFERRED_INCOMPLETE_SURFACE_OPERATION_FEATURE_COUNT
        ),
        1
    );
    assert!(result.report().losses.iter().any(|loss| loss
        .message
        .contains("surface construction history feature")));
}

#[test]
fn decode_types_row_only_class_927_as_unresolved_draft() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let allfeatur = vec![
        4, 0xeb, 0x04, 0, 0x10, 1, 0x80, 0x80, 0, 0xe4, 0xe3, 0xf6, 0x83, 0x9f, 0xe1,
    ];
    let data = build_prt("c", &[("VisibGeom", geometry), ("AllFeatur", allfeatur)]);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let feature = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#4")
        .expect("draft feature");
    assert_eq!(feature.name.as_deref(), Some("Draft id 4"));
    assert!(matches!(
        feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Draft {
            faces: cadmpeg_ir::features::FaceSelection::Unresolved,
            neutral_plane: cadmpeg_ir::features::FaceSelection::Unresolved,
            parting_tool: None,
            pull_direction: None,
            pull_plane: None,
            angle: None,
            outward: None,
        }
    ));
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_DRAFT_FEATURE_COUNT),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_INCOMPLETE_DRAFT_FEATURE_COUNT),
        1
    );
    for key in [
        "transferred_unresolved_draft_face_selection_feature_count",
        "transferred_unresolved_draft_neutral_plane_feature_count",
        "transferred_unresolved_draft_direction_feature_count",
        "transferred_unresolved_draft_angle_feature_count",
        "transferred_unresolved_draft_outward_feature_count",
    ] {
        assert_eq!(
            result.report().coverage.get(key).copied().unwrap_or(0),
            1,
            "{key}"
        );
    }
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_NATIVE_DRAFT_FACE_SELECTION_FEATURE_COUNT),
        0
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_NATIVE_DRAFT_NEUTRAL_PLANE_FEATURE_COUNT),
        0
    );
}

#[test]
fn decode_types_named_draft_with_unresolved_operands() {
    for name in ["Draft", "Schräge"] {
        let stored_name = format!("{name} id 40\0");
        let data = build_prt("c", &[("MdlStatus", stored_name.into_bytes())]);
        let result = CreoCodec
            .decode(&mut Cursor::new(data), &DecodeOptions::default())
            .expect("decode");
        let feature = result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.id.as_str() == "creo:model:feature#40")
            .expect("draft feature");

        assert!(matches!(
            &feature.definition,
            cadmpeg_ir::features::FeatureDefinition::Draft {
                faces: cadmpeg_ir::features::FaceSelection::Unresolved,
                neutral_plane: cadmpeg_ir::features::FaceSelection::Unresolved,
                parting_tool: None,
                pull_direction: None,
                pull_plane: None,
                angle: None,
                outward: None,
            }
        ));
    }
}

#[test]
fn decode_types_named_mirror_with_unresolved_operands() {
    let data = build_prt("c", &[("MdlStatus", b"oMirror id 4\0".to_vec())]);
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let feature = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#4")
        .expect("mirror feature");

    assert_eq!(
        feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Pattern {
            seeds: Vec::new(),
            pattern: cadmpeg_ir::features::PatternKind::Unresolved {
                form: Some(cadmpeg_ir::features::PatternForm::Mirror),
            },
        }
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_PATTERN_FEATURE_COUNT),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_INCOMPLETE_PATTERN_FEATURE_COUNT),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_UNRESOLVED_PATTERN_SEED_FEATURE_COUNT),
        1
    );
    assert_eq!(
        result.report().coverage_count(
            crate::coverage::TRANSFERRED_UNRESOLVED_PATTERN_TRANSFORM_FEATURE_COUNT
        ),
        1
    );
    assert_eq!(
        feature
            .source_properties
            .get("mdl_stored_name_prefix")
            .map(String::as_str),
        Some("o")
    );
}

#[test]
fn decode_types_z_prefixed_round_with_unresolved_operands() {
    let data = build_prt("c", &[("MdlStatus", b"zRound id 4\0".to_vec())]);
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let feature = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#4")
        .expect("round feature");

    assert_eq!(
        feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Fillet {
            groups: vec![cadmpeg_ir::features::FilletGroup {
                edges: cadmpeg_ir::features::EdgeSelection::Unresolved,
                radius: cadmpeg_ir::features::RadiusSpec::Unresolved { form: None },
                tangency_weight: None,
            }],
        }
    );
    assert_eq!(
        feature
            .source_properties
            .get("mdl_stored_name_prefix")
            .map(String::as_str),
        Some("z")
    );
}

#[test]
fn decode_recovers_schema_feature_that_owns_materialized_surfaces() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let allfeatur = vec![
        4, 0xeb, 0x04, 0, 0x10, 1, 0x80, 0x80, 0, 0xe4, 0xe3, 0xf6, 0x83, 0x95, 0xe1, 0xe3, 9,
        0xeb, 0x04, 0, 0x10, 1, 0, 0xe5, 0xe3, 0xf6, 0x83, 0x91, 0xe1,
    ];
    let data = build_prt("c", &[("VisibGeom", geometry), ("AllFeatur", allfeatur)]);
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");

    assert_eq!(result.ir().model.features.len(), 1);
    let feature = &result.ir().model.features[0];
    assert_eq!(feature.id.as_str(), "creo:model:feature#4");
    assert_eq!(feature.name.as_deref(), Some("Protrusion id 4"));
    assert!(matches!(
        &feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Extrude {
            profile: cadmpeg_ir::features::ProfileRef::Unresolved(_),
            direction: cadmpeg_ir::features::ExtrudeDirection::ProfileNormal,
            extent: cadmpeg_ir::features::ExtrudeExtent::OneSided {
                side: cadmpeg_ir::features::ExtrudeSide {
                    termination: cadmpeg_ir::features::Termination::Unresolved,
                    ..
                }
            },
            ..
        }
    ));
    assert_eq!(
        feature
            .source_properties
            .get("featdefs_schema_class")
            .map(String::as_str),
        Some("917")
    );
    assert!(result
        .ir()
        .model
        .features
        .iter()
        .all(|feature| feature.id.as_str() != "creo:model:feature#9"));
}

#[test]
fn decode_types_row_only_class_916_as_subtractive_extrusion() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let allfeatur = vec![
        4, 0xeb, 0x04, 0, 0x10, 1, 0x80, 0x80, 0, 0xe4, 0xe3, 0xf6, 0x83, 0x94, 0xe1,
    ];
    let data = build_prt("c", &[("VisibGeom", geometry), ("AllFeatur", allfeatur)]);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let feature = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#4")
        .expect("cut feature");

    assert_eq!(feature.name.as_deref(), Some("Cut id 4"));
    assert!(matches!(
        feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Extrude {
            profile: cadmpeg_ir::features::ProfileRef::Unresolved(_),
            direction: cadmpeg_ir::features::ExtrudeDirection::ProfileNormal,
            extent: cadmpeg_ir::features::ExtrudeExtent::OneSided {
                side: cadmpeg_ir::features::ExtrudeSide {
                    termination: cadmpeg_ir::features::Termination::Unresolved,
                    ..
                }
            },
            op: cadmpeg_ir::features::BooleanOp::Cut,
            ..
        }
    ));
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_EXTRUDE_FEATURE_COUNT),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_UNRESOLVED_EXTRUDE_PROFILE_FEATURE_COUNT),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_NATIVE_EXTRUDE_PROFILE_FEATURE_COUNT),
        0
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_INCOMPLETE_EXTRUDE_START_FEATURE_COUNT),
        0
    );
    assert_eq!(
        result.report().coverage_count(
            crate::coverage::TRANSFERRED_INCOMPLETE_EXTRUDE_TERMINATION_FEATURE_COUNT
        ),
        1
    );
    assert_eq!(
        result.report().coverage_count(
            crate::coverage::TRANSFERRED_UNRESOLVED_EXTRUDE_BOOLEAN_OPERATION_FEATURE_COUNT
        ),
        0
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_INCOMPLETE_EXTRUDE_FEATURE_COUNT),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_INCOMPLETE_SWEEP_FEATURE_COUNT),
        1
    );
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("profile sweep history feature")));
}

#[test]
fn decode_types_named_base_protrusion_as_new_body() {
    let data = build_prt("c", &[("MdlStatus", b"Protrusion id 4\0".to_vec())]);
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let feature = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#4")
        .expect("protrusion feature");

    assert!(matches!(
        &feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Extrude {
            profile: cadmpeg_ir::features::ProfileRef::Unresolved(_),
            direction: cadmpeg_ir::features::ExtrudeDirection::ProfileNormal,
            extent: cadmpeg_ir::features::ExtrudeExtent::OneSided {
                side: cadmpeg_ir::features::ExtrudeSide {
                    termination: cadmpeg_ir::features::Termination::Unresolved,
                    ..
                }
            },
            op: cadmpeg_ir::features::BooleanOp::NewBody,
            ..
        }
    ));
}

#[test]
fn decode_types_named_sweeps_without_recipe_or_operands() {
    let data = build_prt(
        "c",
        &[(
            "MdlStatus",
            b"Extrude id 4\0Revolve id 5\0Cut id 6\0".to_vec(),
        )],
    );
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let feature = |id| {
        result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.id.as_str() == id)
            .expect("named sweep feature")
    };

    assert!(matches!(
        feature("creo:model:feature#4").definition,
        cadmpeg_ir::features::FeatureDefinition::Extrude {
            profile: cadmpeg_ir::features::ProfileRef::Unresolved(_),
            direction: cadmpeg_ir::features::ExtrudeDirection::ProfileNormal,
            extent: cadmpeg_ir::features::ExtrudeExtent::OneSided {
                side: cadmpeg_ir::features::ExtrudeSide {
                    termination: cadmpeg_ir::features::Termination::Unresolved,
                    ..
                }
            },
            op: cadmpeg_ir::features::BooleanOp::Unresolved,
            ..
        }
    ));
    assert!(matches!(
        feature("creo:model:feature#5").definition,
        cadmpeg_ir::features::FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction {
                profile: None,
                axis: None,
                extent: None,
                ..
            },
            op: cadmpeg_ir::features::BooleanOp::Unresolved,
        }
    ));
    assert!(matches!(
        feature("creo:model:feature#6").definition,
        cadmpeg_ir::features::FeatureDefinition::Extrude {
            profile: cadmpeg_ir::features::ProfileRef::Unresolved(_),
            direction: cadmpeg_ir::features::ExtrudeDirection::ProfileNormal,
            extent: cadmpeg_ir::features::ExtrudeExtent::OneSided {
                side: cadmpeg_ir::features::ExtrudeSide {
                    termination: cadmpeg_ir::features::Termination::Unresolved,
                    ..
                }
            },
            op: cadmpeg_ir::features::BooleanOp::Cut,
            ..
        }
    ));
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_REVOLVE_FEATURE_COUNT),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_UNRESOLVED_REVOLVE_PROFILE_FEATURE_COUNT),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_NATIVE_REVOLVE_PROFILE_FEATURE_COUNT),
        0
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_UNRESOLVED_REVOLVE_AXIS_FEATURE_COUNT),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_INCOMPLETE_REVOLVE_EXTENT_FEATURE_COUNT),
        1
    );
    assert_eq!(
        result.report().coverage_count(
            crate::coverage::TRANSFERRED_UNRESOLVED_REVOLVE_BOOLEAN_OPERATION_FEATURE_COUNT
        ),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_INCOMPLETE_REVOLVE_FEATURE_COUNT),
        1
    );
}

#[test]
fn decode_types_schema_datum_from_its_unique_plane_carrier() {
    let mut geometry = visibgeom_payload(1, 0);
    push_generated_plane_row(
        &mut geometry,
        7,
        false,
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
    );
    let allfeatur = vec![
        4, 0xeb, 0x04, 0, 0x10, 1, 0x80, 0x80, 0, 0xe4, 0xe3, 0xf6, 0x83, 0x9b, 0xe1,
    ];
    let data = build_prt("c", &[("VisibGeom", geometry), ("AllFeatur", allfeatur)]);
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");

    assert_eq!(result.ir().model.features.len(), 1);
    assert!(matches!(
        &result.ir().model.features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::DatumPlane { origin, normal, u_axis }
            if *origin == cadmpeg_ir::math::Point3::new(0.0, 0.0, 1.0)
                && *normal == cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)
                && *u_axis == cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0)
    ));
}

#[test]
fn decode_types_class_913_without_an_edge_array() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let allfeatur = vec![
        4, 0xeb, 0x04, 0, 0x10, 1, 0x80, 0x80, 0, 0xe4, 0xe3, 0xf6, 0x83, 0x91, 0xe1,
    ];
    let data = build_prt(
        "c",
        &[
            ("VisibGeom", geometry),
            ("AllFeatur", allfeatur),
            ("MdlStatus", b"Round id 4\0".to_vec()),
        ],
    );
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");

    assert!(matches!(
        result.ir().model.features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::Fillet {
            ref groups,
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            edges: cadmpeg_ir::features::EdgeSelection::Unresolved,
            radius: cadmpeg_ir::features::RadiusSpec::Unresolved { .. },
            ..
        }])
    ));
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_FILLET_FEATURE_COUNT),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_INCOMPLETE_FILLET_FEATURE_COUNT),
        1
    );
    assert_eq!(
        result.report().coverage_count(
            crate::coverage::TRANSFERRED_UNRESOLVED_FILLET_EDGE_SELECTION_FEATURE_COUNT
        ),
        1
    );
    assert_eq!(
        result.report().coverage_count(
            crate::coverage::TRANSFERRED_NATIVE_FILLET_EDGE_SELECTION_FEATURE_COUNT
        ),
        0
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_UNRESOLVED_FILLET_RADIUS_FEATURE_COUNT),
        1
    );
    assert_eq!(
        result.report().coverage_count(crate::coverage::TRANSFERRED_UNRESOLVED_FILLET_RADIUS_WITHOUT_GENERATED_SURFACE_FEATURE_COUNT),
        0
    );
    assert_eq!(
        result.report().coverage_count(crate::coverage::TRANSFERRED_UNRESOLVED_FILLET_RADIUS_WITH_GENERATED_SURFACE_FEATURE_COUNT),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::UNTRANSFERRED_VISIBLE_PLANE_SURFACE_ROW_COUNT),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::UNTRANSFERRED_VISIBLE_CYLINDER_SURFACE_ROW_COUNT),
        0
    );
    assert!(result.report().losses.iter().any(|loss| {
        loss.message.contains(
            "1 unique VisibGeom surface row(s) were not transferred as carriers and remain \
             structural namespace records (plane=1).",
        )
    }));
}

#[test]
fn decode_types_named_german_round_without_a_schema_row() {
    let data = build_prt("c", &[("MdlStatus", b"Rundung id 4\0".to_vec())]);
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");

    assert_eq!(
        result.ir().model.features[0].name.as_deref(),
        Some("Rundung id 4")
    );
    assert!(matches!(
        result.ir().model.features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::Fillet {
            ref groups,
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            edges: cadmpeg_ir::features::EdgeSelection::Unresolved,
            radius: cadmpeg_ir::features::RadiusSpec::Unresolved { .. }, ..
        }])
    ));
    assert_eq!(
        result.report().coverage_count(crate::coverage::TRANSFERRED_UNRESOLVED_FILLET_RADIUS_WITHOUT_GENERATED_SURFACE_FEATURE_COUNT),
        1
    );
    assert_eq!(
        result.report().coverage_count(crate::coverage::TRANSFERRED_UNRESOLVED_FILLET_RADIUS_WITH_GENERATED_SURFACE_FEATURE_COUNT),
        0
    );
}

#[test]
fn decode_types_named_annotation_feature_as_a_tree_node() {
    let data = build_prt("c", &[("MdlStatus", b"Annotation Feature id 4\0".to_vec())]);
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");

    assert!(matches!(
        result.ir().model.features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::TreeNode {
            role: cadmpeg_ir::features::FeatureTreeNodeRole::Annotations,
            ..
        }
    ));
}

#[test]
fn decode_types_localized_cross_section_nodes() {
    let data = build_prt(
        "c",
        &[(
            "MdlStatus",
            b"Cross Section id 4\0Querschnitt id 5\0".to_vec(),
        )],
    );
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");

    assert_eq!(result.ir().model.features.len(), 2);
    assert!(result.ir().model.features.iter().all(|feature| matches!(
        feature.definition,
        cadmpeg_ir::features::FeatureDefinition::TreeNode {
            role: cadmpeg_ir::features::FeatureTreeNodeRole::CrossSections,
            ..
        }
    )));
}

#[test]
fn decode_types_body_and_surface_tree_nodes() {
    let data = build_prt(
        "c",
        &[(
            "MdlStatus",
            b"Body id 4\0K\xc3\xb6rper ID 5\0Surface id 6\0".to_vec(),
        )],
    );
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");

    assert_eq!(result.ir().model.features.len(), 3);
    assert!(matches!(
        result.ir().model.features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::TreeNode {
            role: cadmpeg_ir::features::FeatureTreeNodeRole::SolidBodies,
            ..
        }
    ));
    assert!(matches!(
        result.ir().model.features[1].definition,
        cadmpeg_ir::features::FeatureDefinition::TreeNode {
            role: cadmpeg_ir::features::FeatureTreeNodeRole::SolidBodies,
            ..
        }
    ));
    assert!(matches!(
        result.ir().model.features[2].definition,
        cadmpeg_ir::features::FeatureDefinition::TreeNode {
            role: cadmpeg_ir::features::FeatureTreeNodeRole::SurfaceBodies,
            ..
        }
    ));
}

#[test]
fn decode_types_round_with_labeled_edge_selection() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let allfeatur = b"\x04\xeb\x04\x00\x10\x01\x00\xe5\xe3\xf6\x83\x91\xe1\
        \xe0\x21edgs_affected\0\xf8\x02\x2c\x2d"
        .to_vec();
    let data = build_prt(
        "c",
        &[
            ("VisibGeom", geometry),
            ("AllFeatur", allfeatur),
            ("MdlStatus", b"Round id 4\0".to_vec()),
        ],
    );
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let feature = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#4")
        .expect("round feature");

    assert_eq!(
        feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Fillet {
            groups: vec![cadmpeg_ir::features::FilletGroup {
                edges: cadmpeg_ir::features::EdgeSelection::Native(
                    "creo:allfeatur:edgs_affected#4:44,45".to_string()
                ),
                radius: cadmpeg_ir::features::RadiusSpec::Unresolved { form: None },
                tangency_weight: None,
            }],
        }
    );
    assert_eq!(
        feature
            .source_properties
            .get("native_parameter.affected_edge_ids")
            .map(String::as_str),
        Some("44,45")
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn decode_types_full_turn_revolution_from_positional_angle_choice() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 40, 0x01, 0, 0]);
    let allfeatur = vec![
        40, 0xeb, 0x04, 0xe3, 0xf6, 0x83, 0x95, 0xe1, 0x02, 0x83, 0xdf, 0xf6, 0xe3, 0x00, 0x00,
        0xea, 0x44, 0x00, 0x00, 0xf6, 0xf6, 0xf6, 0x00, 0x00, 0x00, 0x00,
    ];
    let mdlstatus = b"\xe3icon\0protrevolve\0Revolve id 40\0".to_vec();
    let data = build_prt(
        "c",
        &[
            ("VisibGeom", geometry),
            ("AllFeatur", allfeatur),
            ("MdlStatus", mdlstatus),
        ],
    );
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.features.revolution_extents.len(), 1);
    assert_eq!(scan.features.revolution_extents[0].feature_id, 40);
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let feature = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.id.0 == "creo:model:feature#40")
        .expect("revolution feature");
    assert!(matches!(
        &feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction {
                profile: None,
                axis: None,
                extent: Some(cadmpeg_ir::features::RevolveExtent::OneSided {
                    termination: cadmpeg_ir::features::Termination::Angle {
                        angle: cadmpeg_ir::features::Angle(angle)
                    }
                }),
                ..
            },
            op: cadmpeg_ir::features::BooleanOp::NewBody,
        } if (*angle - std::f64::consts::TAU).abs() < 1e-12
    ));
    let records =
        &result.ir().native.namespace("creo").unwrap().arenas["feature_revolution_extents"];
    assert_eq!(records[0].fields()["kind"], "full_turn");
}

#[test]
fn decode_types_schema_less_datum_plane_names() {
    for name in ["Datum Plane", "Bezugsebene"] {
        let mut payload = b"srf_array\0geom_id\0\x05geom_type\0\x22feat_id\0\x04orient\0\x01boundary_type\0\x01next_geom_ptr\0\0\
            outline\0\xf9\x02\x03"
            .to_vec();
        payload.extend_from_slice(&[0xe4, 0x0f, 0x2f, 0, 0, 0x0d, 0x0f, 0x48, 0, 0]);
        payload.extend_from_slice(b"\xe0\x00srf_prim_ptr(plane)\0\xe3");
        let stored_name = format!("{name} id 4\0");
        let data = build_prt(
            "c",
            &[
                ("VisibGeom", payload),
                ("MdlStatus", stored_name.as_bytes().to_vec()),
            ],
        );

        let result = CreoCodec
            .decode(&mut Cursor::new(data), &DecodeOptions::default())
            .expect("decode");
        let feature = result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.id.as_str() == "creo:model:feature#4")
            .expect("named datum feature");
        assert_eq!(
            feature.name.as_deref(),
            Some(format!("{name} id 4").as_str())
        );
        assert!(matches!(
            feature.definition,
            cadmpeg_ir::features::FeatureDefinition::DatumPlane { .. }
        ));
    }
}
