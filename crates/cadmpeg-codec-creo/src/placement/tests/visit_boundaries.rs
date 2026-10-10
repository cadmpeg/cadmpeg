// SPDX-License-Identifier: Apache-2.0
use super::*;
use cadmpeg_core::decode::{   ResourceDimension};
use cadmpeg_core::CodecError;

fn section(reference_planes: ReferencePlanes) -> FeatureSection3d {
    FeatureSection3d {
        sketch_plane_entity_id: None,
        sketch_plane_flip: None,
        reference_planes,
        reference_plane_datum_geometry_id: None,
        orientation: FeatureSectionOrientation::default(),
        dimension_ids: Vec::new(),
        offset: 17,
    }
}

fn empty_sources(rows: &crate::surface::SurfaceRows) -> PlacementSources<'_> {
    PlacementSources {
        datums: &[],
        surface_rows: rows,
        model_planes: &[],
        outline_planes: &[],
        plane_envelopes: &[],
        surface_parameters: &[],
        geometry_tables: &[],
        affected_ids: &[],
    }
}


#[test]
fn carrier_reference_selection_admits_present_ids_and_stops_at_first_conflict() {
    for (ids, expected) in [
        (vec![7], Some(7)),
        (vec![7, 7], Some(7)),
        (vec![7, 8, 7, 7], None),
    ] {
        let positional = ids
            .iter()
            .map(|id| FeatureSectionReferencePlane {
                plane_entity_id: *id,
                reference_type: None,
                external_reference_id: None,
                segment_id: None,
                sub_index: None,
                reference_flip: None,
            })
            .collect();
        let cases = [
            section(ReferencePlanes::Named(ids)),
            section(ReferencePlanes::Positional(positional)),
        ];
        for section in &cases {
            let actual = crate::test_support::assert_work_boundaries(
                &["creo carrier reference ID selection"],
                |ctx| super::super::unique_carrier_reference_id(ctx, section),
            );
            assert_eq!(actual, expected);
            if expected.is_none() {
                let short = match &section.reference_planes {
                    ReferencePlanes::Named(ids) => {
                        self::section(ReferencePlanes::Named(ids[..2].to_vec()))
                    }
                    ReferencePlanes::Positional(ids) => {
                        self::section(ReferencePlanes::Positional(ids[..2].to_vec()))
                    }
                };
                let refusal = |section: &FeatureSection3d| {
                    let error = crate::test_support::last_refusal_at(
                        &[],
                        ResourceDimension::WorkUnits,
                        "creo carrier reference ID selection",
                        |ctx| super::super::unique_carrier_reference_id(ctx, section),
                    );
                    let CodecError::ResourceLimit(refusal) = error else {
                        panic!("carrier reference work boundary");
                    };
                    refusal
                };
                assert_eq!(
                    refusal(&short),
                    refusal(section),
                    "conflict ends visits before the tail"
                );
            }
        }
    }
}



#[test]
fn generated_parent_admits_each_present_datum_plane_and_envelope_without_terminal_fee() {
    let datums = [datum(2, crate::axis::Axis::X, 2.0)];
    let rows = crate::surface::unique_rows::UniqueIdRows::from_rows(vec![SurfaceRow {
        id: 7,
        kind: SurfaceKind::Plane,
        feature_id: 1,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 17,
    }]);
    let empty_rows = crate::surface::unique_rows::UniqueIdRows::from_rows(Vec::new());
    let envelopes = [PlaneEnvelopeRecord {
        surface_id: 7,
        body: Vec::new(),
        envelope: PlaneEnvelope::Standard {
            bounds_2d: [[None; 2]; 2],
            corners_3d: [[Some(2.0), None, None]; 2],
        },
        corner_coordinate_equal: [Some(true), None, None],
        scalar_tokens: Vec::new(),
        row_offset: 17,
        offset: 18,
    }];
    let equation = Some(SignedPlaneEquation {
        normal: [1.0, 0.0, 0.0],
        offset: 2.0,
    });
    let cases = [
        (
            PlacementSources {
                datums: &datums,
                ..empty_sources(&empty_rows)
            },
            "creo generated datum feature datum traversal",
            equation,
        ),
        (
            empty_sources(&rows),
            "creo generated datum feature plane traversal",
            None,
        ),
        (
            PlacementSources {
                plane_envelopes: &envelopes,
                ..empty_sources(&rows)
            },
            "creo generated datum envelope traversal",
            equation,
        ),
    ];
    for (sources, operation, expected) in cases {
        let actual = crate::test_support::assert_work_boundaries(&[operation], |ctx| {
            let mut lookup = super::super::PlacementLookup::new(ctx, &sources, &[])?;
            super::super::generated_parent_plane_equation(ctx, 1, [0.0, 1.0, 0.0], &mut lookup)
        });
        assert_eq!(actual, expected);
    }
}

