// SPDX-License-Identifier: Apache-2.0
//! Codec-wide inspect and decode contracts over synthetic packages.

use std::io::Cursor;

use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::{DecodeDepth, InspectOptions};
use cadmpeg_core::dialect::Admission;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_ir::ContainerSummary;

use crate::loss::PvzLossCode;
use crate::test_support::{
    assembly, assembly_structure, bracket_member, cube_and_sheet_assembly, exact_assembly, package,
    plate_member, OL_08, OL_09, PVS_0301, PVS_0304,
};
use crate::PvzCodec;

fn inspect(bytes: &[u8]) -> ContainerSummary {
    PvzCodec
        .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
        .unwrap()
}

fn decode_at(bytes: &[u8], depth: DecodeDepth) -> Result<DecodeResult, CodecError> {
    let options = DecodeOptions {
        depth,
        ..DecodeOptions::default()
    };
    PvzCodec
        .decode(&mut Cursor::new(bytes), &options)
        .map_err(|failure| match failure {
            cadmpeg_ir::DecodeFailure::Codec(error) => error,
            other => panic!("unexpected decode failure: {other}"),
        })
}

fn decode(bytes: &[u8]) -> DecodeResult {
    decode_at(bytes, DecodeDepth::default()).unwrap()
}

fn primary_dialect(result: &DecodeResult) -> &str {
    result
        .ir()
        .source
        .as_ref()
        .and_then(|source| source.dialect())
        .map(|matched| matched.dialect().as_str())
        .unwrap()
}

fn loss_codes(result: &DecodeResult) -> Vec<cadmpeg_ir::report::loss::LossKind> {
    result
        .report()
        .losses
        .iter()
        .map(|loss| loss.code.clone())
        .collect()
}

#[test]
fn inspect_lists_every_member_with_its_role_and_signature() {
    let bytes = package(&[
        ("model.pvs", PVS_0304),
        ("part_a.ol", OL_08),
        ("part_b.ol", OL_09),
        ("part_c.ol", b"no signature"),
        ("thumbnail.png", b"\x89PNG"),
    ]);
    let summary = inspect(&bytes);

    assert_eq!(summary.format(), "pvz");
    assert_eq!(summary.container_kind.as_str(), "zip");
    let primary = summary.dialects().unwrap().primary();
    assert_eq!(primary.dialect().as_str(), "pvz:pvs-0304");
    assert_eq!(primary.admission(), &Admission::Admitted);
    assert!(summary.losses.is_empty(), "{:?}", summary.losses);

    let rows = summary
        .entries
        .iter()
        .map(|entry| {
            (
                entry.name.as_str(),
                entry.role,
                entry.attributes.get("signature").map(String::as_str),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [
            ("model.pvs", ContainerRole::Document, Some("PV-PVS0304")),
            ("part_a.ol", ContainerRole::ModelData, Some("DIV-OL08BL")),
            ("part_b.ol", ContainerRole::ModelData, Some("DIV-OL09BL")),
            ("part_c.ol", ContainerRole::ModelData, Some("unrecognized")),
            ("thumbnail.png", ContainerRole::Other, None),
        ]
    );
}

#[test]
fn decode_classifies_the_package_and_reports_an_undecodable_structure() {
    let bytes = package(&[("model.pvs", PVS_0301), ("model.ol", OL_09)]);
    let result = decode(&bytes);

    let source = result.ir().source.as_ref().unwrap();
    let primary = source.dialect().unwrap();
    assert_eq!(primary.dialect().as_str(), "pvz:pvs-0301");
    assert_eq!(primary.admission(), &Admission::Admitted);
    assert_eq!(
        source
            .attributes
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect::<Vec<_>>(),
        [
            ("geometry_member_count", "1"),
            ("structure_member", "model.pvs")
        ]
    );

    assert!(!result.report().geometry_transferred());
    assert_eq!(
        loss_codes(&result),
        [PvzLossCode::StructureMemberMalformed.kind()]
    );
    assert!(result.ir().model.product_definitions.is_empty());
    assert!(result.ir().model.tessellations.is_empty());

    let report = cadmpeg_ir::validate_neutral(result.ir(), Vec::new()).unwrap();
    assert!(report.is_ok(), "{report:?}");
}

#[test]
fn every_structure_signature_decodes_the_assembly() {
    for (signature, dialect) in [
        ("PV-PVS0301", "pvz:pvs-0301"),
        ("PV-PVS0302", "pvz:pvs-0302"),
        ("PV-PVS0303", "pvz:pvs-0303"),
        ("PV-PVS0304", "pvz:pvs-0304"),
        ("PV-PVS0305", "pvz:pvs-0305"),
    ] {
        let result = decode(&assembly(signature));
        assert_eq!(primary_dialect(&result), dialect);
        let model = &result.ir().model;
        assert_eq!(
            (
                model.product_definitions.len(),
                model.occurrences.len(),
                model.bodies.len(),
                model.tessellations.len(),
                model.appearances.len(),
                model.appearance_bindings.len(),
            ),
            (3, 4, 2, 2, 1, 1),
            "{signature}"
        );
        assert!(result.report().geometry_transferred());
    }
}

#[test]
fn the_assembly_maps_to_products_placements_meshes_and_appearances() {
    let result = decode(&assembly("PV-PVS0304"));
    let model = &result.ir().model;

    let definitions: Vec<_> = model
        .product_definitions
        .iter()
        .map(|definition| {
            (
                definition.source_name.as_deref().unwrap(),
                definition
                    .bodies
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    assert_eq!(
        definitions,
        [
            ("TOP.ASM", vec![]),
            ("BRACKET.PRT", vec!["pvz:model:body#bracket.ol".to_owned()]),
            ("PLATE.PRT", vec!["pvz:model:body#plate.ol".to_owned()]),
        ]
    );
    assert_eq!(
        model.product_definitions[0]
            .bom_properties
            .values()
            .collect::<Vec<_>>(),
        ["TOP-001"]
    );

    let turned = model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.id.as_str() == "pvz:model:occurrence#2")
        .unwrap();
    assert_eq!(turned.name.as_deref(), Some("42"));
    assert_eq!(
        turned.transform.affine_rows(),
        [
            [0.0, -1.0, 0.0, 0.0],
            [1.0, 0.0, 0.0, 200.0],
            [0.0, 0.0, 1.0, 0.0]
        ]
    );
    assert_eq!(
        turned.scale.map(cadmpeg_ir::scalar::FiniteReal::get),
        [2.0; 3]
    );

    let bracket = &model.tessellations[0];
    assert_eq!(bracket.vertex_count(), 4);
    assert_eq!(bracket.triangle_count(), 4);
    assert!(bracket.vertex_normals().is_empty());
    let plate = &model.tessellations[1];
    assert_eq!(plate.triangles(), [[0, 1, 2], [2, 1, 3]]);
    assert_eq!(plate.vertex_normals().len(), 4);
    let far = plate.vertices()[3].get();
    assert!((far.x - 20.0).abs() < 1e-4 && (far.y - 20.0).abs() < 1e-4);

    let appearance = &model.appearances[0];
    assert_eq!(appearance.name.as_deref(), Some("red"));
    let color = appearance.base_color.unwrap();
    assert_eq!(
        (color.r(), color.g(), color.b(), color.a()),
        (1.0, 0.0, 0.0, 1.0)
    );
    assert_eq!(model.appearance_bindings[0].appearance, appearance.id);

    assert_eq!(
        loss_codes(&result),
        [
            PvzLossCode::StructureBoundsNotTransferred.kind(),
            PvzLossCode::GeometryExactUndecoded.kind(),
            PvzLossCode::AttributeRecordsUndecoded.kind(),
            PvzLossCode::AttributeAppearanceTail.kind(),
            PvzLossCode::TessellationPatchFields.kind(),
            PvzLossCode::TessellationEdgesNotTransferred.kind(),
            PvzLossCode::GeometryPreambleUndecoded.kind(),
        ]
    );
    let report = cadmpeg_ir::validate_neutral(result.ir(), Vec::new()).unwrap();
    assert!(report.is_ok(), "{report:?}");
}

#[test]
fn missing_and_unreferenced_members_are_reported() {
    let bytes = package(&[
        ("model.pvs", &assembly_structure("PV-PVS0301")),
        ("bracket.ol", &bracket_member()),
        ("spare.ol", &plate_member()),
    ]);
    let result = decode(&bytes);
    let codes = loss_codes(&result);
    assert!(codes.contains(&PvzLossCode::GeometryMemberMissing.kind()));
    assert!(codes.contains(&PvzLossCode::GeometryMemberUnreferenced.kind()));
    assert_eq!(result.ir().model.tessellations.len(), 1);
    assert!(result.ir().model.product_definitions[2].bodies.is_empty());
}

#[test]
fn a_malformed_geometry_member_is_reported_and_the_rest_decodes() {
    let bytes = package(&[
        ("model.pvs", &assembly_structure("PV-PVS0301")),
        ("bracket.ol", OL_08),
        ("plate.ol", &plate_member()),
    ]);
    let result = decode(&bytes);
    assert!(loss_codes(&result).contains(&PvzLossCode::GeometryMemberMalformed.kind()));
    assert_eq!(result.ir().model.tessellations.len(), 1);
    assert_eq!(result.ir().model.occurrences.len(), 4);
}

#[test]
fn a_geometry_member_that_does_not_open_is_reported_and_the_rest_decodes() {
    use cadmpeg_test_support::zip_fixture::{archive, RawEntry};

    let bracket = bracket_member();
    let bytes = archive(&[
        RawEntry::stored("model.pvs", &assembly_structure("PV-PVS0301")),
        RawEntry::compressed("bracket.ol", 12, b"not a bzip2 stream", &bracket),
        RawEntry::stored("plate.ol", &plate_member()),
    ]);
    let result = decode(&bytes);
    let malformed: Vec<_> = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == PvzLossCode::GeometryMemberMalformed.kind())
        .collect();
    assert_eq!(malformed.len(), 1);
    assert!(malformed[0]
        .message
        .starts_with("geometry member \"bracket.ol\" did not open"));
    assert_eq!(result.ir().model.tessellations.len(), 1);
    assert_eq!(result.ir().model.occurrences.len(), 4);
}

#[test]
fn a_container_decode_reports_no_content_loss() {
    let result = decode_at(&assembly("PV-PVS0304"), DecodeDepth::Container).unwrap();

    assert!(result.report().container_only());
    assert!(loss_codes(&result).is_empty());
}

#[test]
fn an_unknown_structure_signature_is_residual_and_charged() {
    let bytes = package(&[("model.pvs", b"PV-PVS9999 body"), ("model.ol", OL_09)]);

    let summary = inspect(&bytes);
    let primary = summary.dialects().unwrap().primary();
    assert_eq!(primary.dialect().as_str(), "pvz:unknown");
    assert_eq!(primary.admission(), &Admission::Residual);
    assert!(summary
        .losses
        .iter()
        .any(|loss| loss.code == PvzLossCode::SourceDialectUnverified.kind()));

    let result = decode(&bytes);
    assert_eq!(primary_dialect(&result), "pvz:unknown");
    assert_eq!(
        loss_codes(&result),
        [
            PvzLossCode::SourceDialectUnverified.kind(),
            PvzLossCode::StructureMemberMalformed.kind(),
        ]
    );
}

#[test]
fn an_unreadable_central_directory_is_recovered_and_charged() {
    let bytes = package(&[("model.pvs", PVS_0304), ("model.ol", OL_09)]);
    let central = bytes
        .windows(4)
        .position(|window| window == b"PK\x01\x02")
        .unwrap();
    let local_only = &bytes[..central];

    let result = decode(local_only);
    assert_eq!(primary_dialect(&result), "pvz:pvs-0304");
    assert_eq!(
        loss_codes(&result),
        [
            PvzLossCode::ContainerDirectoryRecovered.kind(),
            PvzLossCode::StructureMemberMalformed.kind(),
        ]
    );
}

#[test]
fn a_package_without_a_structure_member_is_the_wrong_format() {
    let bytes = package(&[("model.ol", OL_09)]);
    assert!(matches!(
        decode_at(&bytes, DecodeDepth::default()),
        Err(CodecError::WrongFormat(_))
    ));
}

#[test]
fn a_package_with_two_structure_members_is_malformed() {
    let bytes = package(&[
        ("a.pvs", PVS_0304),
        ("b.pvs", PVS_0304),
        ("model.ol", OL_09),
    ]);
    assert!(matches!(
        decode_at(&bytes, DecodeDepth::default()),
        Err(CodecError::Malformed(_))
    ));
}

#[test]
fn exact_streams_become_solids_that_keep_their_tessellation() {
    use cadmpeg_ir::topology::BodyKind;

    let result = decode(&exact_assembly());
    let model = &result.ir().model;
    let body = |member: &str| {
        model
            .bodies
            .iter()
            .find(|body| body.id.as_str() == format!("pvz:model:body#{member}"))
            .unwrap()
    };
    for member in ["cube64.ol", "cube32.ol"] {
        let cube = body(member);
        assert_eq!(cube.kind, BodyKind::Solid);
        assert_eq!(cube.name.as_deref(), Some(member));
        assert_eq!(cube.regions.len(), 1);
        let faces = model
            .faces
            .iter()
            .filter(|face| face.id.as_str().contains(&format!("#{member}:")))
            .count();
        assert_eq!(faces, 6);
        let meshes: Vec<_> = model
            .tessellations
            .iter()
            .filter(|mesh| mesh.body.as_ref() == Some(&cube.id))
            .collect();
        assert_eq!(meshes.len(), 1);
        assert_eq!(meshes[0].triangle_count(), 12);
    }
    assert_eq!(body("plate.ol").kind, BodyKind::General);
    assert!(body("plate.ol").regions.is_empty());
    assert_eq!(model.bodies.len(), 3);
    for definition in &model.product_definitions[1..] {
        assert_eq!(definition.bodies.len(), 1);
    }
    let codes = loss_codes(&result);
    assert_eq!(
        codes
            .iter()
            .filter(|code| **code == PvzLossCode::GeometryExactUndecoded.kind())
            .count(),
        1
    );
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == PvzLossCode::GeometryExactUndecoded.kind()
            && loss.message.contains("\"plate.ol\"")
    }));
    assert!(result.report().geometry_transferred());
    let report = cadmpeg_ir::validate_neutral(result.ir(), Vec::new()).unwrap();
    let serious: Vec<_> = report
        .findings
        .iter()
        .filter(|finding| finding.severity >= cadmpeg_ir::report::Severity::Warning)
        .collect();
    assert!(serious.is_empty(), "{serious:?}");
}

#[test]
fn closed_and_open_shells_of_one_member_become_a_solid_and_a_sheet() {
    use cadmpeg_ir::topology::BodyKind;

    let result = decode(&cube_and_sheet_assembly());
    let model = &result.ir().model;
    let of_kind = |kind: BodyKind| {
        let found: Vec<_> = model
            .bodies
            .iter()
            .filter(|body| body.kind == kind)
            .collect();
        assert_eq!(found.len(), 1, "{kind:?}");
        found[0]
    };
    let (solid, sheet) = (of_kind(BodyKind::Solid), of_kind(BodyKind::Sheet));
    assert_eq!(model.bodies.len(), 2);
    for body in [solid, sheet] {
        assert_eq!(body.name.as_deref(), Some("part.ol"));
    }
    let shells = |body: &cadmpeg_ir::topology::Body| -> usize {
        model
            .regions
            .iter()
            .filter(|region| body.regions.contains(&region.id))
            .map(|region| region.shells.len())
            .sum()
    };
    assert_eq!((shells(solid), shells(sheet)), (1, 1));
    let part = model
        .product_definitions
        .iter()
        .find(|definition| definition.bodies.contains(&solid.id))
        .unwrap();
    assert_eq!(part.bodies, [solid.id.clone(), sheet.id.clone()]);
    let meshes: Vec<_> = model
        .tessellations
        .iter()
        .map(|mesh| mesh.body.clone())
        .collect();
    assert_eq!(meshes, [Some(solid.id.clone())]);
    let report = cadmpeg_ir::validate_neutral(result.ir(), Vec::new()).unwrap();
    let serious: Vec<_> = report
        .findings
        .iter()
        .filter(|finding| finding.severity >= cadmpeg_ir::report::Severity::Warning)
        .collect();
    assert!(serious.is_empty(), "{serious:?}");
}
