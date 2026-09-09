// SPDX-License-Identifier: Apache-2.0

use crate::decode::coverage::{
    constraint_kind_breakdown, curve_transfer_coverage, design_constraint_transfer_coverage,
    surface_transfer_coverage,
};
use crate::decode::sketch_transfer::profiles::{
    normalize_section_incidence_curve_family_evidence, SectionEntityIncidenceFamily,
};
use crate::decode::sketch_transfer::skamp_constraints::sketch_constraint_loci_compatible;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, ProceduralSurface, ProceduralSurfaceDefinition, SolvedSurfaceGeometry,
    Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, ProceduralSurfaceId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::sketches::{
    SketchConstraint, SketchConstraintDefinitionInput, SketchConstraintId, SketchEntityId,
    SketchGeometry, SketchId, SketchLocus,
};
use cadmpeg_ir::SourceObjectAssociation;
use std::collections::{BTreeMap, BTreeSet};

#[test]
// These checked constructors must accept the explicit test fixtures.
#[allow(clippy::unwrap_used)]
fn surface_coverage_separates_transferred_unique_rows_from_ambiguous_ids() {
    let row = |id, kind: crate::surface::SurfaceKind| crate::surface::SurfaceRow {
        id,
        kind,
        feature_id: 17,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    };
    let rows = vec![
        row(41, crate::surface::SurfaceKind::Plane),
        row(42, crate::surface::SurfaceKind::Cylinder),
        row(
            44,
            crate::surface::SurfaceKind::Extrusion(crate::surface::ExtrusionVariant::Linear),
        ),
        row(43, crate::surface::SurfaceKind::Cone),
        row(43, crate::surface::SurfaceKind::Cone),
    ];
    let plane = |id: &str, native_id: u32| Surface {
        id: SurfaceId::mint(format!("test:model:surface#{id}")).expect("identity grammar"),
        geometry: SurfaceGeometry::Plane(
            cadmpeg_ir::geometry::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        ),
        source_object: Some(SourceObjectAssociation {
            format: cadmpeg_ir::CodecFormat::Creo,
            object_id: cadmpeg_ir::products::NonEmptyString::new(format!("VisibGeom:{native_id}"))
                .expect("nonempty source identity"),
            name: None,
            color: None,
            visible: None,
            layer: None,
            instance_path: Vec::new(),
        }),
    };
    let mut surfaces = vec![
        plane("derived-id-independent-of-native-id", 41),
        plane("wrong-family", 42),
        plane("extrusion-carrier", 44),
    ];
    let cache = surfaces[2].geometry.clone();
    surfaces[2].geometry = SurfaceGeometry::Procedural {
        construction: ProceduralSurfaceId::mint(
            "test:model:entity#extrusion-construction".to_string(),
        )
        .expect("identity grammar"),
        cache: Some(SolvedSurfaceGeometry::new(cache).unwrap()),
    };
    let procedural_surfaces = vec![ProceduralSurface::new(
        ProceduralSurfaceId::mint("test:model:entity#extrusion-construction".to_string())
            .expect("identity grammar"),
        ProceduralSurfaceDefinition::Extrusion(
            cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction::try_new(
                CurveId::mint("test:model:entity#directrix".to_string()).expect("identity grammar"),
                None,
                Vector3::new(0.0, 0.0, 1.0),
                None,
                None,
            )
            .unwrap(),
        ),
        None,
    )
    .unwrap()];

    let coverage = surface_transfer_coverage(&rows, &surfaces, &procedural_surfaces);

    assert_eq!(coverage.unique_rows(), 3);
    assert_eq!(coverage.transferred_rows(), 2);
    assert_eq!(coverage.ambiguous_rows(), 2);
    assert_eq!(coverage.family(crate::surface::SurfaceKind::Plane), (1, 1));
    assert_eq!(
        coverage.family(crate::surface::SurfaceKind::Cylinder),
        (1, 0)
    );
    assert_eq!(coverage.family(crate::surface::SurfaceKind::Cone), (0, 0));
    assert_eq!(
        coverage.family(crate::surface::SurfaceKind::Extrusion(
            crate::surface::ExtrusionVariant::Linear
        )),
        (1, 1)
    );
}

#[test]
fn curve_coverage_excludes_unknown_carriers_and_ambiguous_ids() {
    let row = |id, type_byte| crate::curve::CurveTopologyRow {
        id,
        type_byte,
        feature_id: 17,
        directions: [0x01, 0xf6],
        faces: [std::num::NonZeroU32::new(1), std::num::NonZeroU32::new(2)],
        next_edges: [id, id],
        offset: 0,
    };
    let rows = vec![row(41, 0x05), row(42, 0x13), row(43, 0x05), row(43, 0x05)];
    let source = |native_id| SourceObjectAssociation {
        format: cadmpeg_ir::CodecFormat::Creo,
        object_id: cadmpeg_ir::products::NonEmptyString::new(format!("VisibGeom:{native_id}"))
            .expect("nonempty source identity"),
        name: None,
        color: None,
        visible: None,
        layer: None,
        instance_path: Vec::new(),
    };
    let curves = vec![
        Curve {
            id: CurveId::mint("test:model:entity#typed".to_string()).expect("identity grammar"),
            geometry: CurveGeometry::Line(
                cadmpeg_ir::geometry::LineCurve::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("valid LineCurve fixture"),
            ),
            source_object: Some(source(41)),
        },
        Curve {
            id: CurveId::mint("test:model:entity#opaque".to_string()).expect("identity grammar"),
            geometry: CurveGeometry::Unknown { record: None },
            source_object: Some(source(42)),
        },
    ];

    let coverage = curve_transfer_coverage(&rows, &curves);

    assert_eq!(coverage.unique_rows(), 2);
    assert_eq!(coverage.transferred_rows(), 1);
    assert_eq!(coverage.ambiguous_rows(), 2);
    assert_eq!(coverage.by_type()[&0x05], (1, 1));
    assert_eq!(coverage.by_type()[&0x13], (1, 0));
}

#[test]
fn design_constraint_coverage_separates_typed_and_native_constraints() {
    let sketch =
        SketchId::mint("synthetic:test:id#sketch".to_string()).expect("valid test fixture");
    let constraint = |id: &str, definition| SketchConstraint {
        id: SketchConstraintId::mint(format!("synthetic:test:id#{id}"))
            .expect("valid test fixture"),
        sketch: sketch.clone(),
        definition: cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(definition)
            .expect("valid test fixture"),
        name: None,
        driving: None,
        active: None,
        virtual_space: None,
        visible: None,
        orientation: None,
        label_distance: None,
        label_position: None,
        metadata: None,
        native_ref: None,
    };
    let entity =
        SketchEntityId::mint("synthetic:test:id#entity".to_string()).expect("valid test fixture");
    let mut constraints = vec![
        constraint(
            "sketch:relation:1",
            SketchConstraintDefinitionInput::Fixed {
                entity: entity.clone(),
            },
        ),
        constraint(
            "sketch:relation:2",
            SketchConstraintDefinitionInput::Native {
                native_kind: cadmpeg_ir::products::NonEmptyString::new("creo:relation:9")
                    .expect("nonempty native kind"),
                entities: vec![entity.clone()],
                parameter: None,
                operands: Vec::new(),
                native_state: None,
                native_flags: None,
                native_properties: std::collections::BTreeMap::new(),
            },
        ),
        constraint(
            "sketch:skamp:3",
            SketchConstraintDefinitionInput::Fixed { entity },
        ),
    ];
    constraints[0].active = Some(true);
    constraints[1].active = Some(true);
    constraints[2].active = Some(false);

    let coverage =
        design_constraint_transfer_coverage(&constraints, ":relation:", "creo:relation:");

    assert_eq!(coverage.transferred, 2);
    assert_eq!(coverage.native, 1);
    assert_eq!(coverage.typed(), 1);
    assert_eq!(coverage.active, 2);
    assert_eq!(coverage.active_native, 1);
    assert_eq!(coverage.active_typed(), 1);
    assert_eq!(coverage.native_by_kind, BTreeMap::from([(9, 1)]));
    assert_eq!(coverage.active_native_by_kind, BTreeMap::from([(9, 1)]));
    let mut report_coverage = cadmpeg_ir::Coverage::default();
    report_coverage.record_indexed(
        crate::coverage::ACTIVE_NATIVE_FEATURE_RELATION_TYPE_CONSTRAINT_COUNT,
        1,
        2,
    );
    report_coverage.record_indexed(
        crate::coverage::ACTIVE_NATIVE_FEATURE_RELATION_TYPE_CONSTRAINT_COUNT,
        9,
        1,
    );
    report_coverage.record_indexed(
        crate::coverage::TRANSFERRED_NATIVE_FEATURE_RELATION_TYPE_CONSTRAINT_COUNT,
        9,
        4,
    );
    assert_eq!(
        constraint_kind_breakdown(&report_coverage, "active_native_feature_relation_type_",),
        "type 1=2, type 9=1"
    );
}

#[test]
fn native_curve_families_accept_only_their_defined_loci() {
    let point =
        SketchEntityId::mint("synthetic:test:id#point".to_string()).expect("valid test fixture");
    let bounded =
        SketchEntityId::mint("synthetic:test:id#bounded".to_string()).expect("valid test fixture");
    let line =
        SketchEntityId::mint("synthetic:test:id#line".to_string()).expect("valid test fixture");
    let reference_line = SketchEntityId::mint("synthetic:test:id#reference_line".to_string())
        .expect("valid test fixture");
    let circle =
        SketchEntityId::mint("synthetic:test:id#circle".to_string()).expect("valid test fixture");
    let geometry = BTreeMap::from([
        (
            point.clone(),
            SketchGeometry::native(
                cadmpeg_ir::products::NonEmptyString::new("point")
                    .expect("nonempty source identity"),
            ),
        ),
        (
            bounded.clone(),
            SketchGeometry::native(
                cadmpeg_ir::products::NonEmptyString::new("bounded_curve")
                    .expect("nonempty source identity"),
            ),
        ),
        (
            line.clone(),
            SketchGeometry::native(
                cadmpeg_ir::products::NonEmptyString::new("line")
                    .expect("nonempty source identity"),
            ),
        ),
        (
            reference_line.clone(),
            SketchGeometry::native(
                cadmpeg_ir::products::NonEmptyString::new("reference_line")
                    .expect("nonempty source identity"),
            ),
        ),
        (
            circle.clone(),
            SketchGeometry::native(
                cadmpeg_ir::products::NonEmptyString::new("circle")
                    .expect("nonempty source identity"),
            ),
        ),
    ]);
    let compatible = SketchConstraintDefinitionInput::CoincidentLoci {
        loci: vec![
            SketchLocus::Entity(point),
            SketchLocus::Start(bounded),
            SketchLocus::Center(circle.clone()),
        ],
    };
    assert!(sketch_constraint_loci_compatible(&compatible, &geometry));
    let incompatible = SketchConstraintDefinitionInput::CoincidentLoci {
        loci: vec![SketchLocus::Start(line), SketchLocus::Start(circle)],
    };
    assert!(!sketch_constraint_loci_compatible(&incompatible, &geometry));
    let centered_midpoint = SketchConstraintDefinitionInput::Midpoint {
        point: SketchLocus::Center(
            SketchEntityId::mint("synthetic:test:id#line".to_string()).expect("valid test fixture"),
        ),
        entity: SketchEntityId::mint("synthetic:test:id#bounded".to_string())
            .expect("valid test fixture"),
    };
    assert!(sketch_constraint_loci_compatible(
        &centered_midpoint,
        &geometry
    ));
    let incompatible_midpoint = SketchConstraintDefinitionInput::Midpoint {
        point: SketchLocus::Center(reference_line),
        entity: SketchEntityId::mint("synthetic:test:id#bounded".to_string())
            .expect("valid test fixture"),
    };
    assert!(!sketch_constraint_loci_compatible(
        &incompatible_midpoint,
        &geometry
    ));
}

#[test]
fn incidence_family_lattice_narrows_endpoint_evidence() {
    let mut line = BTreeSet::from([
        SectionEntityIncidenceFamily::BoundedCurve,
        SectionEntityIncidenceFamily::Line,
    ]);
    normalize_section_incidence_curve_family_evidence(&mut line);
    assert_eq!(line, BTreeSet::from([SectionEntityIncidenceFamily::Line]));

    let mut arc = BTreeSet::from([
        SectionEntityIncidenceFamily::BoundedCurve,
        SectionEntityIncidenceFamily::Circular,
    ]);
    normalize_section_incidence_curve_family_evidence(&mut arc);
    assert_eq!(arc, BTreeSet::from([SectionEntityIncidenceFamily::Arc]));

    let mut conflicting = BTreeSet::from([
        SectionEntityIncidenceFamily::Line,
        SectionEntityIncidenceFamily::Circular,
    ]);
    normalize_section_incidence_curve_family_evidence(&mut conflicting);
    assert_eq!(conflicting.len(), 2);
}
