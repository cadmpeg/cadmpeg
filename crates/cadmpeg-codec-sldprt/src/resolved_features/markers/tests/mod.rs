//! Tests for the `markers` module.

use super::compact_linked_profile_vertex;
use super::sketch_input_entities;
use super::sketch_marker_at;
use super::terminal_wide_geometry_locus_profile_vertex;
use cadmpeg_ir::units::FiniteVector;

fn marker_coordinates(payload: &[u8], offset: usize) -> Option<FiniteVector<2>> {
    super::standard_marker_result(super::marker_coordinates(
        &super::StandardMarkerAdmission,
        payload,
        offset,
    ))
}

fn marker_spatial_coordinates(payload: &[u8], offset: usize) -> Option<cadmpeg_ir::math::Point3> {
    super::standard_marker_result(super::marker_spatial_coordinates(
        &super::StandardMarkerAdmission,
        payload,
        offset,
    ))
}

fn marker_spatial_coordinate_offset(payload: &[u8], offset: usize) -> Option<usize> {
    super::standard_marker_result(super::marker_spatial_coordinate_offset(
        &super::StandardMarkerAdmission,
        payload,
        offset,
    ))
}

fn spatial_relation_marker_coordinates(
    payload: &[u8],
    offset: usize,
) -> Option<cadmpeg_ir::math::Point3> {
    super::standard_marker_result(super::spatial_relation_marker_coordinates(
        &super::StandardMarkerAdmission,
        payload,
        offset,
    ))
}

fn marker_local_id(payload: &[u8], offset: usize) -> Option<u32> {
    super::standard_marker_result(super::marker_local_id(
        &super::StandardMarkerAdmission,
        payload,
        offset,
    ))
}

fn compact_legacy_profile_vertex(payload: &[u8], offset: usize) -> bool {
    super::standard_marker_result(super::compact_legacy_profile_vertex(
        &super::StandardMarkerAdmission,
        payload,
        offset,
    ))
}

fn linked_profile_vertex(payload: &[u8], offset: usize) -> bool {
    super::standard_marker_result(super::linked_profile_vertex(
        &super::StandardMarkerAdmission,
        payload,
        offset,
    ))
}

fn geometry_locus_profile_vertex(payload: &[u8], offset: usize) -> bool {
    super::standard_marker_result(super::geometry_locus_profile_vertex(
        &super::StandardMarkerAdmission,
        payload,
        offset,
    ))
}

fn extended_geometry_locus_single_link_point(payload: &[u8], offset: usize) -> bool {
    super::extended_geometry_locus_single_link_point(
        &cadmpeg_test_support::service_decode_context(),
        payload,
        offset,
    )
    .expect("extended geometry-locus scan fits service policy")
}

#[test]
fn coordinate_pair_reader_admits_finite_values_and_rejects_nonfinite_values() {
    let mut payload = [0u8; 16];
    payload[..8].copy_from_slice(&1.5f64.to_le_bytes());
    payload[8..].copy_from_slice(&(-2.5f64).to_le_bytes());
    assert_eq!(
        super::finite_coordinate_pair(&payload, 0).map(FiniteVector::get),
        Some([1.5, -2.5])
    );

    payload[8..].copy_from_slice(&f64::NAN.to_le_bytes());
    assert!(super::finite_coordinate_pair(&payload, 0).is_none());
    payload[8..].copy_from_slice(&f64::INFINITY.to_le_bytes());
    assert!(super::finite_coordinate_pair(&payload, 0).is_none());
}

fn raw2(value: Option<FiniteVector<2>>) -> Option<[f64; 2]> {
    value.map(FiniteVector::get)
}

fn raw_pairs<const N: usize>(value: Option<[FiniteVector<2>; N]>) -> Option<[[f64; 2]; N]> {
    value.map(|coordinates| coordinates.map(FiniteVector::get))
}

fn raw_link<T>(value: Option<(FiniteVector<2>, T)>) -> Option<([f64; 2], T)> {
    value.map(|(coordinates, links)| (coordinates.get(), links))
}

mod alternate_profile_points;
mod dimension_carriers;
mod four_link_profile_points;
mod lanes;
mod profile_curves;
mod profile_points;
mod spatial;
mod spatial_boundaries;
