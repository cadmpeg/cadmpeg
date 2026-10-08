// SPDX-License-Identifier: Apache-2.0
//! Hole ownership and sketch projection admission.

use super::*;

fn hole_ownership_source(geometry: bool) -> Vec<u8> {
    let mut source = if geometry {
        crate::test_support::container::sldprt_with_body(&triangle_body())
    } else {
        outer_header()
    };
    source.extend(make_block(0x43, "Contents/Keywords", br#"<Keywords><HoleWizard Name="Hole" Type="HoleWizard" id="7"/><Sketch Name="Profile" Type="Sketch" id="8"><Dimension Name="Diameter">&lt;MOD-DIAM&gt;4.2</Dimension><Dimension Name="Depth">6.8</Dimension></Sketch><Sketch Name="Position" Type="Sketch" id="9"/></Keywords>"#));
    source
}

fn serialized_name_marker_offset(payload: &[u8], value: &str) -> usize {
    let units = value.encode_utf16().collect::<Vec<_>>();
    let mut marker = vec![
        0x04,
        0x80,
        0xff,
        0xfe,
        0xff,
        u8::try_from(units.len()).expect("name fits u8"),
    ];
    for unit in &units {
        marker.extend_from_slice(&unit.to_le_bytes());
    }
    payload
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("serialized feature name")
}

fn bore_backed_position_source() -> Vec<u8> {
    let mut body = crate::test_support::parasolid::closed_cylinder_body();
    // A reversed cylinder face is an admitted bore carrier. The compact bridge
    // marker follows the tag, attr, sequence, owner, signature, and five refs.
    let cylinder_bridge = body
        .windows(4)
        .position(|window| window[..2] == [0x00, 0x0e] && window[2..] == 10u16.to_be_bytes())
        .expect("closed cylinder face bridge");
    body[cylinder_bridge + 28] = 0x2d;

    // The position-sketch projection needs a plane in the decoded surface lane,
    // so give this plane its own compact face and closed three-edge loop.
    body.extend(crate::test_support::parasolid::plane_carrier(
        200,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    ));
    body.extend(crate::test_support::parasolid::bridge(11, 22, 200));
    body.extend(crate::test_support::parasolid::loop_head(22, 32, 11));
    body.extend(crate::test_support::parasolid::coedge(
        32, 22, 33, 52, 0, 42, false,
    ));
    body.extend(crate::test_support::parasolid::coedge(
        33, 22, 34, 53, 0, 43, false,
    ));
    body.extend(crate::test_support::parasolid::coedge(
        34, 22, 32, 54, 0, 44, false,
    ));
    body.extend(crate::test_support::parasolid::edge_use(42, 0));
    body.extend(crate::test_support::parasolid::edge_use(43, 0));
    body.extend(crate::test_support::parasolid::edge_use(44, 0));
    body.extend(crate::test_support::parasolid::vertex_use(52, 62));
    body.extend(crate::test_support::parasolid::vertex_use(53, 63));
    body.extend(crate::test_support::parasolid::vertex_use(54, 64));
    body.extend(crate::test_support::parasolid::world_point(
        62,
        [0.0, 0.0, 0.0],
    ));
    body.extend(crate::test_support::parasolid::world_point(
        63,
        [1.0, 0.0, 0.0],
    ));
    body.extend(crate::test_support::parasolid::world_point(
        64,
        [0.0, 1.0, 0.0],
    ));

    let mut source = crate::test_support::container::sldprt_with_body(&body);
    source.extend(make_block(
        0x43,
        "Contents/Keywords",
        br#"<Keywords><HoleWizard Name="Hole" Type="HoleWizard" id="7"><Dimension Name="Diameter">2000mm</Dimension><Dimension Name="Depth">10mm</Dimension></HoleWizard><Sketch Name="Position" Type="Sketch" id="9"/></Keywords>"#,
    ));

    let mut resolved = crate::test_support::history::resolved_feature_classes_with_ids(&[
        ("moHoleWzd_c", "Hole", 7),
        ("moProfileFeature_c", "Position", 9),
    ]);
    let hole_name = serialized_name_marker_offset(&resolved, "Hole");
    let hole_trailer = hole_name + 6 + "Hole".encode_utf16().count() * 2;
    resolved[hole_trailer..hole_trailer + 8].copy_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0x40]);
    resolved[hole_trailer + 8..hole_trailer + 12].copy_from_slice(&7u32.to_le_bytes());
    resolved.splice(hole_trailer + 12..hole_trailer + 12, [0; 4]);

    // The hole object body has a 128-byte extent. Its source link is
    // `00 c0` plus the Position source ID and six zero trailer bytes.
    let mut hole_body = [0; 128];
    hole_body[..2].copy_from_slice(&[0x00, 0xc0]);
    hole_body[2..6].copy_from_slice(&9u32.to_le_bytes());
    resolved.splice(hole_trailer + 16..hole_trailer + 16, hole_body);
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &resolved,
    ));
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
fn metadata_hole_ownership_scratch_refuses_work_limit() {
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "copy SLDPRT hole profile ownership",
        |cap| {
            let mut options = options;
            options.policy.limits.max_work_units = cap;
            match SldprtCodec.decode(&mut Cursor::new(hole_ownership_source(false)), &options) {
                Ok(decoded) => Ok(decoded),
                Err(cadmpeg_ir::DecodeFailure::Codec(error)) => Err(error),
                Err(error) => panic!("unexpected decode refusal: {error:?}"),
            }
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && limit.operation == "copy SLDPRT hole profile ownership")
    );
}

#[test]
fn geometry_hole_ownership_scratch_refuses_work_limit() {
    let options = DecodeOptions::default();
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "copy SLDPRT hole profile ownership",
        |cap| {
            let mut options = options;
            options.policy.limits.max_work_units = cap;
            match SldprtCodec.decode(&mut Cursor::new(hole_ownership_source(true)), &options) {
                Ok(decoded) => Ok(decoded),
                Err(cadmpeg_ir::DecodeFailure::Codec(error)) => Err(error),
                Err(error) => panic!("unexpected decode refusal: {error:?}"),
            }
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && limit.operation == "copy SLDPRT hole profile ownership")
    );
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
fn metadata_profiled_hole_projection_scratch_refuses_work_limit() {
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "project SLDPRT profiled hole constructions",
        |cap| {
            let mut options = options;
            options.policy.limits.max_work_units = cap;
            match SldprtCodec.decode(&mut Cursor::new(hole_ownership_source(false)), &options) {
                Ok(decoded) => Ok(decoded),
                Err(cadmpeg_ir::DecodeFailure::Codec(error)) => Err(error),
                Err(error) => panic!("unexpected decode refusal: {error:?}"),
            }
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
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
fn geometry_profiled_hole_projection_scratch_refuses_work_limit() {
    let options = DecodeOptions::default();
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "project SLDPRT profiled hole constructions",
        |cap| {
            let mut options = options;
            options.policy.limits.max_work_units = cap;
            match SldprtCodec.decode(&mut Cursor::new(hole_ownership_source(true)), &options) {
                Ok(decoded) => Ok(decoded),
                Err(cadmpeg_ir::DecodeFailure::Codec(error)) => Err(error),
                Err(error) => panic!("unexpected decode refusal: {error:?}"),
            }
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
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
fn metadata_hole_position_projection_scratch_refuses_work_limit() {
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "project SLDPRT hole position sketches",
        |cap| {
            let mut options = options;
            options.policy.limits.max_work_units = cap;
            match SldprtCodec.decode(&mut Cursor::new(hole_ownership_source(false)), &options) {
                Ok(decoded) => Ok(decoded),
                Err(cadmpeg_ir::DecodeFailure::Codec(error)) => Err(error),
                Err(error) => panic!("unexpected decode refusal: {error:?}"),
            }
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
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
fn geometry_hole_position_projection_scratch_refuses_work_limit() {
    let options = DecodeOptions::default();
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "project SLDPRT hole position sketches",
        |cap| {
            let mut options = options;
            options.policy.limits.max_work_units = cap;
            match SldprtCodec.decode(&mut Cursor::new(hole_ownership_source(true)), &options) {
                Ok(decoded) => Ok(decoded),
                Err(cadmpeg_ir::DecodeFailure::Codec(error)) => Err(error),
                Err(error) => panic!("unexpected decode refusal: {error:?}"),
            }
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && limit.operation == "project SLDPRT hole position sketches")
    );
}

#[test]
fn metadata_bore_backed_position_projection_refuses_collection_limit() {
    let options = DecodeOptions::default();
    let limit = collection_refusal_with_options(
        &bore_backed_position_source(),
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
    let options = DecodeOptions::default();
    let limit = work_refusal_with_request(
        &bore_backed_position_source(),
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
        &bore_backed_position_source(),
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
        &bore_backed_position_source(),
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
