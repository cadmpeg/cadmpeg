// SPDX-License-Identifier: Apache-2.0

#[test]
fn resolved_visible_body_roster_keeps_pool_surfaces_as_scoped_supports() {
    use super::*;
    use cadmpeg_ir::geometry::{analytic::PlaneSurface, Surface};
    use cadmpeg_ir::ids::SurfaceId;
    use cadmpeg_ir::math::Vector3;
    use cadmpeg_ir::SourceGeometryRole;

    let bytes = [0xb2, 0x13, 0x5b, 0, 5, 0xb2, 0x13, 0x5b, 0, 5];
    let sources = vec![
        vec![SourceExtent::within(&bytes, 0, 5).expect("selected source")],
        vec![SourceExtent::within(&bytes, 5, 10).expect("other source")],
    ];
    let records = crate::wire::records::consolidated_records_in_sources(&bytes, &sources)
        .expect("body records");
    let geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("plane"),
    ));
    for (ends, role) in [
        (None, None),
        (Some(vec![1]), Some(SourceGeometryRole::Support)),
        (Some(vec![2]), None),
    ] {
        let mut ir = CadIr::empty();
        for index in 0..3 {
            ir.geometry.surfaces.push(Surface {
                id: SurfaceId::mint(format!("catia:test:surface#{index}")).expect("surface id"),
                geometry: geometry.clone(),
                native_ref: None,
                source_object: Some(
                    crate::assemble::cgm_source_key("surface", index).expect("source identity"),
                ),
            });
        }
        let selection =
            ends.map(
                |allocation_ends| crate::container::body_selection::BodySelection {
                    source: 0,
                    allocation_ends,
                },
            );
        crate::test_support::with_service_context(|ctx| {
            append(
                &mut ir,
                &mut AnnotationBuilder::new(),
                &Stream {
                    data: &bytes,
                    sources: &sources,
                    records: &records,
                    selection: selection.as_ref(),
                },
                &BTreeMap::from([(0, 0), (4, 1), (5, 2)]),
                &mut crate::nurbs::LaneRefusals::new(),
                &mut FamilyEntityAdmission::new(ctx),
            )
        })
        .expect("body selection");
        assert_eq!(ir.geometry.surfaces.len(), 3);
        for (index, surface) in ir.geometry.surfaces.iter().enumerate() {
            assert_eq!(surface.geometry, geometry);
            assert_eq!(
                surface
                    .source_object
                    .as_ref()
                    .expect("source")
                    .geometry_role,
                if index < 2 { role } else { None }
            );
        }
    }
}

#[test]
fn sheet_rolling_bridge_admits_first_middle_control_one() {
    let mut bytes = vec![0x85, 0x15, 0x11, 0x0d, 0x09, 0x05, 0x11, 0x05];
    bytes.extend(1.0_f64.to_le_bytes());
    bytes.extend([0x01, 0x05]);
    bytes.extend([0; 8]);
    bytes.extend([0x01, 0x05]);
    assert_eq!(
        super::bridge_radius(&bytes, true).map(|(_, radius)| radius),
        Some(1.0)
    );
    assert!(super::bridge_radius(&bytes, false).is_none());
    bytes[16] = 0x03;
    bytes[17] = 0x01;
    assert!(super::bridge_radius(&bytes, true).is_none());
}
