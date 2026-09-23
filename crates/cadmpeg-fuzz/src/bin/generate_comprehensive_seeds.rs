// SPDX-License-Identifier: Apache-2.0
//! Writes deep topology, NURBS, and format-variant container seeds for
//! SolidWorks, CATIA, Creo, and NX. Existing F3D seeds remain unchanged.

use std::fs;

use cadmpeg_fuzz::seed_paths::seed_dir;
use cadmpeg_fuzz::seeds;

type SeedError = Box<dyn std::error::Error>;

fn main() -> Result<(), SeedError> {
    generate_f3d_seeds();
    generate_sldprt_seeds()?;
    generate_catia_seeds()?;
    generate_creo_seeds()?;
    generate_nx_seeds()?;
    println!("All comprehensive seeds generated.");
    Ok(())
}

// ============================================================================
// F3D seeds
// ============================================================================

fn generate_f3d_seeds() {
    println!("f3d seeds already comprehensive, skipping regeneration");
}

// ============================================================================
// SLDPRT seeds
// ============================================================================

fn generate_sldprt_seeds() -> Result<(), SeedError> {
    let dir = seed_dir("seeds/sldprt_container");
    fs::create_dir_all(&dir)?;

    let seeds: Vec<(&str, Vec<u8>)> = vec![
        ("empty", vec![]),
        ("just_header", seeds::sldprt::outer_header()),
        ("synthetic_sldprt", seeds::sldprt::synthetic_sldprt()?),
        (
            "triangle_body",
            seeds::sldprt::sldprt_with_body(&seeds::sldprt::triangle_body())?,
        ),
        (
            "triangle_overlapping_point",
            seeds::sldprt::sldprt_with_body(&sldprt::triangle_body_with_overlapping_point())?,
        ),
        (
            "closed_cylinder",
            seeds::sldprt::sldprt_with_body(&seeds::sldprt::closed_cylinder_body())?,
        ),
        (
            "with_material",
            sldprt::sldprt_with_body_and_material(
                &seeds::sldprt::triangle_body(),
                "Steel",
                [32, 64, 128],
            )?,
        ),
        (
            "with_display_list",
            sldprt::sldprt_with_body_and_display_list(&seeds::sldprt::triangle_body())?,
        ),
        (
            "partition_and_deltas",
            sldprt::sldprt_with_partition_and_deltas(&seeds::sldprt::triangle_body())?,
        ),
        (
            "sheet_body",
            seeds::sldprt::sldprt_with_body(&sldprt::sheet_body())?,
        ),
        (
            "two_owned_triangles",
            seeds::sldprt::sldprt_with_body(&sldprt::two_owned_triangles())?,
        ),
        (
            "with_nurbs_curve",
            seeds::sldprt::sldprt_with_body(&sldprt::triangle_with_nurbs_curve())?,
        ),
        (
            "with_nurbs_surface",
            seeds::sldprt::sldprt_with_body(&sldprt::triangle_with_nurbs_surface())?,
        ),
        (
            "face_on_untyped_surface",
            seeds::sldprt::sldprt_with_body(&sldprt::face_on_untyped_surface())?,
        ),
        (
            "with_line_curve",
            seeds::sldprt::sldprt_with_body(&sldprt::triangle_with_line_curve())?,
        ),
    ];

    for (name, data) in seeds {
        fs::write(dir.join(name), &data)?;
        println!("  sldprt/{} ({} bytes)", name, data.len());
    }
    Ok(())
}

mod sldprt {
    use cadmpeg_fuzz::seeds::sldprt::{
        be16, be32, bef64, bridge, coedge, edge_use, loop_head, make_block, outer_header,
        parasolid_with_body, plane_carrier, sldprt_with_body, triangle_body, vertex_use,
        world_point,
    };

    pub(super) fn sldprt_with_body_and_material(
        body: &[u8],
        name: &str,
        rgb: [u8; 3],
    ) -> std::io::Result<Vec<u8>> {
        let mut f = sldprt_with_body(body)?;
        let mut material = b"moVisualProperties_c".to_vec();
        material.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 0]);
        material.extend_from_slice(&0u32.to_le_bytes());
        material.extend_from_slice(&0x00c0c0c0u32.to_le_bytes());
        material.extend_from_slice(&[0xff, 0xfe, 0xff, 0x00]);
        let units = name.encode_utf16().collect::<Vec<_>>();
        let unit_count = u8::try_from(units.len())
            .map_err(|_| std::io::Error::other("material name exceeds 255 UTF-16 units"))?;
        material.extend_from_slice(&[0xff, 0xfe, 0xff, unit_count]);
        for unit in units {
            material.extend_from_slice(&unit.to_le_bytes());
        }
        f.extend(make_block(0x40, "SWObjects", &material)?);
        Ok(f)
    }

    fn display_list_payload() -> Vec<u8> {
        fn descriptor(item_size: u32, kind: u32, count: u32, data: &[u8]) -> Vec<u8> {
            let mut b = Vec::new();
            b.extend_from_slice(&item_size.to_le_bytes());
            b.extend_from_slice(&kind.to_le_bytes());
            b.extend_from_slice(&2u32.to_le_bytes());
            b.extend_from_slice(&count.to_le_bytes());
            b.extend_from_slice(data);
            b
        }
        let mut b = b"uoTempBodyTessData_c".to_vec();
        b.extend_from_slice(&[0u8; 8]);
        b.extend_from_slice(b"uoTempFaceTessData_c");
        b.extend_from_slice(&[0u8; 8]);
        b.extend(descriptor(4, 8, 1, &3u32.to_le_bytes()));
        let mut positions = Vec::new();
        for value in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
            positions.extend_from_slice(&value.to_le_bytes());
        }
        b.extend(descriptor(12, 100, 3, &positions));
        b.extend(descriptor(12, 100, 3, &[0u8; 36]));
        b.extend(descriptor(4, 8, 0, &[]));
        b.extend(descriptor(4, 8, 1, &4u32.to_le_bytes()));
        b.extend(descriptor(1, 8, 0, &[]));
        b
    }

    pub(super) fn sldprt_with_body_and_display_list(body: &[u8]) -> std::io::Result<Vec<u8>> {
        let mut f = sldprt_with_body(body)?;
        f.extend(make_block(
            0x41,
            "Contents/DisplayLists",
            &display_list_payload(),
        )?);
        Ok(f)
    }

    pub(super) fn sldprt_with_partition_and_deltas(partition: &[u8]) -> std::io::Result<Vec<u8>> {
        let mut f = outer_header();
        f.extend_from_slice(&make_block(
            0x20,
            "Contents/Config-0-Partition",
            &parasolid_with_body("partition body", "SCH_SW_33103_11000", partition),
        )?);
        f.extend_from_slice(&make_block(
            0x21,
            "Contents/Config-0-Deltas",
            &parasolid_with_body("deltas body", "SCH_SW_33103_11000", &[]),
        )?);
        Ok(f)
    }

    fn line_carrier(attr: u16, point: [f64; 3], dir: [f64; 3]) -> Vec<u8> {
        let mut b = vec![0x00, 0x1e];
        be16(&mut b, attr);
        be32(&mut b, 0);
        for _ in 0..5 {
            be16(&mut b, 0);
        }
        b.push(0x2b);
        for v in point.into_iter().chain(dir) {
            bef64(&mut b, v);
        }
        b
    }

    fn bridge_owned(attr: u16, loop_attr: u16, surface_attr: u16, owner: u16) -> Vec<u8> {
        let mut b = bridge(attr, loop_attr, surface_attr);
        b[8..10].copy_from_slice(&owner.to_be_bytes());
        b
    }

    fn entity51(flags: u32, attr: u16, disc: u16, slots: &[u16]) -> Vec<u8> {
        let mut b = vec![0x00, 0x51];
        be32(&mut b, flags);
        be16(&mut b, attr);
        be32(&mut b, 1);
        be16(&mut b, disc);
        for slot in slots {
            be16(&mut b, *slot);
        }
        b
    }

    pub(super) fn triangle_body_with_overlapping_point() -> Vec<u8> {
        let mut b = Vec::new();
        b.extend(plane_carrier(
            100,
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0],
        ));
        let mut face_bridge = bridge(10, 20, 100);
        face_bridge.splice(31..31, world_point(60, [0.0, 0.0, 0.0]));
        b.extend(face_bridge);
        b.extend(loop_head(20, 30, 10));
        b.extend(coedge(30, 20, 31, 50, 0, 40, false));
        b.extend(coedge(31, 20, 32, 51, 0, 41, false));
        b.extend(coedge(32, 20, 30, 52, 0, 42, false));
        b.extend(edge_use(40, 0));
        b.extend(edge_use(41, 0));
        b.extend(edge_use(42, 0));
        b.extend(vertex_use(50, 60));
        b.extend(vertex_use(51, 61));
        b.extend(vertex_use(52, 62));
        b.extend(world_point(61, [1.0, 0.0, 0.0]));
        b.extend(world_point(62, [0.0, 1.0, 0.0]));
        b
    }

    pub(super) fn sheet_body() -> Vec<u8> {
        let mut body = Vec::new();
        body.extend(entity51(2, 500, 0x0017, &[510, 700, 0, 0, 0, 0]));
        body.extend(entity51(2, 501, 0x0017, &[511, 701, 0, 0, 0, 0]));
        body.extend(entity51(1, 510, 0x001b, &[700, 0, 0, 0, 0, 0]));
        body.extend(entity51(1, 511, 0x001d, &[701, 0, 0, 0, 0, 0]));

        let mut tri1 = Vec::new();
        tri1.extend(plane_carrier(
            100,
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0],
        ));
        tri1.extend(bridge_owned(10, 20, 100, 700));
        tri1.extend(loop_head(20, 30, 10));
        tri1.extend(coedge(30, 20, 31, 50, 0, 40, false));
        tri1.extend(coedge(31, 20, 32, 51, 0, 41, false));
        tri1.extend(coedge(32, 20, 30, 52, 0, 42, false));
        tri1.extend(edge_use(40, 0));
        tri1.extend(edge_use(41, 0));
        tri1.extend(edge_use(42, 0));
        tri1.extend(vertex_use(50, 60));
        tri1.extend(vertex_use(51, 61));
        tri1.extend(vertex_use(52, 62));
        tri1.extend(world_point(60, [0.0, 0.0, 0.0]));
        tri1.extend(world_point(61, [1.0, 0.0, 0.0]));
        tri1.extend(world_point(62, [0.0, 1.0, 0.0]));
        body.extend(tri1);

        let mut tri2 = Vec::new();
        tri2.extend(plane_carrier(
            200,
            [10.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0],
        ));
        tri2.extend(bridge_owned(210, 220, 200, 701));
        tri2.extend(loop_head(220, 230, 210));
        tri2.extend(coedge(230, 220, 231, 250, 0, 240, false));
        tri2.extend(coedge(231, 220, 232, 251, 0, 241, false));
        tri2.extend(coedge(232, 220, 230, 252, 0, 242, false));
        tri2.extend(edge_use(240, 0));
        tri2.extend(edge_use(241, 0));
        tri2.extend(edge_use(242, 0));
        tri2.extend(vertex_use(250, 260));
        tri2.extend(vertex_use(251, 261));
        tri2.extend(vertex_use(252, 262));
        tri2.extend(world_point(260, [10.0, 0.0, 0.0]));
        tri2.extend(world_point(261, [11.0, 0.0, 0.0]));
        tri2.extend(world_point(262, [10.0, 1.0, 0.0]));
        body.extend(tri2);

        body
    }

    pub(super) fn two_owned_triangles() -> Vec<u8> {
        let mut body = Vec::new();
        body.extend(entity51(2, 500, 0x0017, &[700, 0, 0, 0, 0, 0]));
        body.extend(entity51(2, 501, 0x0017, &[701, 0, 0, 0, 0, 0]));

        let mut tri1 = Vec::new();
        tri1.extend(plane_carrier(
            100,
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0],
        ));
        tri1.extend(bridge_owned(10, 20, 100, 700));
        tri1.extend(loop_head(20, 30, 10));
        tri1.extend(coedge(30, 20, 31, 50, 0, 40, false));
        tri1.extend(coedge(31, 20, 32, 51, 0, 41, false));
        tri1.extend(coedge(32, 20, 30, 52, 0, 42, false));
        tri1.extend(edge_use(40, 0));
        tri1.extend(edge_use(41, 0));
        tri1.extend(edge_use(42, 0));
        tri1.extend(vertex_use(50, 60));
        tri1.extend(vertex_use(51, 61));
        tri1.extend(vertex_use(52, 62));
        tri1.extend(world_point(60, [0.0, 0.0, 0.0]));
        tri1.extend(world_point(61, [1.0, 0.0, 0.0]));
        tri1.extend(world_point(62, [0.0, 1.0, 0.0]));
        body.extend(tri1);

        let mut tri2 = Vec::new();
        tri2.extend(plane_carrier(
            300,
            [10.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0],
        ));
        tri2.extend(bridge_owned(310, 320, 300, 701));
        tri2.extend(loop_head(320, 330, 310));
        tri2.extend(coedge(330, 320, 331, 350, 0, 340, false));
        tri2.extend(coedge(331, 320, 332, 351, 0, 341, false));
        tri2.extend(coedge(332, 320, 330, 352, 0, 342, false));
        tri2.extend(edge_use(340, 0));
        tri2.extend(edge_use(341, 0));
        tri2.extend(edge_use(342, 0));
        tri2.extend(vertex_use(350, 360));
        tri2.extend(vertex_use(351, 361));
        tri2.extend(vertex_use(352, 362));
        tri2.extend(world_point(360, [10.0, 0.0, 0.0]));
        tri2.extend(world_point(361, [11.0, 0.0, 0.0]));
        tri2.extend(world_point(362, [10.0, 1.0, 0.0]));
        body.extend(tri2);

        body
    }

    fn f64_array(tag: u8, attr: u16, values: &[f64]) -> Vec<u8> {
        let mut b = vec![0x00, tag, 0x2b];
        be32(&mut b, values.len() as u32);
        be16(&mut b, attr);
        for value in values {
            bef64(&mut b, *value);
        }
        b
    }

    fn u16_array(attr: u16, values: &[u16]) -> Vec<u8> {
        let mut b = vec![0x00, 0x7f, 0x2b];
        be32(&mut b, values.len() as u32);
        be16(&mut b, attr);
        for value in values {
            be16(&mut b, *value);
        }
        b
    }

    pub(super) fn triangle_with_nurbs_curve() -> Vec<u8> {
        let mut body = triangle_body();

        let wrapper_attr = 170u16;
        let descriptor_attr = 171u16;
        let control_attr = descriptor_attr + 1;
        let mult_attr = descriptor_attr + 2;
        let knot_attr = descriptor_attr + 3;

        let mut b = vec![0x00, 0x86];
        be16(&mut b, wrapper_attr);
        be16(&mut b, descriptor_attr);
        b.extend_from_slice(&[0u8; 8]);
        b.extend_from_slice(&[0x00, 0x88]);
        be16(&mut b, descriptor_attr);
        be16(&mut b, 2);
        be32(&mut b, 3);
        be16(&mut b, 3);
        be32(&mut b, 2);
        b.push(0);
        be32(&mut b, 0);
        be16(&mut b, control_attr);
        be16(&mut b, mult_attr);
        be16(&mut b, knot_attr);
        b.extend(f64_array(
            0x2d,
            control_attr,
            &[0.0, 0.0, 0.0, 0.5, 1.0, 0.0, 1.0, 0.0, 0.0],
        ));
        b.extend(u16_array(mult_attr, &[3, 3]));
        b.extend(f64_array(0x80, knot_attr, &[0.0, 1.0]));
        body.extend(b);

        // The body this function just built carries the edge tag, so the
        // patch applies to every body reaching here.
        if let Some(edge) = body.windows(2).position(|w| w == [0x00, 0x10]) {
            body[edge + 24..edge + 26].copy_from_slice(&170u16.to_be_bytes());
        }

        body
    }

    pub(super) fn triangle_with_nurbs_surface() -> Vec<u8> {
        let mut body = triangle_body();

        let wrapper_attr = 180u16;
        let descriptor_attr = 181u16;
        let bridge_attr = 10u16;
        let control_attr = descriptor_attr + 1;
        let u_mult_attr = descriptor_attr + 2;
        let v_mult_attr = descriptor_attr + 3;
        let u_knot_attr = descriptor_attr + 4;
        let v_knot_attr = descriptor_attr + 5;

        let mut b = vec![0x00, 0x7c];
        be16(&mut b, wrapper_attr);
        be32(&mut b, 1);
        for reference in [0, bridge_attr, 0, 0, 0] {
            be16(&mut b, reference);
        }
        b.push(0x2b);
        be16(&mut b, descriptor_attr);
        be16(&mut b, 0);
        b.extend_from_slice(&[0x00, 0x7e]);
        be16(&mut b, descriptor_attr);
        b.extend_from_slice(&[0u8; 12]);
        for reference in [
            control_attr,
            u_mult_attr,
            v_mult_attr,
            u_knot_attr,
            v_knot_attr,
        ] {
            be16(&mut b, reference);
        }
        b.extend(f64_array(
            0x2d,
            control_attr,
            &[0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.5],
        ));
        b.extend(u16_array(u_mult_attr, &[2, 2]));
        b.extend(u16_array(v_mult_attr, &[2, 2]));
        b.extend(f64_array(0x80, u_knot_attr, &[0.0, 1.0]));
        b.extend(f64_array(0x80, v_knot_attr, &[0.0, 1.0]));
        body.extend(b);

        // The body this function just built carries the bridge tag, so the
        // patch applies to every body reaching here.
        if let Some(bridge) = body.windows(2).position(|w| w == [0x00, 0x0e]) {
            body[bridge + 26..bridge + 28].copy_from_slice(&180u16.to_be_bytes());
        }

        body
    }

    pub(super) fn face_on_untyped_surface() -> Vec<u8> {
        let mut body = Vec::new();
        body.extend(bridge(10, 20, 999));
        body.extend(loop_head(20, 30, 10));
        body.extend(coedge(30, 20, 31, 50, 0, 40, false));
        body.extend(coedge(31, 20, 32, 51, 0, 41, false));
        body.extend(coedge(32, 20, 30, 52, 0, 42, false));
        body.extend(edge_use(40, 0));
        body.extend(edge_use(41, 0));
        body.extend(edge_use(42, 0));
        body.extend(vertex_use(50, 60));
        body.extend(vertex_use(51, 61));
        body.extend(vertex_use(52, 62));
        body.extend(world_point(60, [0.0, 0.0, 0.0]));
        body.extend(world_point(61, [1.0, 0.0, 0.0]));
        body.extend(world_point(62, [0.0, 1.0, 0.0]));
        body
    }

    pub(super) fn triangle_with_line_curve() -> Vec<u8> {
        let mut body = triangle_body();
        body.extend(line_carrier(70, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]));
        // The body this function just built carries the edge tag, so the
        // patch applies to every body reaching here.
        if let Some(edge) = body.windows(2).position(|w| w == [0x00, 0x10]) {
            body[edge + 24..edge + 26].copy_from_slice(&70u16.to_be_bytes());
        }
        body
    }

    #[cfg(test)]
    mod tests {
        use super::sldprt_with_body_and_material;
        use cadmpeg_container::compression::inflate_bounded_probe;

        #[test]
        fn material_name_length_counts_utf16_units_and_refuses_overflow() {
            let file =
                sldprt_with_body_and_material(&[], "é", [0, 0, 0]).expect("short material name");
            let marker = [0x9e, 0x14, 0x01, 0x00];
            let at = super::sldprt_with_body(&[])
                .expect("base SLDPRT seed")
                .len();
            assert_eq!(&file[at..at + marker.len()], marker);
            let name_len = cadmpeg_core::decode::View::u32_le_at(&file, at + 20)
                .expect("material block name length") as usize;
            let payload_len = cadmpeg_core::decode::View::u32_le_at(&file, at + 16)
                .expect("material block payload length");
            let payload_len = usize::try_from(payload_len).expect("host payload length");
            let material = inflate_bounded_probe(&file[at + 24 + name_len..], payload_len)
                .expect("material block");
            assert!(material
                .windows(6)
                .any(|bytes| bytes == [0xff, 0xfe, 0xff, 1, 0xe9, 0]));
            assert!(sldprt_with_body_and_material(&[], &"x".repeat(256), [0, 0, 0]).is_err());
        }
    }
}

// ============================================================================
// CATIA seeds - comprehensive
// ============================================================================

fn generate_catia_seeds() -> Result<(), SeedError> {
    let dir = seed_dir("seeds/catia_container");
    fs::create_dir_all(&dir)?;

    let seeds: Vec<(&str, Vec<u8>)> = vec![
        ("empty", vec![]),
        ("just_magic", seeds::catia::outer_magic()),
        ("zero_entity", seeds::catia::zero_entity_catpart()),
        (
            "zero_entity_cylinder",
            catia::zero_entity_cylinder_catpart(),
        ),
        ("zero_entity_nurbs", catia::zero_entity_nurbs_catpart()),
        ("standard_nested", seeds::catia::standard_catpart()?),
        ("e5_circle", catia::e5_catpart()?),
    ];

    for (name, data) in seeds {
        fs::write(dir.join(name), &data)?;
        println!("  catia/{} ({} bytes)", name, data.len());
    }
    Ok(())
}

mod catia {
    use cadmpeg_fuzz::seeds::catia::{be32, descriptor, DIR_MAGIC, OUTER_MAGIC};
    fn le_f64(v: f64) -> [u8; 8] {
        v.to_le_bytes()
    }
    pub(super) fn zero_entity_cylinder_catpart() -> Vec<u8> {
        let mut f = Vec::new();
        f.extend_from_slice(OUTER_MAGIC);
        f.extend_from_slice(&be32(0));
        f.extend_from_slice(&be32(0));
        f.extend_from_slice(&[0xa9, 0x03, 0x28, 0x8a]);
        let mut payload = vec![0u8; 146];
        let write = |payload: &mut [u8], at: usize, value: f64| {
            payload[at..at + 8].copy_from_slice(&le_f64(value));
        };
        for (at, value) in [
            (8, 1.0),
            (16, 2.0),
            (24, 3.0),
            (33, 1.0),
            (65, 1.0),
            (81, 4.0),
        ] {
            write(&mut payload, at, value);
        }
        f.extend_from_slice(&payload);
        f
    }

    pub(super) fn zero_entity_nurbs_catpart() -> Vec<u8> {
        let mut f = vec![0u8; 16];
        f[..8].copy_from_slice(OUTER_MAGIC);
        let record = f.len();
        f.extend_from_slice(&[0xa9, 0x03, 0x34, 0xc8]);
        f.extend_from_slice(&[0u8; 300]);
        let write_f64 = |f: &mut [u8], at: usize, value: f64| {
            f[record + at..record + at + 8].copy_from_slice(&le_f64(value));
        };
        let write_token = |f: &mut [u8], at: usize, value: u32| {
            f[record + at] = 0x10;
            f[record + at + 1..record + at + 5].copy_from_slice(&value.to_le_bytes());
        };
        write_f64(&mut f, 23, 0.0);
        write_f64(&mut f, 31, 1.0);
        write_token(&mut f, 39, 3);
        write_token(&mut f, 44, 3);
        write_f64(&mut f, 50, 0.0);
        write_f64(&mut f, 58, 1.0);
        write_token(&mut f, 66, 3);
        write_token(&mut f, 71, 3);
        for i in 0..9 {
            let at = 79 + i * 24;
            write_f64(&mut f, at, i as f64);
            write_f64(&mut f, at + 8, (i / 3) as f64);
            write_f64(&mut f, at + 16, (i % 3) as f64);
        }
        f
    }

    fn e5_circle_stream() -> Vec<u8> {
        let mut record = vec![0u8; 113];
        record[..3].copy_from_slice(&[0xe5, 0x0d, 0x03]);
        record[3] = 0xc9;
        record[5..7].copy_from_slice(&100u16.to_le_bytes());
        let write = |record: &mut [u8], at: usize, value: f64| {
            record[at..at + 8].copy_from_slice(&le_f64(value));
        };
        for (at, value) in [
            (14, 10.0),
            (22, 20.0),
            (30, 30.0),
            (38, 1.0),
            (70, 1.0),
            (86, 2.5),
        ] {
            write(&mut record, at, value);
        }
        record
    }

    pub(super) fn e5_catpart() -> std::io::Result<Vec<u8>> {
        let main = e5_circle_stream();
        let surf = vec![0u8];
        let main_off = 16u32;
        let surf_off = main_off + main.len() as u32;
        let dir_rel = surf_off + surf.len() as u32;
        let mut dir = Vec::new();
        dir.extend_from_slice(DIR_MAGIC);
        dir.extend_from_slice(&descriptor("MainDataStream", main_off, main.len() as u32)?);
        dir.extend_from_slice(&descriptor("SurfacicReps", surf_off, surf.len() as u32)?);
        dir.extend_from_slice(b"CB__END");
        let mut inner = Vec::new();
        inner.extend_from_slice(OUTER_MAGIC);
        inner.extend_from_slice(&be32(dir_rel));
        inner.extend_from_slice(&be32(dir.len() as u32));
        inner.extend_from_slice(&main);
        inner.extend_from_slice(&surf);
        inner.extend_from_slice(&dir);
        let mut file = Vec::new();
        file.extend_from_slice(OUTER_MAGIC);
        file.extend_from_slice(&be32(16 + inner.len() as u32));
        file.extend_from_slice(&be32(0));
        file.extend_from_slice(&inner);
        Ok(file)
    }
}

// ============================================================================
// CREO seeds - comprehensive
// ============================================================================

fn generate_creo_seeds() -> Result<(), SeedError> {
    let dir = seed_dir("seeds/creo_container");
    fs::create_dir_all(&dir)?;

    let seeds: Vec<(&str, Vec<u8>)> = vec![
        ("empty", vec![]),
        ("just_magic", seeds::creo::just_magic()),
        ("minimal_prt", seeds::creo::minimal_prt()),
        ("with_visibgeom", seeds::creo::with_visibgeom()),
        ("nd_layout", creo::nd_layout()),
        ("depdb_layout", creo::depdb_layout()),
        ("with_surface_rows", creo::with_surface_rows()),
        ("with_curve_prototypes", creo::with_curve_prototypes()),
    ];

    for (name, data) in seeds {
        fs::write(dir.join(name), &data)?;
        println!("  creo/{} ({} bytes)", name, data.len());
    }
    Ok(())
}

mod creo {
    use cadmpeg_fuzz::seeds::creo::{build_prt, visibgeom_payload};
    pub(super) fn nd_layout() -> Vec<u8> {
        build_prt("c", &[("ND:0:VisibGeom:1", visibgeom_payload(3, 4))])
    }
    pub(super) fn depdb_layout() -> Vec<u8> {
        build_prt(
            "c",
            &[("VisibGeom", vec![0x00]), ("DEPDB_DATA", vec![0x00, 0x01])],
        )
    }

    pub(super) fn with_surface_rows() -> Vec<u8> {
        let mut payload = visibgeom_payload(2, 0);
        payload.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 8]);
        payload.extend_from_slice(&[8, 0x24, 4, 0xf6, 0x01, 0]);
        build_prt("c", &[("VisibGeom", payload)])
    }

    pub(super) fn with_curve_prototypes() -> Vec<u8> {
        let mut payload = visibgeom_payload(0, 1);
        payload.extend_from_slice(b"crv_array\0crv_id\0\x07type\0\x08feat_id\0\x04");
        build_prt("c", &[("VisibGeom", payload)])
    }
}

// ============================================================================
// NX seeds - comprehensive
// ============================================================================

fn generate_nx_seeds() -> Result<(), SeedError> {
    let dir = seed_dir("seeds/nx_container");
    fs::create_dir_all(&dir)?;

    let seeds: Vec<(&str, Vec<u8>)> = vec![
        ("empty", vec![]),
        ("just_magic", seeds::nx::just_magic()),
        ("single_part", seeds::nx::single_part_prt()?),
        ("assembly", seeds::nx::assembly_prt()?),
        ("topology_part", nx::topology_part_prt()?),
        ("bspline_part", nx::bspline_part_prt()?),
    ];

    for (name, data) in seeds {
        fs::write(dir.join(name), &data)?;
        println!("  nx/{} ({} bytes)", name, data.len());
    }
    Ok(())
}

mod nx {
    use cadmpeg_core::CodecError;
    use cadmpeg_fuzz::seeds::nx::{
        put_f64, put_ref, put_vec3, record, single_part_prt_with_partition,
    };

    /// A connected sheet: body, shell, one face on a plane, one loop with a
    /// one-fin ring, one edge on a line, one vertex on a point, and the
    /// shell's region. Face, edge and vertex carry their tolerances.
    fn topology_partition_stream() -> Result<Vec<u8>, CodecError> {
        let mut s = Vec::new();
        s.extend_from_slice(b"PS\x00\x00");
        s.extend_from_slice(
            b"XX: TRANSMIT FILE (partition) created by modeller\x00SCH_TEST_1_9999\x00",
        );

        let mut body = record(12, 24)?;
        put_ref(&mut body, 2, 2);
        s.extend_from_slice(&body);

        let mut shell = record(13, 24)?;
        put_ref(&mut shell, 2, 3);
        put_ref(&mut shell, 8, 1);
        put_ref(&mut shell, 10, 2);
        put_ref(&mut shell, 12, 1);
        put_ref(&mut shell, 14, 4);
        put_ref(&mut shell, 16, 1);
        put_ref(&mut shell, 18, 1);
        put_ref(&mut shell, 20, 12);
        put_ref(&mut shell, 22, 1);
        s.extend_from_slice(&shell);

        let mut face = record(14, 39)?;
        put_ref(&mut face, 2, 4);
        put_f64(&mut face, 10, 0.000_2);
        put_ref(&mut face, 18, 1);
        put_ref(&mut face, 20, 1);
        put_ref(&mut face, 22, 5);
        put_ref(&mut face, 24, 3);
        put_ref(&mut face, 26, 6);
        face[28] = b'+';
        s.extend_from_slice(&face);

        let mut loop_ = record(15, 16)?;
        put_ref(&mut loop_, 2, 5);
        put_ref(&mut loop_, 10, 7);
        put_ref(&mut loop_, 12, 4);
        put_ref(&mut loop_, 14, 1);
        s.extend_from_slice(&loop_);

        let mut fin = record(17, 23)?;
        put_ref(&mut fin, 2, 7);
        put_ref(&mut fin, 6, 5);
        put_ref(&mut fin, 8, 7);
        put_ref(&mut fin, 10, 7);
        put_ref(&mut fin, 12, 10);
        put_ref(&mut fin, 14, 1);
        put_ref(&mut fin, 16, 8);
        put_ref(&mut fin, 18, 9);
        fin[22] = b'+';
        s.extend_from_slice(&fin);

        let mut edge = record(16, 32)?;
        put_ref(&mut edge, 2, 8);
        put_f64(&mut edge, 10, 0.000_3);
        put_ref(&mut edge, 18, 7);
        put_ref(&mut edge, 24, 9);
        s.extend_from_slice(&edge);

        let mut plane = record(50, 91)?;
        put_ref(&mut plane, 2, 6);
        plane[18] = b'+';
        put_vec3(&mut plane, 19, [0.0, 0.0, 0.0]);
        put_vec3(&mut plane, 43, [0.0, 0.0, 1.0]);
        put_vec3(&mut plane, 67, [1.0, 0.0, 0.0]);
        s.extend_from_slice(&plane);

        let mut line = record(30, 67)?;
        put_ref(&mut line, 2, 9);
        line[18] = b'+';
        put_vec3(&mut line, 19, [0.0, 0.0, 0.0]);
        put_vec3(&mut line, 43, [1.0, 0.0, 0.0]);
        s.extend_from_slice(&line);

        let mut vertex = record(18, 28)?;
        put_ref(&mut vertex, 2, 10);
        put_ref(&mut vertex, 16, 11);
        put_f64(&mut vertex, 18, 0.000_1);
        s.extend_from_slice(&vertex);

        let mut region = record(19, 16)?;
        put_ref(&mut region, 2, 12);
        s.extend_from_slice(&region);

        let mut point = record(29, 40)?;
        put_ref(&mut point, 2, 11);
        put_vec3(&mut point, 16, [0.01, 0.02, 0.03]);
        s.extend_from_slice(&point);

        Ok(s)
    }

    fn bspline_partition_stream() -> Result<Vec<u8>, CodecError> {
        let mut s = Vec::new();
        s.extend_from_slice(b"PS\x00\x00XX: TRANSMIT FILE (partition)\x00SCH_TEST_1_9999\x00");
        let mut surface = record(124, 23)?;
        put_ref(&mut surface, 2, 10);
        surface[18] = b'+';
        put_ref(&mut surface, 19, 20);
        put_ref(&mut surface, 21, 21);
        s.extend(surface);

        let mut descriptor = record(126, 48)?;
        put_ref(&mut descriptor, 2, 20);
        put_ref(&mut descriptor, 6, 1);
        put_ref(&mut descriptor, 8, 1);
        put_ref(&mut descriptor, 12, 2);
        put_ref(&mut descriptor, 16, 2);
        descriptor[18] = 5;
        descriptor[19] = 5;
        descriptor[20..24].copy_from_slice(&2u32.to_be_bytes());
        descriptor[24..28].copy_from_slice(&2u32.to_be_bytes());
        put_ref(&mut descriptor, 36, 30);
        put_ref(&mut descriptor, 38, 31);
        put_ref(&mut descriptor, 40, 32);
        put_ref(&mut descriptor, 42, 33);
        put_ref(&mut descriptor, 44, 125);
        put_ref(&mut descriptor, 46, 21);
        s.extend(descriptor);

        let mut data = record(125, 97 + 12 * 8)?;
        put_ref(&mut data, 2, 21);
        data[90] = b'+';
        data[91..95].copy_from_slice(&12u32.to_be_bytes());
        for (index, value) in [
            0.0, 0.0, 0.0, 0.0, 0.02, 0.0, 0.01, 0.0, 0.0, 0.01, 0.02, 0.0,
        ]
        .into_iter()
        .enumerate()
        {
            put_f64(&mut data, 97 + index * 8, value);
        }
        s.extend(data);

        for (tag, reference, values) in [(127, 30, vec![2u16, 2]), (127, 31, vec![2, 2])] {
            let mut array = record(tag, 8 + values.len() * 2)?;
            array[4..6].copy_from_slice(&(values.len() as u16).to_be_bytes());
            put_ref(&mut array, 6, reference);
            for (index, value) in values.into_iter().enumerate() {
                put_ref(&mut array, 8 + index * 2, value);
            }
            s.extend(array);
        }
        for reference in [32, 33] {
            let mut array = record(128, 8 + 2 * 8)?;
            array[4..6].copy_from_slice(&2u16.to_be_bytes());
            put_ref(&mut array, 6, reference);
            put_f64(&mut array, 8, 0.0);
            put_f64(&mut array, 16, 1.0);
            s.extend(array);
        }

        let mut curve = record(134, 23)?;
        put_ref(&mut curve, 2, 50);
        curve[18] = b'+';
        put_ref(&mut curve, 19, 40);
        put_ref(&mut curve, 21, 41);
        s.extend(curve);
        let mut curve_descriptor = record(136, 27)?;
        put_ref(&mut curve_descriptor, 2, 40);
        put_ref(&mut curve_descriptor, 4, 1);
        put_ref(&mut curve_descriptor, 8, 2);
        put_ref(&mut curve_descriptor, 10, 3);
        put_ref(&mut curve_descriptor, 14, 2);
        curve_descriptor[16] = 5;
        put_ref(&mut curve_descriptor, 23, 42);
        put_ref(&mut curve_descriptor, 25, 43);
        s.extend(curve_descriptor);
        let mut curve_data = record(135, 15 + 6 * 8)?;
        put_ref(&mut curve_data, 2, 41);
        curve_data[9..13].copy_from_slice(&6u32.to_be_bytes());
        for (index, value) in [0.0, 0.0, 0.0, 0.02, 0.0, 0.0].into_iter().enumerate() {
            put_f64(&mut curve_data, 15 + index * 8, value);
        }
        s.extend(curve_data);
        for (tag, reference) in [(127, 42), (128, 43)] {
            let mut array = record(tag, if tag == 127 { 12 } else { 24 })?;
            array[4..6].copy_from_slice(&2u16.to_be_bytes());
            put_ref(&mut array, 6, reference);
            if tag == 127 {
                put_ref(&mut array, 8, 2);
                put_ref(&mut array, 10, 2);
            } else {
                put_f64(&mut array, 8, 0.0);
                put_f64(&mut array, 16, 1.0);
            }
            s.extend(array);
        }
        Ok(s)
    }

    pub(super) fn topology_part_prt() -> Result<Vec<u8>, CodecError> {
        single_part_prt_with_partition(&topology_partition_stream()?)
    }
    pub(super) fn bspline_part_prt() -> Result<Vec<u8>, CodecError> {
        single_part_prt_with_partition(&bspline_partition_stream()?)
    }

    #[cfg(test)]
    mod tests {
        use std::io::Cursor;

        use cadmpeg_codec_nx::NxCodec;
        use cadmpeg_core::decode::InspectOptions;
        use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
        use cadmpeg_ir::geometry::{
            CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
        };

        /// Inspect and decode one seed. The container must catalogue the
        /// `/Root/UG_PART/UG_PART` file entry and inflate exactly one
        /// Parasolid partition stream from it.
        fn decode_single_partition(bytes: &[u8]) -> DecodeResult {
            let summary = NxCodec
                .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
                .expect("the container passes its directory stage");
            let part = summary
                .entries
                .iter()
                .find(|entry| entry.name == "/Root/UG_PART/UG_PART")
                .expect("the HEADER directory catalogues the part payload");
            assert!(part.attributes.contains_key("file_offset"));
            let partitions = summary
                .entries
                .iter()
                .filter(|entry| {
                    entry.name.starts_with("parasolid#")
                        && entry.attributes.get("kind").map(String::as_str) == Some("partition")
                })
                .count();
            assert_eq!(partitions, 1);
            NxCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .expect("the partition decodes")
        }

        #[test]
        fn single_part_seed_decodes_its_analytic_carriers() {
            let bytes = cadmpeg_fuzz::seeds::nx::single_part_prt().expect("single_part seed");
            let result = decode_single_partition(&bytes);
            let model = &result.ir().model;
            assert_eq!(model.points.len(), 1);
            let surface_count = |kind: fn(&SolvedSurfaceGeometry) -> bool| {
                model
                    .surfaces
                    .iter()
                    .filter(|surface| {
                        matches!(&surface.geometry, SurfaceGeometry::Solved(solved) if kind(solved))
                    })
                    .count()
            };
            assert_eq!(
                surface_count(|s| matches!(s, SolvedSurfaceGeometry::Plane(_))),
                1
            );
            assert_eq!(
                surface_count(|s| matches!(s, SolvedSurfaceGeometry::Cylinder(_))),
                1
            );
            assert_eq!(
                model
                    .curves
                    .iter()
                    .filter(|curve| matches!(
                        curve.geometry,
                        CurveGeometry::Solved(SolvedCurveGeometry::Line(_))
                    ))
                    .count(),
                1
            );
        }

        #[test]
        fn topology_part_seed_decodes_its_connected_sheet() {
            let bytes = super::topology_part_prt().expect("topology_part seed");
            let result = decode_single_partition(&bytes);
            let model = &result.ir().model;
            assert_eq!(model.bodies.len(), 1);
            assert_eq!(model.regions.len(), 1);
            assert_eq!(model.shells.len(), 1);
            assert_eq!(model.faces.len(), 1);
            assert_eq!(model.loops.len(), 1);
            assert_eq!(model.coedges.len(), 1);
            assert_eq!(model.edges.len(), 1);
            assert_eq!(model.vertices.len(), 1);
        }

        #[test]
        fn assembly_seed_decodes_its_external_references() {
            const ENTRY: &str = "/Root/UG_PART/ExternalReferences";
            let bytes = cadmpeg_fuzz::seeds::nx::assembly_prt().expect("assembly seed");
            let summary = NxCodec
                .inspect(&mut Cursor::new(&bytes), &InspectOptions::default())
                .expect("the container passes its directory stage");
            let entry = summary
                .entries
                .iter()
                .find(|entry| entry.name == ENTRY)
                .expect("the HEADER directory catalogues the external references");
            assert!(entry.attributes.contains_key("file_offset"));
            assert!(!summary
                .entries
                .iter()
                .any(|entry| entry.name.starts_with("parasolid#")));

            let result = NxCodec
                .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
                .expect("the assembly decodes");
            let nx = result
                .ir()
                .native
                .namespace("nx")
                .expect("the decode emits the nx namespace");
            let arena = |name: &str| nx.arenas().get(name).map_or(0, Vec::len);
            let references = nx
                .arenas()
                .get("external_references")
                .expect("the string table decodes");
            let text = |record: &cadmpeg_ir::native::NativeRecord, field: &str| {
                record
                    .field(field)
                    .and_then(|value| value.as_str().map(str::to_owned))
            };
            assert_eq!(
                references
                    .iter()
                    .map(|record| text(record, "path"))
                    .collect::<Vec<_>>(),
                ["child.prt", "dirA", "dirB", "extra"].map(|path| Some(path.to_owned()))
            );
            assert!(references
                .iter()
                .all(|record| text(record, "source_entry").as_deref() == Some(ENTRY)));
            assert_eq!(arena("external_reference_indexed_records"), 2);
            assert_eq!(arena("external_reference_empty_records"), 1);
            assert_eq!(arena("external_reference_records"), 1);
            assert_eq!(arena("external_reference_record_string_uses"), 4);
            assert_eq!(arena("external_reference_record_children"), 1);
            assert_eq!(arena("external_reference_tail_reference_pairs"), 1);
        }

        #[test]
        fn bspline_part_seed_decodes_its_nurbs_surface_and_curve() {
            let bytes = super::bspline_part_prt().expect("bspline_part seed");
            let result = decode_single_partition(&bytes);
            let model = &result.ir().model;
            assert!(model.surfaces.iter().any(|surface| matches!(
                surface.geometry,
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(_))
            )));
            assert!(model.curves.iter().any(|curve| matches!(
                curve.geometry,
                CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(_))
            )));
        }
    }
}
