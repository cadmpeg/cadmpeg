// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::features::{
    BooleanOp, ExtrudeDirection, ExtrudeExtent, ExtrudeSide, ExtrudeStart, LinearTermination,
    PlanarProfileRef, SketchProfileRegion,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::NonZeroLength;
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
    SketchPlacement, SketchProfiles,
};

use super::super::cap_regions::compact::{tests::prism_over, Sweep};
use super::super::ScopedExtrudeProfileResolution;
use super::certified_regions_bounded_by;
use crate::design::geometry::ProfileGeometryWork;
use crate::history_records::{AsmDeltaState, AsmHistoricalTopology};

const TOLERANCE: f64 = 1.0e-6;

fn xy_sketch() -> Sketch {
    Sketch {
        id: SketchId::mint("synthetic:test:sketch#stored-loop-cap").unwrap(),
        name: None,
        configuration: None,
        visible: None,
        native_ref: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: SketchProfiles::default(),
    }
}

/// The sides of the rectangle from the origin to `(2, 1)`, a line from its
/// bottom to its top that divides it into two squares, and a line beside it.
fn divided_rectangle() -> Vec<SketchEntity> {
    let sketch = xy_sketch();
    [
        ("bottom", (0.0, 0.0), (2.0, 0.0)),
        ("right", (2.0, 0.0), (2.0, 1.0)),
        ("top", (2.0, 1.0), (0.0, 1.0)),
        ("left", (0.0, 1.0), (0.0, 0.0)),
        ("divider", (1.0, 0.0), (1.0, 1.0)),
        ("beside", (3.0, 0.0), (3.0, 1.0)),
    ]
    .iter()
    .map(|(name, start, end)| {
        SketchEntity::new(
            SketchEntityId::mint(format!("synthetic:test:curve#{name}")).unwrap(),
            sketch.id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(start.0, start.1),
                end: Point2::new(end.0, end.1),
            })
            .unwrap(),
        )
    })
    .collect()
}

/// The region set certified for the stored curves `stored` when a Join of
/// length 2 from the sketch plane adds the prism over the whole rectangle.
fn select(entities: &[SketchEntity], stored: &[usize]) -> Option<Vec<SketchProfileRegion>> {
    let states = [(2, prism_over(2.0)), (1, AsmHistoricalTopology::default())]
        .into_iter()
        .map(|(state_id, topology)| {
            (
                AsmDeltaState {
                    id: format!("synthetic:test:state#{state_id}"),
                    parent: "synthetic:test:history#stored-loop-cap".into(),
                    byte_offset: 0,
                    state_id,
                    version_flag: 1,
                    state_flag: 0,
                    previous_ref: None,
                    next_ref: None,
                    node_index: state_id,
                    partner_ref: None,
                    owner_ref: 0,
                    bulletin_boards: Vec::new(),
                    records: Vec::new(),
                    transition: None,
                },
                Some(topology),
                Vec::new(),
            )
        })
        .collect();
    let history = crate::test_support::history::history(
        "synthetic:test:history#stored-loop-cap".into(),
        0,
        None,
        false,
        states,
    );
    let stored: Vec<_> = stored
        .iter()
        .map(|index| entities[*index].id().clone())
        .collect();
    let selected = crate::test_support::with_decode_context(|ctx| {
        let work = ProfileGeometryWork::new();
        let resolution = ScopedExtrudeProfileResolution {
            entities,
            spatial_entities: &[],
            histories: std::slice::from_ref(&history),
            linear_tolerance: TOLERANCE,
            angular_tolerance: TOLERANCE,
            profile_work: &work,
            ctx,
        };
        certified_regions_bounded_by(
            &xy_sketch(),
            &stored,
            resolution,
            (2, 1),
            Sweep {
                operation: &BooleanOp::Join,
                start: &ExtrudeStart::ProfilePlane {},
                direction: &ExtrudeDirection::ProfileNormal {},
                extent: &ExtrudeExtent::OneSided {
                    side: ExtrudeSide {
                        termination: LinearTermination::Blind {
                            length: NonZeroLength::new(2.0).unwrap(),
                        },
                        draft: None,
                    },
                },
            },
        )
    })
    .unwrap();
    selected.map(|profile| match profile {
        PlanarProfileRef::SketchRegions { regions, .. } => regions.to_vec(),
        _ => panic!("sketch regions"),
    })
}

#[test]
fn a_cap_that_a_curve_outside_the_stored_loop_divides_selects_each_region() {
    let entities = divided_rectangle();
    let regions = select(&entities, &[0, 1, 2, 3]).expect("certified regions");
    assert_eq!(regions.len(), 2);
    let divider = entities[4].id();
    for region in &regions {
        let SketchProfileRegion::Trimmed {
            outer_boundary,
            hole_boundaries,
        } = region
        else {
            panic!("trimmed region");
        };
        assert!(hole_boundaries.is_empty());
        assert!(outer_boundary.iter().any(|use_| use_.entity == *divider));
    }
}

#[test]
fn a_stored_curve_that_bounds_no_certified_region_selects_nothing() {
    let entities = divided_rectangle();
    assert!(select(&entities, &[0, 1, 2, 3, 5]).is_none());
}

mod stale;
