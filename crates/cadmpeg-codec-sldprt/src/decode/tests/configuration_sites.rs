// SPDX-License-Identifier: Apache-2.0
//! Configuration site selection and partition-synthesis decode tests.
#![allow(clippy::unwrap_used)]

use cadmpeg_test_support::EditableDecodeResult;

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::container;
use crate::test_support::container::make_block;
use crate::test_support::container::outer_header;
use crate::test_support::container::sldprt_with_body;
use crate::test_support::container::sldprt_with_colliding_sites;
use crate::test_support::container::sldprt_with_partition_and_deltas;
use crate::test_support::parasolid::owned_triangle;
use crate::test_support::parasolid::parasolid_with_body;
use crate::test_support::parasolid::triangle_body;
use crate::SldprtCodec;

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
        container::active_configuration_index(&crate::test_support::container::scan(&source)),
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
        .all(|configuration| !configuration.active));
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.message
            == "active configuration identity is unresolved; 0 of 3 configuration records are active."
    }));
    assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
        .expect("validate the neutral model")
        .is_ok());
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
    let decoded = EditableDecodeResult::from(
        SldprtCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap(),
    );
    assert_eq!(decoded.ir().model.configurations.len(), 1);
    assert!(decoded.ir().model.configurations[0].active);
    assert_eq!(
        decoded.ir().model.configurations[0].bodies.as_deref(),
        Some(
            decoded
                .ir()
                .model
                .bodies
                .iter()
                .map(|body| body.id.clone())
                .collect::<Vec<_>>()
                .as_slice()
        )
    );
    let mut written = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut written,
    )
    .unwrap();
    let round_trip = SldprtCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        round_trip.ir().model.configurations[0].bodies.as_deref(),
        Some(
            round_trip
                .ir()
                .model
                .bodies
                .iter()
                .map(|body| body.id.clone())
                .collect::<Vec<_>>()
                .as_slice()
        )
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
        crate::test_support::container::scan(&source).blocks[0]
            .section
            .name(),
        Some("Contents/Config-3-Partition")
    );
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.configurations.len(), 1);
    let configuration = &decoded.ir().model.configurations[0];
    assert_eq!(configuration.ordinal, 0);
    assert_eq!(configuration.source_index, Some(3));
    assert!(configuration.active);
    assert_eq!(configuration.name.as_deref(), Some("Config-3"));
    assert_eq!(
        configuration.bodies.as_deref(),
        Some(
            decoded
                .ir()
                .model
                .bodies
                .iter()
                .map(|body| body.id.clone())
                .collect::<Vec<_>>()
                .as_slice()
        )
    );

    let (mut edited, _, fidelity) = decoded.into_parts();
    let moved = edited.model.points[0].position().get();
    edited.model.points[0].set_position(
        cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
            moved.x + 1.0,
            moved.y,
            moved.z,
        ))
        .expect("a finite position is a point"),
    );
    let mut written = Vec::new();
    crate::test_support::plan_fixture_write_with_native_envelope(&edited, &fidelity, &mut written)
        .unwrap();
    let scan = crate::test_support::container::scan(&written);
    assert!(scan
        .blocks
        .iter()
        .any(|block| block.section.name() == Some("Contents/Config-3-Partition")));
    assert!(!scan
        .blocks
        .iter()
        .any(|block| block.section.name() == Some("Contents/Config-0-Partition")));
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
        .any(|point| point.position().get().x == 0.0));
    assert!(result
        .ir()
        .model
        .points
        .iter()
        .any(|point| point.position().get().x == 10_000.0));
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
        .all(|point| point.id.as_str().contains("@block@")));
    let report = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new())
        .expect("validate the neutral model");
    assert!(report.is_ok(), "validation findings: {:?}", report.findings);
}

#[test]
fn decode_does_not_infer_a_source_header_for_unresolved_partition_sites() {
    let mut source = outer_header();
    source.extend(make_block(
        0x20,
        "Contents/Config-0-Partition",
        &parasolid_with_body(
            "first partition header",
            "SCH_SW_33103_11000",
            &owned_triangle(0, 700, 0.0),
        ),
    ));
    source.extend(make_block(
        0x21,
        "Contents/Config-1-Partition",
        &parasolid_with_body(
            "second partition header",
            "SCH_SW_33104_12000",
            &owned_triangle(0, 701, 10_000.0),
        ),
    ));

    let result = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let attributes = &result.ir().source.as_ref().unwrap().attributes;

    assert_eq!(
        attributes.get("sldprt_active_partition_unresolved"),
        Some(&"true".to_string())
    );
    assert!(!attributes.contains_key("parasolid_schema"));
    assert!(!attributes.contains_key("parasolid_description"));
    assert!(result
        .ir()
        .model
        .points
        .iter()
        .all(|point| point.id.as_str().contains("@block@")));
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
        .filter(|point| !point.id.as_str().contains("@block@"))
        .collect::<Vec<_>>();
    assert_eq!(active_points.len(), 3);
    assert!(active_points
        .iter()
        .all(|point| point.position().get().x >= 10_000.0));
    assert_eq!(
        result.ir().source.as_ref().unwrap().attributes["active_parasolid_block"],
        "Contents/Config-1-Partition"
    );
    assert!(
        cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
            .expect("validate the neutral model")
            .is_ok()
    );
}

#[test]
fn decode_uses_the_namespaced_manifest_site_without_source_indices() {
    let mut source = sldprt_with_colliding_sites();
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="First"/><Configuration Name="Second"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x43,
        "Contents/Features",
        br#"<?xml version="1.0"?><swSolidWorks xmlns="http://www.solidworks.com/sw2003/schema"><swModel id="model-0" swConfigurationName="First" swConfigurationId="0"/><swModel id="model-1" swConfigurationName="Second" swConfigurationId="1"/><swConfigurationList><swConfiguration swID="0" swModelRef="model-0" swMostRecentConfiguration="NO"/><swConfiguration swID="1" swModelRef="model-1" swMostRecentConfiguration="YES"/></swConfigurationList></swSolidWorks>"#,
    ));

    let result = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let second = result
        .ir()
        .model
        .configurations
        .iter()
        .find(|configuration| configuration.name.as_deref() == Some("Second"))
        .expect("manifest configuration is projected");

    assert!(second.active);
    assert_eq!(second.source_index, Some(1));
    assert!(second
        .bodies
        .as_deref()
        .is_some_and(|bodies| !bodies.is_empty()));
    assert!(result
        .ir()
        .model
        .configurations
        .iter()
        .filter(|configuration| configuration.name.as_deref() == Some("First"))
        .all(|configuration| !configuration.active));
    assert_eq!(
        result.ir().source.as_ref().unwrap().attributes["sw_configuration_name"],
        "Second"
    );
    assert_eq!(
        result.ir().source.as_ref().unwrap().attributes["active_parasolid_block"],
        "Contents/Config-1-Partition"
    );
    assert!(!result.report().losses.iter().any(|loss| loss
        .message
        .contains("active configuration identity is unresolved")));
}

/// A `Config-<configuration>-FeatureBodies/LocalBodies` block with one
/// Imported feature per plain stream body (spec §3.3).
fn local_bodies_block(configuration: usize, bodies: &[Vec<u8>]) -> Vec<u8> {
    let mut payload = u32::try_from(bodies.len()).unwrap().to_le_bytes().to_vec();
    for (index, body) in bodies.iter().enumerate() {
        let name: Vec<u16> = format!("Imported{}", index + 1).encode_utf16().collect();
        payload.extend([0xff, 0xfe, 0xff, u8::try_from(name.len()).unwrap()]);
        payload.extend(name.iter().flat_map(|unit| unit.to_le_bytes()));
        payload.extend(0u32.to_le_bytes());
        payload.extend(1u32.to_le_bytes());
        let stream = parasolid_with_body(
            ": TRANSMIT FILE created by modeller version 3500125",
            "SCH_SW_33103_11000",
            body,
        );
        payload.extend([0; 5]);
        payload.extend(u32::try_from(stream.len() + 5).unwrap().to_le_bytes());
        payload.push(0);
        payload.extend(stream);
    }
    make_block(
        0x46,
        &format!("Config-{configuration}-FeatureBodies/LocalBodies"),
        &payload,
    )
}

fn decode(source: Vec<u8>) -> cadmpeg_ir::codec::DecodeResult {
    SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap()
}

#[test]
fn an_empty_partition_takes_the_bodies_of_its_imported_features() {
    let mut source = sldprt_with_partition_and_deltas(&[], &[]);
    source.extend(local_bodies_block(0, &[triangle_body(), triangle_body()]));

    let decoded = decode(source);

    assert!(decoded.report().geometry_transferred());
    assert_eq!(decoded.ir().model.bodies.len(), 2);
    assert_eq!(decoded.ir().model.faces.len(), 2);
    let ids: std::collections::BTreeSet<_> = decoded
        .ir()
        .model
        .bodies
        .iter()
        .map(|body| body.id.as_str())
        .collect();
    assert_eq!(ids.len(), 2);
    assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
        .expect("validate the neutral model")
        .is_ok());
}

#[test]
fn an_imported_body_whose_faces_store_a_tolerance_decodes() {
    let mut body = triangle_body();
    let sentinel = [0xc2, 0xbc, 0x92, 0x8f, 0x99, 0x6e, 0x00, 0x00];
    let at = (0..body.len() - 18)
        .find(|&at| body[at..at + 2] == [0x00, 0x0e] && body[at + 10..at + 18] == sentinel)
        .unwrap();
    body[at + 10..at + 18].copy_from_slice(&1.0e-8f64.to_be_bytes());
    let mut source = sldprt_with_partition_and_deltas(&[], &[]);
    source.extend(local_bodies_block(0, &[body]));

    let decoded = decode(source);

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.faces.len(), 1);
}

#[test]
fn a_partition_with_geometry_keeps_its_own_bodies() {
    let mut source = sldprt_with_body(&triangle_body());
    let alone = decode(source.clone());
    source.extend(local_bodies_block(0, &[triangle_body()]));

    let decoded = decode(source);

    assert_eq!(
        decoded.ir().model.bodies.len(),
        alone.ir().model.bodies.len()
    );
    assert_eq!(decoded.ir().model.faces.len(), alone.ir().model.faces.len());
}

/// One block with an empty partition/deltas model, named `section`.
fn empty_model_block(section: &str) -> Vec<u8> {
    let mut payload = parasolid_with_body("partition body", "SCH_SW_33103_11000", &[]);
    payload.extend(parasolid_with_body(
        "deltas body",
        "SCH_SW_33103_11000",
        &[],
    ));
    make_block(0x20, section, &payload)
}

#[test]
fn imported_bodies_stay_out_while_another_site_has_geometry() {
    let mut source = outer_header();
    source.extend(make_block(
        0x21,
        "Contents/Config-0-Partition",
        &parasolid_with_body("partition body", "SCH_SW_33103_11000", &triangle_body()),
    ));
    source.extend(empty_model_block("Contents/Config-1-Partition"));
    let alone = decode(source.clone());
    source.extend(local_bodies_block(1, &[triangle_body()]));

    let decoded = decode(source);

    assert_eq!(
        decoded.ir().model.bodies.len(),
        alone.ir().model.bodies.len()
    );
}

#[test]
fn imported_bodies_need_a_configuration() {
    let mut source = outer_header();
    source.extend(empty_model_block("Contents/Model"));
    source.extend(local_bodies_block(0, &[triangle_body()]));

    let decoded = decode(source);

    assert!(decoded.ir().model.bodies.is_empty());
}

#[test]
fn imported_bodies_of_another_configuration_are_not_taken() {
    let mut source = sldprt_with_partition_and_deltas(&[], &[]);
    source.extend(local_bodies_block(1, &[triangle_body()]));

    let decoded = decode(source);

    assert!(decoded.ir().model.bodies.is_empty());
}

#[test]
fn imported_body_streams_that_do_not_decode_are_reported() {
    assert!(super::super::imported_bodies_loss(0, 2).is_none());
    let loss = super::super::imported_bodies_loss(1, 2).unwrap();
    assert_eq!(
        loss.code,
        crate::loss::SldprtLossCode::GeometryImportedBodiesNotTransferred.kind()
    );
    assert_eq!(
        loss.message,
        "1 of 2 imported body stream(s) do not decode."
    );
}
