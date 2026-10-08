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

fn hole_bound_sketch_source() -> Vec<u8> {
    let mut source =
        crate::test_support::history::sldprt_with_nested_sketch_profile(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    source
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
            match SldprtCodec.decode(&mut Cursor::new(hole_bound_sketch_source()), &options) {
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
            match SldprtCodec.decode(&mut Cursor::new(hole_bound_sketch_source()), &options) {
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
            match SldprtCodec.decode(&mut Cursor::new(hole_bound_sketch_source()), &options) {
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
            match SldprtCodec.decode(&mut Cursor::new(hole_bound_sketch_source()), &options) {
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
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let limit = collection_refusal_with_options(
        &hole_ownership_source(false),
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
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let limit = work_refusal_with_request(
        &hole_ownership_source(false),
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
        &hole_ownership_source(true),
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
        &hole_ownership_source(true),
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
