// SPDX-License-Identifier: Apache-2.0
//! The picked-entity rules on the fixture cube's current state: the fixture's
//! extrude takes each operation in turn, its picks resolved to cube entities.

use std::collections::{BTreeMap, BTreeSet};

use super::super::fixture::{self, design_findings};
use crate::design::construction::{kinds, Construction};
use crate::design::dimension::Dimension;
use crate::design::feature::FeatureOperation;
use crate::design::names::{NameKind, TopoName};
use crate::design::ops::body::Graft;
use crate::design::ops::dressup::{
    BlendControls, Chamfer, ChamferSet, ChamferSize, Corner, EdgeSetback, Fillet, FilletSection,
    FilletSet, FilletSize, Setbacks, Stations,
};
use crate::design::ops::sheet_metal::{
    BendSet, BendSpec, Convert, ConvertBend, ConvertFrom, Flange, FlangeWidth, SheetMetal,
    SheetMetalKind, StampForm, StampOutline, VertexBend,
};
use crate::design::ops::surface::{Extend, ExtendLimit, Patch, PatchBounds, PatchEdge, Unsew};
use crate::design::ops::swept::{
    Guide, Loft, LoftShape, Revolve, SpineLimits, Sweep, SweepControls, SweepSection,
};
use crate::design::options::{
    BodyForm, CornerRound, SectionCondition, SheetOutput, SolidOutput, Support, Thickness,
    ThinWall, WallSide,
};
use crate::design::refs::EntityRef;
use crate::design::selection::{Anchor, Members, PickTarget, Picked, Picks, SetSelection};
use crate::design::selection::{MemberOrder, Selection};
use crate::document::CadIr;
use crate::features::geometry::FinitePoint3;
use crate::geometry::analytic::{CircleCurve, CylinderSurface, SphereSurface};
use crate::geometry::nurbs::{NurbsCurve, NurbsPoles3};
use crate::geometry::{CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry};
use crate::ids::VertexId;
use crate::math::{Point3, Vector3};
use crate::scalar::{Angle, NonNegativeLength, PositiveLength};
use crate::topology::{BodyKind, EdgeCarrier};
use crate::transform::Transform;

/// A pick of `entity`, told apart from the other picks by `key`.
fn pick(key: u32, entity: EntityRef) -> Picked {
    Picked {
        target: PickTarget::Entity(TopoName::Generated {
            kind: NameKind::Face,
            feature: fixture::feature("extrude"),
            role: None,
            generator: None,
            index: Some(key),
        }),
        context: None,
        native: None,
        domain: None,
        witness: None,
        resolved: Some(vec![entity]),
    }
}

/// A selection in the current state of `entities`.
fn current<O: MemberOrder>(entities: Vec<EntityRef>) -> Selection<O> {
    let picks = entities
        .into_iter()
        .enumerate()
        .map(|(key, entity)| pick(u32::try_from(key).unwrap(), entity))
        .collect();
    Selection {
        at: Anchor::Current,
        picks: Picks::Named(Members::new(picks).unwrap()),
        native: None,
    }
}

fn face(ir: &CadIr, index: usize) -> EntityRef {
    EntityRef::Face(ir.geometry.faces[index].id.clone())
}

fn edge(ir: &CadIr, index: usize) -> EntityRef {
    EntityRef::Edge(ir.geometry.edges[index].id.clone())
}

fn body(ir: &CadIr) -> EntityRef {
    EntityRef::Body(ir.geometry.bodies[0].id.clone())
}

fn point(x: f64, y: f64, z: f64) -> Construction<kinds::Point> {
    Construction {
        inputs: Vec::new(),
        after: Vec::new(),
        solution: None,
        mode: None,
        resolved: FinitePoint3::new(Point3::new(x, y, z)),
        resolved_in: BTreeMap::new(),
    }
}

fn positive(value: f64) -> Dimension<PositiveLength> {
    Dimension::Literal(PositiveLength::new(value).unwrap())
}

fn nonnegative(value: f64) -> Dimension<NonNegativeLength> {
    Dimension::Literal(NonNegativeLength::new(value).unwrap())
}

fn sheet(kind: SheetMetalKind) -> FeatureOperation {
    FeatureOperation::SheetMetal(Box::new(SheetMetal {
        kind,
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    }))
}

/// Gives the fixture's extrude the operation `make` builds from the fixture.
fn with(ir: &mut CadIr, make: impl FnOnce(&CadIr) -> FeatureOperation) {
    let operation = make(ir);
    ir.design.features[0].definition.operation = operation;
}

/// The position of a vertex.
fn place(ir: &CadIr, vertex: &VertexId) -> Point3 {
    let point = &ir
        .geometry
        .vertices
        .iter()
        .find(|record| record.id == *vertex)
        .unwrap()
        .point;
    let point = ir.geometry.points.iter().find(|record| record.id == *point);
    point.unwrap().position().get()
}

/// The height of a vertex.
fn height(ir: &CadIr, vertex: &VertexId) -> f64 {
    place(ir, vertex).z
}

/// The edges of the cube's bottom face: those with both ends at height 0.
fn bottom_edges(ir: &CadIr) -> Vec<usize> {
    level_edges(ir, 0.0)
}

/// The edges with both ends at height `z`.
fn level_edges(ir: &CadIr, z: f64) -> Vec<usize> {
    let edges = ir.geometry.edges.iter().enumerate();
    edges
        .filter(|(_, edge)| edge.vertices().all(|end| height(ir, end) == z))
        .map(|(index, _)| index)
        .collect()
}

/// The cube's bottom face: the face that uses the bottom edges.
fn bottom(ir: &CadIr) -> usize {
    level(ir, 0.0)
}

/// The face of the cube at height `z`: the face that uses the edges there.
fn level(ir: &CadIr, z: f64) -> usize {
    let edges: BTreeSet<_> = level_edges(ir, z)
        .into_iter()
        .map(|index| &ir.geometry.edges[index].id)
        .collect();
    let record = ir
        .geometry
        .loops
        .iter()
        .find(|record| {
            ir.geometry
                .coedges
                .iter()
                .filter(|coedge| coedge.owner_loop == record.id)
                .all(|coedge| edges.contains(&coedge.edge))
        })
        .unwrap();
    let mut faces = ir.geometry.faces.iter();
    faces.position(|face| face.id == record.face).unwrap()
}

/// Makes the bottom face's surface the cylinder of radius 5 about the
/// vertical axis through the cube's middle.
fn curve_bottom(ir: &mut CadIr) {
    let surface = ir.geometry.faces[bottom(ir)].surface.clone();
    let cylinder = CylinderSurface::try_new(
        Point3::new(5.0, 5.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
        5.0,
    )
    .unwrap();
    let record = ir
        .geometry
        .surfaces
        .iter_mut()
        .find(|record| record.id == surface)
        .unwrap();
    record.geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder));
}

/// Makes the surface of face `index` the sphere of `radius` about the
/// cube's middle.
fn sphere_face(ir: &mut CadIr, index: usize, radius: f64) {
    let surface = ir.geometry.faces[index].surface.clone();
    let sphere = SphereSurface::try_new(
        Point3::new(5.0, 5.0, 5.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
        radius,
    )
    .unwrap();
    let record = ir
        .geometry
        .surfaces
        .iter_mut()
        .find(|record| record.id == surface)
        .unwrap();
    record.geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere));
}

fn set_kind(ir: &mut CadIr, kind: BodyKind) {
    ir.geometry.bodies[0].kind = kind;
}

/// The messages of the findings of `rule`.
fn ruled(ir: &CadIr, rule: &str) -> Vec<String> {
    design_findings(ir)
        .into_iter()
        .filter(|finding| finding.rule.as_str() == rule)
        .map(|finding| finding.message)
        .collect()
}

/// The vertex edge 0 starts at, and the indices of the edges that end at it.
fn corner(ir: &CadIr) -> (EntityRef, Vec<usize>) {
    let vertex = ir.geometry.edges[0].start().unwrap().clone();
    let edges = ir
        .geometry
        .edges
        .iter()
        .enumerate()
        .filter(|(_, edge)| edge.vertices().any(|end| *end == vertex))
        .map(|(index, _)| index)
        .collect();
    (EntityRef::Vertex(vertex), edges)
}

/// An edge that touches no end of edge 0.
fn apart(ir: &CadIr) -> usize {
    let ends: BTreeSet<_> = ir.geometry.edges[0].vertices().collect();
    ir.geometry
        .edges
        .iter()
        .position(|edge| edge.vertices().all(|end| !ends.contains(end)))
        .unwrap()
}

#[test]
fn a_fixed_face_is_planar_and_a_bend_pick_is_not() {
    let unfold = |ir: &CadIr| {
        sheet(SheetMetalKind::Unfold {
            fixed: Some(current(vec![face(ir, bottom(ir))])),
            bends: Some(BendSet::Selected(current(vec![face(ir, bottom(ir))]))),
        })
    };
    let mut ir = fixture::document();
    with(&mut ir, unfold);
    assert!(ruled(&ir, "design.picked-face-not-planar").is_empty());
    assert_eq!(ruled(&ir, "design.bend-pick-planar-face").len(), 1);
    curve_bottom(&mut ir);
    assert_eq!(ruled(&ir, "design.picked-face-not-planar").len(), 1);
    assert!(ruled(&ir, "design.bend-pick-planar-face").is_empty());
}

#[test]
fn a_circle_outline_center_lies_on_the_entry_plane() {
    let hole = |ir: &CadIr, z: f64| {
        sheet(SheetMetalKind::ExtrudedHole {
            bore: StampOutline::Circle {
                center: point(5.0, 5.0, z),
                diameter: positive(2.0),
            },
            entry: Some(current(vec![face(ir, bottom(ir))])),
            height: positive(1.0),
            root_fillet: None,
            rule: None,
        })
    };
    let rule = "design.circle-outline-center-off-entry-plane";
    let mut ir = fixture::document();
    with(&mut ir, |ir| hole(ir, 0.0));
    assert!(ruled(&ir, rule).is_empty());
    with(&mut ir, |ir| hole(ir, 1.0));
    assert_eq!(ruled(&ir, rule).len(), 1);
}

#[test]
fn a_stiffening_rib_point_lies_on_its_bend_face() {
    let rib = |ir: &CadIr, x: f64| {
        sheet(SheetMetalKind::Stamp {
            entry: None,
            form: StampForm::StiffeningRib {
                bend: current(vec![face(ir, bottom(ir))]),
                at: point(x, 5.0, 3.0),
                length: positive(1.0),
                bottom_radius: positive(1.0),
                wall_angle: Dimension::Literal(Angle::new(0.5).unwrap()),
                rim_radius: positive(1.0),
            },
            rule: None,
        })
    };
    let rule = "design.stiffening-rib-point-off-bend";
    let mut ir = fixture::document();
    curve_bottom(&mut ir);
    with(&mut ir, |ir| rib(ir, 10.0));
    assert!(ruled(&ir, rule).is_empty());
    assert!(ruled(&ir, "design.bend-pick-planar-face").is_empty());
    with(&mut ir, |ir| rib(ir, 5.0));
    assert_eq!(ruled(&ir, rule).len(), 1);
}

#[test]
fn a_graft_picks_a_solid_body_and_a_sheet() {
    let graft = |ir: &CadIr| {
        FeatureOperation::Graft(Box::new(Graft {
            body: current(vec![body(ir)]),
            sheet: current(vec![body(ir)]),
            flip: None,
            simplify: None,
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        }))
    };
    let rule = "design.graft-pick-body-wrong-kind";
    let mut ir = fixture::document();
    with(&mut ir, graft);
    let messages = ruled(&ir, rule);
    assert!(
        matches!(messages.as_slice(), [message] if message.starts_with("Graft.sheet")),
        "{messages:?}"
    );
    set_kind(&mut ir, BodyKind::Sheet);
    let messages = ruled(&ir, rule);
    assert!(
        matches!(messages.as_slice(), [message] if message.starts_with("Graft.body")),
        "{messages:?}"
    );
}

#[test]
fn unsew_edges_divide_one_sheet_in_two() {
    let unsew = |ir: &CadIr, edges: &[usize]| {
        FeatureOperation::Unsew(Box::new(Unsew {
            edges: current(edges.iter().map(|index| edge(ir, *index)).collect()),
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        }))
    };
    let shared = "design.unsew-edge-not-shared-sheet";
    let divide = "design.unsew-edges-do-not-divide-sheet";
    let mut ir = fixture::document();
    with(&mut ir, |ir| unsew(ir, &[0]));
    assert_eq!(ruled(&ir, shared).len(), 1);
    set_kind(&mut ir, BodyKind::Sheet);
    assert!(ruled(&ir, shared).is_empty());
    assert_eq!(ruled(&ir, divide).len(), 1);
    with(&mut ir, |ir| unsew(ir, &bottom_edges(ir)));
    assert!(ruled(&ir, shared).is_empty());
    assert!(ruled(&ir, divide).is_empty());
}

fn controls(corners: Vec<Corner>) -> BlendControls {
    BlendControls {
        corner_style: None,
        corners,
        limits: Vec::new(),
        keep: None,
        transitions: Vec::new(),
        overflow: None,
        keep_features: None,
        round_corners: None,
    }
}

fn chamfer(sets: Vec<ChamferSet>, corners: Vec<Corner>) -> FeatureOperation {
    FeatureOperation::Chamfer(Box::new(Chamfer {
        sets,
        controls: controls(corners),
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    }))
}

fn setbacks(ir: &CadIr, edges: &[usize]) -> Vec<EdgeSetback> {
    edges
        .iter()
        .map(|index| EdgeSetback {
            edge: current(vec![edge(ir, *index)]),
            distance: nonnegative(1.0),
        })
        .collect()
}

#[test]
fn a_corner_setback_edge_ends_at_the_corner_vertex() {
    let corner_at = |ir: &CadIr, edges: &[usize]| {
        let (vertex, _) = corner(ir);
        chamfer(
            Vec::new(),
            vec![Corner {
                vertex: current(vec![vertex]),
                setbacks: Some(Setbacks::PerEdge(setbacks(ir, edges))),
            }],
        )
    };
    let rule = "design.edge-setback-edge-not-incident";
    let mut ir = fixture::document();
    let (_, at) = corner(&ir);
    with(&mut ir, |ir| corner_at(ir, &at));
    assert!(ruled(&ir, rule).is_empty());
    with(&mut ir, |ir| corner_at(ir, &[at[0], apart(ir)]));
    assert_eq!(ruled(&ir, rule).len(), 1);
}

#[test]
fn a_vertex_chamfer_edges_meet_at_one_vertex() {
    let vertex_chamfer = |ir: &CadIr, edges: &[usize]| {
        let set = ChamferSet {
            edges: current(vec![edge(ir, edges[0])]),
            size: Some(ChamferSize::Vertex(setbacks(ir, edges))),
            reference: None,
            flip: None,
            measure: None,
        };
        chamfer(vec![set], Vec::new())
    };
    let rule = "design.chamfer-vertex-edges-meet-no-vertex";
    let mut ir = fixture::document();
    let (_, at) = corner(&ir);
    with(&mut ir, |ir| vertex_chamfer(ir, &at));
    assert!(ruled(&ir, rule).is_empty());
    with(&mut ir, |ir| vertex_chamfer(ir, &[at[0], at[1], apart(ir)]));
    assert_eq!(ruled(&ir, rule).len(), 1);
}

#[test]
fn flange_setbacks_from_the_ends_fit_the_edge() {
    let flange = |ir: &CadIr, second: f64| {
        sheet(SheetMetalKind::Flange(Flange {
            edges: current(vec![edge(ir, 0)]),
            height: None,
            angle: None,
            width: Some(FlangeWidth::FromEnds {
                first: nonnegative(4.0),
                second: nonnegative(second),
            }),
            bend: bend(),
            rule: None,
            height_datum: None,
            position: None,
            outline: None,
        }))
    };
    let rule = "design.from-ends-setbacks-exceed-edge";
    let mut ir = fixture::document();
    with(&mut ir, |ir| flange(ir, 6.0));
    assert!(ruled(&ir, rule).is_empty());
    with(&mut ir, |ir| flange(ir, 7.0));
    assert_eq!(ruled(&ir, rule).len(), 1);
}

fn bend() -> BendSpec {
    BendSpec {
        radius: None,
        radius_side: None,
        relief: None,
    }
}

#[test]
fn a_convert_bend_face_is_a_cylindrical_face_of_the_solid() {
    let convert = |ir: &CadIr| {
        sheet(SheetMetalKind::Convert(Convert {
            body: current(vec![body(ir)]),
            fixed: current(vec![face(ir, (bottom(ir) + 1) % 6)]),
            thickness: positive(1.0),
            from: ConvertFrom::Faces {
                side: None,
                bends: vec![ConvertBend::Face {
                    face: current(vec![face(ir, bottom(ir))]),
                }],
            },
            rips: Vec::new(),
            bend: bend(),
            keep_solid: None,
            rule: None,
        }))
    };
    let rule = "design.convert-bend-face-not-cylindrical";
    let mut ir = fixture::document();
    with(&mut ir, convert);
    assert_eq!(ruled(&ir, rule).len(), 1);
    assert!(ruled(&ir, "design.convert-pick-outside-solid").is_empty());
    curve_bottom(&mut ir);
    assert!(ruled(&ir, rule).is_empty());
}

#[test]
fn a_pick_without_resolved_entities_or_a_matching_name_is_not_judged() {
    let mut ir = fixture::document();
    let mut fixed: SetSelection = current(vec![face(&ir, bottom(&ir))]);
    if let Picks::Named(members) = &mut fixed.picks {
        let mut picks = members.as_slice().to_vec();
        picks[0].resolved = None;
        *members = Members::new(picks).unwrap();
    }
    with(&mut ir, |_| {
        sheet(SheetMetalKind::FlatPattern { fixed: Some(fixed) })
    });
    curve_bottom(&mut ir);
    assert!(ruled(&ir, "design.picked-face-not-planar").is_empty());
}

fn edges(ir: &CadIr, edges: &[usize]) -> Vec<EntityRef> {
    edges.iter().map(|index| edge(ir, *index)).collect()
}

/// A vertex at which `degree` ends of `edges` meet.
fn meeting(ir: &CadIr, edges: &[usize], degree: usize) -> EntityRef {
    let mut degrees = BTreeMap::new();
    for index in edges {
        for end in ir.geometry.edges[*index].vertices() {
            *degrees.entry(end.clone()).or_insert(0) += 1;
        }
    }
    let (vertex, _) = degrees
        .into_iter()
        .find(|(_, count)| *count == degree)
        .unwrap();
    EntityRef::Vertex(vertex)
}

/// Makes the curve of edge `index` the circle of radius 5 about the vertical
/// axis through the cube's middle.
fn round_edge(ir: &mut CadIr, index: usize) {
    let (EdgeCarrier::Curve(curve) | EdgeCarrier::Bounded(curve, _)) =
        ir.geometry.edges[index].carrier.clone()
    else {
        unreachable!("the fixture's edges lie on curves")
    };
    let circle = CircleCurve::try_new(
        Point3::new(5.0, 5.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
        5.0,
    )
    .unwrap();
    let record = ir
        .geometry
        .curves
        .iter_mut()
        .find(|record| record.id == curve);
    record.unwrap().geometry = CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle));
}

/// Makes the curve of edge `index` the NURBS curve of degree one between
/// its ends.
fn free_edge(ir: &mut CadIr, index: usize) {
    let edge = &ir.geometry.edges[index];
    let ends: Vec<Point3> = edge.vertices().map(|end| place(ir, end)).collect();
    let (EdgeCarrier::Curve(curve) | EdgeCarrier::Bounded(curve, _)) = edge.carrier.clone() else {
        unreachable!("the fixture's edges lie on curves")
    };
    let nurbs = NurbsCurve::new(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        NurbsPoles3::from_lanes(ends, None).unwrap(),
        false,
    )
    .unwrap();
    let record = ir
        .geometry
        .curves
        .iter_mut()
        .find(|record| record.id == curve);
    record.unwrap().geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs));
}

/// Places the cube's body by the affine map of `rows`.
fn place_body(ir: &mut CadIr, rows: [[f64; 4]; 3]) {
    ir.geometry.bodies[0].transform = Some(Transform::affine(rows).unwrap());
}

#[test]
fn a_contour_flange_profile_is_one_open_chain_bent_where_two_pieces_meet() {
    let contour = |ir: &CadIr, profile: &[usize], at: EntityRef| {
        sheet(SheetMetalKind::ContourFlange {
            profile: current(edges(ir, profile)),
            edges: None,
            width: None,
            extent: None,
            thickness: None,
            bend: bend(),
            vertex_bends: vec![VertexBend {
                vertex: current(vec![at]),
                radius: positive(1.0),
                radius_side: None,
            }],
            rule: None,
        })
    };
    let open = "design.contour-flange-profile-not-open-chain";
    let joint = "design.vertex-bend-not-chain-joint";
    let mut ir = fixture::document();
    let around = bottom_edges(&ir);
    let three = &around[..3];
    with(&mut ir, |ir| contour(ir, three, meeting(ir, three, 2)));
    assert!(ruled(&ir, open).is_empty());
    assert!(ruled(&ir, joint).is_empty());
    with(&mut ir, |ir| contour(ir, three, meeting(ir, three, 1)));
    assert!(ruled(&ir, open).is_empty());
    assert_eq!(ruled(&ir, joint).len(), 1);
    with(&mut ir, |ir| contour(ir, &around, meeting(ir, &around, 2)));
    assert_eq!(ruled(&ir, open).len(), 1);
    assert!(ruled(&ir, joint).is_empty());
}

#[test]
fn a_variable_fillet_chain_starts_at_an_end_of_its_chain() {
    let fillet = |ir: &CadIr, chain: &[usize], start: EntityRef| {
        let stations = Stations {
            points: Vec::new(),
            law: None,
            chain_start: Some(current(vec![start])),
        };
        FeatureOperation::Fillet(Box::new(Fillet {
            sets: vec![FilletSet::Edges {
                edges: current(edges(ir, chain)),
                section: FilletSection {
                    size: Some(FilletSize::Variable(stations)),
                    profile: None,
                    weight: None,
                },
                spine: None,
                hold: [None, None],
            }],
            controls: controls(Vec::new()),
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        }))
    };
    let rule = "design.chain-start-not-chain-end";
    let mut ir = fixture::document();
    let around = bottom_edges(&ir);
    let three = &around[..3];
    with(&mut ir, |ir| fillet(ir, three, meeting(ir, three, 1)));
    assert!(ruled(&ir, rule).is_empty());
    with(&mut ir, |ir| fillet(ir, three, meeting(ir, three, 2)));
    assert_eq!(ruled(&ir, rule).len(), 1);
    with(&mut ir, |ir| fillet(ir, &around, meeting(ir, &around, 2)));
    assert!(ruled(&ir, rule).is_empty());
}

fn patch(outer: Vec<PatchEdge>) -> FeatureOperation {
    FeatureOperation::Patch(Box::new(Patch {
        bounds: PatchBounds::Loops {
            outer,
            inner: Vec::new(),
        },
        passing: None,
        planar: None,
        simplified: None,
        output: SheetOutput {
            solid: None,
            boolean: None,
        },
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    }))
}

#[test]
fn a_patch_support_face_meets_its_boundary_curve() {
    let supported = |ir: &CadIr, curve: usize, face_index: usize| {
        patch(vec![PatchEdge {
            curve: current(vec![edge(ir, curve)]),
            condition: Some(SectionCondition::Tangent {
                support: Some(Support::Faces(current(vec![face(ir, face_index)]))),
                weight: None,
                flip: None,
            }),
        }])
    };
    let rule = "design.patch-support-face-off-curve";
    let mut ir = fixture::document();
    let top = level_edges(&ir, 10.0)[0];
    with(&mut ir, |ir| supported(ir, bottom_edges(ir)[0], bottom(ir)));
    assert!(ruled(&ir, rule).is_empty());
    with(&mut ir, |ir| supported(ir, top, level(ir, 10.0)));
    assert!(ruled(&ir, rule).is_empty());
    with(&mut ir, |ir| supported(ir, top, bottom(ir)));
    assert_eq!(ruled(&ir, rule).len(), 1);
    round_edge(&mut ir, top);
    assert!(ruled(&ir, rule).is_empty());
}

#[test]
fn a_patch_support_on_a_sphere_meets_its_boundary_curve() {
    let rule = "design.patch-support-face-off-curve";
    let mut ir = fixture::document();
    let top = level_edges(&ir, 10.0)[0];
    let floor = bottom(&ir);
    with(&mut ir, |ir| {
        patch(vec![PatchEdge {
            curve: current(vec![edge(ir, top)]),
            condition: Some(SectionCondition::Tangent {
                support: Some(Support::Faces(current(vec![face(ir, floor)]))),
                weight: None,
                flip: None,
            }),
        }])
    });
    // A top edge comes within 50^0.5 of the cube's middle at its midpoint,
    // and its ends lie 75^0.5 from it.
    sphere_face(&mut ir, floor, 50.0_f64.sqrt());
    assert!(ruled(&ir, rule).is_empty());
    sphere_face(&mut ir, floor, 1.0);
    assert_eq!(ruled(&ir, rule).len(), 1);
    sphere_face(&mut ir, floor, 10.0);
    assert_eq!(ruled(&ir, rule).len(), 1);
}

#[test]
fn a_natural_extend_picks_whole_face_loops() {
    let extend = |ir: &CadIr, picked: &[usize]| {
        FeatureOperation::Extend(Box::new(Extend {
            edges: current(edges(ir, picked)),
            limit: Some(ExtendLimit::Natural),
            method: None,
            output: None,
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        }))
    };
    let rule = "design.natural-extend-edge-not-loop";
    let mut ir = fixture::document();
    let around = bottom_edges(&ir);
    with(&mut ir, |ir| extend(ir, &around));
    assert!(ruled(&ir, rule).is_empty());
    with(&mut ir, |ir| extend(ir, &around[..3]));
    assert_eq!(ruled(&ir, rule).len(), 3);
}

#[test]
fn a_patch_loop_is_one_closed_chain() {
    let looped = |ir: &CadIr, outer: &[usize]| {
        let outer = outer.iter().map(|index| PatchEdge {
            curve: current(vec![edge(ir, *index)]),
            condition: None,
        });
        patch(outer.collect())
    };
    let rule = "design.patch-loop-not-closed-chain";
    let mut ir = fixture::document();
    let around = bottom_edges(&ir);
    with(&mut ir, |ir| looped(ir, &around));
    assert!(ruled(&ir, rule).is_empty());
    with(&mut ir, |ir| looped(ir, &around[..3]));
    assert_eq!(ruled(&ir, rule).len(), 1);
}

#[test]
fn a_guide_path_is_one_connected_chain() {
    let loft = |ir: &CadIr, path: &[usize]| {
        FeatureOperation::Loft(Box::new(Loft {
            start: None,
            sections: Vec::new(),
            end: None,
            cross: Vec::new(),
            spine: None,
            guides: vec![Guide {
                path: current(edges(ir, path)),
                tangency: None,
                influence: None,
            }],
            correspondence: None,
            shape: LoftShape {
                style: None,
                max_degree: None,
                rebuild: None,
                ruled: None,
                closed: None,
                relimit: None,
                merge_tangent_faces: None,
                smoothing: None,
                section_law: None,
            },
            output: SolidOutput {
                body: None,
                boolean: None,
            },
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        }))
    };
    let rule = "design.path-picks-not-one-chain";
    let mut ir = fixture::document();
    let around = bottom_edges(&ir);
    let (_, star) = corner(&ir);
    with(&mut ir, |ir| loft(ir, &around[..3]));
    assert!(ruled(&ir, rule).is_empty());
    with(&mut ir, |ir| loft(ir, &around));
    assert!(ruled(&ir, rule).is_empty());
    with(&mut ir, |ir| loft(ir, &star));
    assert_eq!(ruled(&ir, rule).len(), 1);
    with(&mut ir, |ir| loft(ir, &[0, apart(ir)]));
    assert_eq!(ruled(&ir, rule).len(), 1);
}

#[test]
fn a_thin_wall_side_and_corners_fit_whether_its_profile_closes() {
    let revolve = |ir: &CadIr, profile: &[usize], side: WallSide, corners: Option<CornerRound>| {
        let wall = ThinWall {
            thickness: Thickness::OneSide {
                value: positive(1.0),
                side: Some(side),
            },
            ends: None,
            corner_round: corners,
        };
        FeatureOperation::Revolve(Box::new(Revolve {
            profile: current(edges(ir, profile)),
            loops: None,
            axis: None,
            span: None,
            fit: None,
            output: SolidOutput {
                body: Some(BodyForm::Thin(wall)),
                boolean: None,
            },
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        }))
    };
    let side = "design.wall-side-profile-closure";
    let closed = "design.corner-round-closed-profile";
    let lines = "design.round-corner-profile-not-lines";
    let mut ir = fixture::document();
    let around = bottom_edges(&ir);
    let three = &around[..3];
    with(&mut ir, |ir| revolve(ir, &around, WallSide::Inside, None));
    assert!(ruled(&ir, side).is_empty());
    with(&mut ir, |ir| revolve(ir, &around, WallSide::Left, None));
    assert_eq!(ruled(&ir, side).len(), 1);
    with(&mut ir, |ir| revolve(ir, three, WallSide::Outside, None));
    assert_eq!(ruled(&ir, side).len(), 1);
    let sharp = Some(CornerRound::Sharp);
    with(&mut ir, |ir| {
        revolve(ir, &around, WallSide::Inside, sharp.clone())
    });
    assert_eq!(ruled(&ir, closed).len(), 1);
    with(&mut ir, |ir| revolve(ir, three, WallSide::Left, sharp));
    assert!(ruled(&ir, closed).is_empty());
    assert!(ruled(&ir, side).is_empty());
    let round = Some(CornerRound::Round(positive(1.0)));
    with(&mut ir, |ir| revolve(ir, three, WallSide::Left, round));
    assert!(ruled(&ir, lines).is_empty());
    round_edge(&mut ir, three[0]);
    let found = ruled(&ir, lines);
    assert_eq!(found.len(), 1);
    assert!(found[0].contains("picks edge "), "{found:?}");
}

/// A sweep along the edges `path` of the cube, limited at its start by
/// `start`.
fn spine_sweep(ir: &CadIr, path: &[usize], start: EntityRef) -> FeatureOperation {
    FeatureOperation::Sweep(Box::new(Sweep {
        section: SweepSection::Profile(current(vec![face(ir, bottom(ir))])),
        path: current(edges(ir, path)),
        controls: SweepControls {
            orientation: None,
            twist: None,
            scaling: None,
        },
        guides: Vec::new(),
        extent: None,
        placement: None,
        limits: SpineLimits {
            start: Some(current(vec![start])),
            end: None,
        },
        transition: None,
        output: SolidOutput {
            body: None,
            boolean: None,
        },
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    }))
}

#[test]
fn a_spine_limit_lies_on_the_spine() {
    let rule = "design.spine-limit-off-spine";
    let mut ir = fixture::document();
    let around = bottom_edges(&ir);
    let three = &around[..3];
    let raised = ir.geometry.edges[level_edges(&ir, 10.0)[0]]
        .start()
        .unwrap()
        .clone();
    with(&mut ir, |ir| spine_sweep(ir, three, meeting(ir, three, 1)));
    assert!(ruled(&ir, rule).is_empty());
    with(&mut ir, |ir| spine_sweep(ir, three, face(ir, bottom(ir))));
    assert!(ruled(&ir, rule).is_empty());
    with(&mut ir, |ir| {
        spine_sweep(ir, three, face(ir, level(ir, 10.0)))
    });
    assert_eq!(ruled(&ir, rule).len(), 1);
    with(&mut ir, |ir| {
        spine_sweep(ir, three, EntityRef::Vertex(raised))
    });
    assert_eq!(ruled(&ir, rule).len(), 1);
}

#[test]
fn a_spine_limit_lies_on_a_free_form_or_placed_spine() {
    let rule = "design.spine-limit-off-spine";
    let mut ir = fixture::document();
    let three = bottom_edges(&ir)[..3].to_vec();
    let raised = ir.geometry.edges[level_edges(&ir, 10.0)[0]]
        .start()
        .unwrap()
        .clone();
    for &index in &three {
        free_edge(&mut ir, index);
    }
    with(&mut ir, |ir| spine_sweep(ir, &three, face(ir, bottom(ir))));
    assert!(ruled(&ir, rule).is_empty());
    with(&mut ir, |ir| {
        spine_sweep(ir, &three, face(ir, level(ir, 10.0)))
    });
    assert_eq!(ruled(&ir, rule).len(), 1);
    // A rigid placement moves the spine and the face together.
    place_body(
        &mut ir,
        [
            [0.0, -1.0, 0.0, 3.0],
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 100.0],
        ],
    );
    assert_eq!(ruled(&ir, rule).len(), 1);
    with(&mut ir, |ir| {
        spine_sweep(ir, &three, EntityRef::Vertex(raised))
    });
    assert_eq!(ruled(&ir, rule).len(), 1);
    // A placement that scales is not rigid, and no bound reads it.
    place_body(
        &mut ir,
        [
            [2.0, 0.0, 0.0, 0.0],
            [0.0, 2.0, 0.0, 0.0],
            [0.0, 0.0, 2.0, 0.0],
        ],
    );
    assert!(ruled(&ir, rule).is_empty());
}

#[test]
fn a_spine_limit_resolves_to_a_point_or_a_plane() {
    let rule = "design.spine-limit-resolves-no-point-or-plane";
    let mut ir = fixture::document();
    let three = bottom_edges(&ir)[..3].to_vec();
    with(&mut ir, |ir| spine_sweep(ir, &three, face(ir, bottom(ir))));
    assert!(ruled(&ir, rule).is_empty());
    with(&mut ir, |ir| spine_sweep(ir, &three, edge(ir, three[0])));
    assert_eq!(ruled(&ir, rule).len(), 1);
    with(&mut ir, |ir| spine_sweep(ir, &three, body(ir)));
    assert_eq!(ruled(&ir, rule).len(), 1);
    with(&mut ir, |ir| spine_sweep(ir, &three, face(ir, bottom(ir))));
    curve_bottom(&mut ir);
    assert_eq!(ruled(&ir, rule).len(), 1);
}

mod operands;
mod sketch_profiles;
