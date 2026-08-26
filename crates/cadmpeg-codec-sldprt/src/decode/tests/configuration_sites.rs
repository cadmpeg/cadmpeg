// SPDX-License-Identifier: Apache-2.0
//! Configuration site selection and partition-synthesis decode tests.
#![allow(clippy::unwrap_used)]
#![allow(unused_imports)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions, Encoder};

use crate::container;
use crate::test_support::*;
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
        .all(|point| point.id.0.contains("@block@")));
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
        .find(|configuration| configuration.name.resolved() == Some("Second"))
        .expect("manifest configuration is projected");

    assert!(second.active.is_active());
    assert_eq!(second.source_index, Some(1));
    assert!(!second.bodies.is_empty());
    assert!(result
        .ir()
        .model
        .configurations
        .iter()
        .filter(|configuration| configuration.name.resolved() == Some("First"))
        .all(|configuration| configuration.active.is_inactive()));
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
