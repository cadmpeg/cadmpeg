//! Tests for the `markers` module.

use super::compact_linked_profile_vertex;
use super::linked_profile_vertex;
use super::sketch_input_entities;
use super::sketch_marker_at;
use super::terminal_wide_geometry_locus_profile_vertex;
use cadmpeg_ir::units::FiniteVector;

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
