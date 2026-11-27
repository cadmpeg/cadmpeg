// SPDX-License-Identifier: Apache-2.0
//! Codec-wide inspect and decode contracts over synthetic packages.

use std::io::Cursor;

use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::{DecodeDepth, InspectOptions};
use cadmpeg_core::dialect::Admission;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_ir::unknown::part::record_unreadable_kind;
use cadmpeg_ir::ContainerSummary;

use crate::loss::PvzLossCode;
use crate::test_support::{
    assembly, assembly_structure, bracket_member, cube_and_sheet_assembly, exact_assembly, package,
    plate_member, OL_08, OL_09, PVS_0301, PVS_0304,
};
use crate::PvzCodec;

mod curved_solids;

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
    assert_eq!(summary.losses.len(), 0);

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
        [("structure_member", "model.pvs")]
    );

    assert!(!result.report().geometry_transferred());
    assert_eq!(loss_codes(&result), [record_unreadable_kind()]);
    let message = &result.report().losses[0].message;
    assert!(
        message.starts_with("part archive-entry \"model.pvs\" stream=\"\" offset=")
            && message.ends_with(
                "; the package carries no product structure, and none of its 1 .ol geometry \
                 members was placed; dropped=[]"
            ),
        "{message}"
    );
    assert!(
        cadmpeg_ir::products::stated_definitions(&result.ir().model.product_definitions)
            .next()
            .is_none()
    );
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
        let geometry = &result.ir().geometry;
        assert_eq!(
            (
                model.product_definitions.len(),
                model.occurrences.len(),
                geometry.bodies.len(),
                model.tessellations.len(),
                model.appearances.len(),
                model.appearance_bindings.len(),
            ),
            (3, 3, 0, 2, 1, 1),
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
            ("BRACKET.PRT", vec![]),
            ("PLATE.PRT", vec![]),
        ]
    );
    for (mesh, definition) in model
        .tessellations
        .iter()
        .zip(&model.product_definitions[1..])
    {
        assert_eq!(
            mesh.of,
            cadmpeg_ir::tessellation::Meshed::definition(definition.id.clone())
        );
    }
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
        .find(|occurrence| occurrence.id.as_str() == "pvz:model:occurrence#1")
        .unwrap();
    assert_eq!(turned.name.as_deref(), Some("42"));
    assert_eq!(
        turned.transform.unwrap().affine_rows(),
        [
            [0.0, -2.0, 0.0, 0.0],
            [2.0, 0.0, 0.0, 200.0],
            [0.0, 0.0, 2.0, 0.0]
        ]
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
            cadmpeg_ir::unknown::part::record_unreadable_kind(),
        ]
    );
    let report = cadmpeg_ir::validate_neutral(result.ir(), Vec::new()).unwrap();
    assert!(report.is_ok(), "{report:?}");
}

#[test]
fn a_pinned_decode_refuses_an_exact_stream_that_does_not_decode() {
    let bytes = assembly("PV-PVS0304");
    let admitted = decode(&bytes);
    let given_up = admitted
        .report()
        .losses
        .iter()
        .find(|loss| loss.code == record_unreadable_kind())
        .expect("the exact stream is given up");
    assert!(
        given_up
            .message
            .starts_with("part ol-stream stream=\"pvz:plate.ol\" offset="),
        "{given_up:?}"
    );
    let stubs = admitted.ir().native_unknowns("pvz").unwrap();
    let [stub] = stubs.as_slice() else {
        panic!("one stub: {stubs:?}");
    };
    assert!(given_up.message.contains(&format!("record={}", stub.id)));

    let mut options = DecodeOptions::default();
    options.strictness.pinned = true;
    let failure = PvzCodec
        .decode(&mut Cursor::new(bytes.as_slice()), &options)
        .expect_err("a pinned decode refuses a given-up exact stream");
    let cadmpeg_ir::DecodeFailure::StrictRejected { rejection } = &failure else {
        panic!("expected a strict rejection, got {failure:?}");
    };
    assert_eq!(rejection.loss().message, given_up.message);
}

/// A package whose structure places one leaf definition `count` times.
fn wide_package(count: usize) -> Vec<u8> {
    use crate::test_support::pvs::{self, definition, occurrence, Node, Place};
    let pool = Node::leaf(0x01, pvs::string_pool_payload(&[])).encode();
    let children = (0..count)
        .map(|_| occurrence(1, Place::Identity, ""))
        .collect();
    let structure = Node::list(
        0x02,
        vec![
            definition("TOP.ASM", children),
            definition("LEAF.PRT", vec![]),
        ],
    )
    .encode();
    let member = pvs::member("PV-PVS0304", &[(1, "", pool), (2, "", structure)]);
    package(&[("model.pvs", &member)])
}

#[test]
fn occurrences_past_the_limit_are_given_up_and_a_pinned_decode_refuses_them() {
    let limit = crate::decode::MAX_OCCURRENCES;
    let bytes = wide_package(limit + 2);
    let admitted = decode(&bytes);
    assert_eq!(admitted.ir().model.occurrences.len(), limit);
    let charges: Vec<_> = admitted
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == record_unreadable_kind())
        .collect();
    let [given_up] = charges.as_slice() else {
        panic!("one charge: {charges:?}");
    };
    let stopped = crate::ids::occurrence(limit);
    let dropped = crate::ids::occurrence(limit + 1);
    assert!(
        given_up
            .message
            .starts_with(&format!("record {stopped} given up: ")),
        "{given_up:?}"
    );
    assert!(
        given_up
            .message
            .ends_with(&format!("; dropped=[{dropped}]")),
        "{given_up:?}"
    );

    let mut options = DecodeOptions::default();
    options.strictness.pinned = true;
    let failure = PvzCodec
        .decode(&mut Cursor::new(bytes.as_slice()), &options)
        .expect_err("a pinned decode refuses the occurrences past the limit");
    let cadmpeg_ir::DecodeFailure::StrictRejected { rejection } = &failure else {
        panic!("expected a strict rejection, got {failure:?}");
    };
    assert_eq!(rejection.loss().message, given_up.message);
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
    assert!(loss_codes(&result).contains(&record_unreadable_kind()));
    assert_eq!(result.ir().model.tessellations.len(), 1);
    // §8.1: the root definition owns the three placements.
    assert_eq!(result.ir().model.occurrences.len(), 3);
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
    let (malformed, exact): (Vec<_>, Vec<_>) = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == record_unreadable_kind())
        .partition(|loss| loss.message.starts_with("part archive-entry "));
    assert_eq!(malformed.len(), 1);
    assert!(
        exact.iter().all(|loss| loss
            .message
            .starts_with("part ol-stream stream=\"pvz:plate.ol\" offset=")),
        "{exact:?}"
    );
    assert!(malformed[0]
        .message
        .starts_with("part archive-entry \"bracket.ol\" stream=\"\" offset="));
    assert!(malformed[0].message.ends_with("; dropped=[]"));
    assert_eq!(result.ir().model.tessellations.len(), 1);
    // §8.1: the root definition owns the three placements.
    assert_eq!(result.ir().model.occurrences.len(), 3);
    let stubs = result.ir().native_unknowns("pvz").unwrap();
    let stub = stubs
        .iter()
        .find(|stub| {
            malformed[0]
                .message
                .contains(&format!("record={}", stub.id))
        })
        .expect("the stub of the member");
    let [link] = stub.links.as_slice() else {
        panic!("the stub links the definition that names the member");
    };
    assert!(result
        .ir()
        .model
        .product_definitions
        .iter()
        .any(|definition| cadmpeg_ir::ids::Identity::from(definition.id.clone()) == *link));
    let (_, _, fidelity) = result.into_parts();
    let retained = fidelity.retained_record(stub.id.as_str()).unwrap();
    assert_eq!(retained.data(), Some(b"not a bzip2 stream".as_slice()));
    let model = std::collections::BTreeSet::from([cadmpeg_core::decode::ArenaClass::Model]);
    assert_eq!(retained.untyped(), &model);
    assert_eq!(fidelity.unrepresented(), model);
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
            record_unreadable_kind(),
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
            record_unreadable_kind(),
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
    let geometry = &result.ir().geometry;
    let body = |member: &str| {
        geometry
            .bodies
            .iter()
            .find(|body| body.id.as_str() == format!("pvz:model:body#{member}"))
            .unwrap()
    };
    for member in ["cube64.ol", "cube32.ol"] {
        let cube = body(member);
        assert_eq!(cube.kind, BodyKind::Solid);
        assert_eq!(cube.name, None);
        assert_eq!(geometry.stated_body_regions(&cube.id).count(), 1);
        let faces = geometry
            .faces
            .iter()
            .filter(|face| face.id.as_str().contains(&format!("#{member}:")))
            .count();
        assert_eq!(faces, 6);
        let meshes: Vec<_> = model
            .tessellations
            .iter()
            .filter(|mesh| mesh.of.body() == Some(&cube.id))
            .collect();
        assert_eq!(meshes.len(), 1);
        assert_eq!(meshes[0].triangle_count(), 12);
    }
    assert_eq!(geometry.bodies.len(), 2);
    let plate = model
        .product_definitions
        .iter()
        .find(|definition| definition.bodies.is_empty() && definition.occurrences.is_empty())
        .unwrap();
    assert!(model
        .tessellations
        .iter()
        .any(|mesh| mesh.of == cadmpeg_ir::tessellation::Meshed::definition(plate.id.clone())));
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
    let geometry = &result.ir().geometry;
    let of_kind = |kind: BodyKind| {
        let found: Vec<_> = geometry
            .bodies
            .iter()
            .filter(|body| body.kind == kind)
            .collect();
        assert_eq!(found.len(), 1, "{kind:?}");
        found[0]
    };
    let (solid, sheet) = (of_kind(BodyKind::Solid), of_kind(BodyKind::Sheet));
    assert_eq!(geometry.bodies.len(), 2);
    for body in [solid, sheet] {
        assert_eq!(body.name, None);
    }
    let shells = |body: &cadmpeg_ir::topology::Body| -> usize {
        geometry
            .regions
            .iter()
            .filter(|region| region.body == body.id)
            .map(|region| region.stated_shells().count())
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
        .map(|mesh| mesh.of.body().cloned())
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
