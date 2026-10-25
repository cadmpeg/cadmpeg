// SPDX-License-Identifier: Apache-2.0
//! Each solved and procedural geometry producer owns its original-row inputs.
use super::*;

#[test]
fn recursive_geometry_producers_have_independent_original_source_reads() {
    let xml = "<Document SchemaVersion='4' FileVersion='1'><Objects Count='1'><Object name='Shape' type='Part::Feature'/></Objects><ObjectData Count='1'><Object name='Shape'><Properties Count='1'><Property name='Shape' type='Part::PropertyPartShape'><Part file='Shape.brp'/></Property></Properties></Object></ObjectData></Document>";
    let curves = "8 0 1 1 0 0 0 1 0 0\n9 2 0 0 1 1 0 0 0 1 0 0\n";
    let surfaces = "6 0 0 2 1 0 0 0 1 0 0\n7 0 0 0 0 0 1 1 0 0 0 1 0 0\n10 0 1 2 3 11 4 1 0 0 0 0 0 1 1 0 0 0 1 0\n";
    let header = "CASCADE Topology V3, (c) Open Cascade\nLocations 0\nCurve2ds 0\nCurves 2\n";
    let surface_marker = "Polygon3D 0\nPolygonOnTriangulations 0\nSurfaces 3\n";
    let member =
        format!("{header}{curves}{surface_marker}{surfaces}Triangulations 0\nTShapes 0\n*\n");
    let bytes = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", xml.as_bytes()),
        ("Shape.brp", member.as_bytes()),
    ]);
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let model = &decoded.ir().model;
    assert_eq!(model.curves.len(), 6);
    assert_eq!(model.procedural_curves.len(), 2);
    assert_eq!(model.surfaces.len(), 5);
    assert_eq!(model.procedural_surfaces.len(), 4);
    let layouts = decoded.source_fidelity().record_layout();
    let namespace = decoded.ir().native.namespace("fcstd").unwrap();
    let payloads = crate::brep::records::payloads_untracked(namespace).unwrap();
    assert_eq!(payloads.len(), 1);
    let home = &layouts[&payloads[0].id];
    let curve_range = header.len()..header.len() + curves.len();
    let surface_start = curve_range.end + surface_marker.len();
    let surface_range = surface_start..surface_start + surfaces.len();
    let curve_range =
        u64::try_from(curve_range.start).unwrap()..u64::try_from(curve_range.end).unwrap();
    let surface_range =
        u64::try_from(surface_range.start).unwrap()..u64::try_from(surface_range.end).unwrap();
    let mut records = Vec::new();
    for curve in &model.curves {
        records.push((curve.id.as_str(), curve.native_ref.as_deref(), false));
    }
    for curve in &model.procedural_curves {
        records.push((curve.id.as_str(), curve.native_ref.as_deref(), false));
    }
    for surface in &model.surfaces {
        records.push((surface.id.as_str(), surface.native_ref.as_deref(), true));
    }
    for surface in &model.procedural_surfaces {
        records.push((surface.id.as_str(), surface.native_ref.as_deref(), true));
    }
    for (id, native_ref, surface) in records {
        assert_eq!(native_ref, Some(payloads[0].id.as_str()), "{id}");
        let layout = layouts
            .get(id)
            .unwrap_or_else(|| panic!("{id} has no own producer"));
        assert_eq!(layout.stream(), home.stream(), "{id}");
        let reads = &layout.reads()[home.stream()];
        let expected = if surface {
            &surface_range
        } else {
            &curve_range
        };
        let in_expected = |read: &std::ops::Range<u64>| {
            read.start < home.offset() + expected.end && home.offset() + expected.start < read.end
        };
        let in_surface = |read: &std::ops::Range<u64>| {
            read.start < home.offset() + surface_range.end
                && home.offset() + surface_range.start < read.end
        };
        assert!(
            reads.iter().any(in_expected) || (!surface && reads.iter().any(in_surface)),
            "{id} must obtain its actual curve or directrix through its own getter"
        );
    }
}
