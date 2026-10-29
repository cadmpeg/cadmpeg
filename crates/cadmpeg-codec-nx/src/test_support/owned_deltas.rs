// SPDX-License-Identifier: Apache-2.0
//! Literal BODY witnesses from the independent raw census and documented 27/29 omissions.
//! These fixtures are placed after a constructed proved prefix; they do not certify corpus enclosure.

pub(crate) fn hex(text: &str) -> Vec<u8> {
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

pub(crate) fn body(form: u8) -> (Vec<u8>, Vec<u8>, Vec<(&'static str, usize)>) {
    let (declaration, instance, fields): (&str, &str, &[(&str, usize)]) = match form {
        // Raw stream 2796092eec5ad33a96b3d7a28e71b883bc67d0d57d40347c379391ae69b68414, ordinary BODY at 8890; form 27.
        27 => ("000c1b434343434343434343434343434449056f776e6572041000014343434343434343434110696e6465785f6d61705f6f66667365740000000101644109696e6465785f6d61700052000141116e6f64655f69645f696e6465785f6d6170005200014114736368656d615f656d62656464696e675f6d6170005200015a", "000c0003000071e4040901000601004e01010c01000101000101408f4000000000013e45798ee2308c3a000101000101000101010002010101000901000a01005601000c01000d01005c01000f0100000000000101000101000101", &[("highest_node_id", 4),("attributes_features", 3),("attribute_chains", 3),("surface", 3),("curve", 3),("point", 3),("key", 3),("res_size", 8),("res_linear", 8),("ref_instance", 3),("next", 3),("previous", 3),("state", 1),("owner", 3),("body_type", 1),("nom_geom_state", 1),("shell", 3),("boundary_surface", 3),("boundary_curve", 3),("boundary_point", 3),("region", 3),("edge", 3),("vertex", 3),("index_map_offset", 4),("index_map", 3),("node_id_index_map", 3),("schema_embedding_map", 3)]),
        // Raw stream 2796092eec5ad33a96b3d7a28e71b883bc67d0d57d40347c379391ae69b68414, ordinary BODY at 8890; form 29.
        29 => ("000c1d434343434343434343434343434449056f776e6572041000014343434343434343434110696e6465785f6d61705f6f66667365740000000101644109696e6465785f6d61700052000141116e6f64655f69645f696e6465785f6d6170005200014114736368656d615f656d62656464696e675f6d61700052000141056368696c64000c0001410e6c6f776573745f6e6f64655f69640000000101645a", "000c0003000071e4040901000601004e01010c01000101000101408f4000000000013e45798ee2308c3a000101000101000101010002010101000901000a01005601000c01000d01005c01000f010000000000010100010100010100010100000000", &[("highest_node_id", 4),("attributes_features", 3),("attribute_chains", 3),("surface", 3),("curve", 3),("point", 3),("key", 3),("res_size", 8),("res_linear", 8),("ref_instance", 3),("next", 3),("previous", 3),("state", 1),("owner", 3),("body_type", 1),("nom_geom_state", 1),("shell", 3),("boundary_surface", 3),("boundary_curve", 3),("boundary_point", 3),("region", 3),("edge", 3),("vertex", 3),("index_map_offset", 4),("index_map", 3),("node_id_index_map", 3),("schema_embedding_map", 3),("child", 3),("lowest_node_id", 4)]),
        // Raw stream 2796092eec5ad33a96b3d7a28e71b883bc67d0d57d40347c379391ae69b68414, ordinary BODY at 8890; form 34.
        34 => ("000c2243434343434349046d65736803ee00014908706f6c796c696e6503f00001434343434343434449056f776e657204100001434343434343490d626f756e646172795f6d65736803ee00014911626f756e646172795f706f6c796c696e6503f000014343434110696e6465785f6d61705f6f66667365740000000101644109696e6465785f6d61700052000141116e6f64655f69645f696e6465785f6d6170005200014114736368656d615f656d62656464696e675f6d61700052000141056368696c64000c0001410e6c6f776573745f6e6f64655f696400000001016441106d6573685f6f66667365745f6461746100ce00015a", "000c0003000071e4040901000601004e01010c01000101000101000101000101408f4000000000013e45798ee2308c3a000101000101000101010002010101000901000a01005601000c01000101000101000d01005c01000f010000000000010100010100010100010100000000000101", &[("highest_node_id", 4),("attributes_features", 3),("attribute_chains", 3),("surface", 3),("curve", 3),("point", 3),("mesh", 3),("polyline", 3),("key", 3),("res_size", 8),("res_linear", 8),("ref_instance", 3),("next", 3),("previous", 3),("state", 1),("owner", 3),("body_type", 1),("nom_geom_state", 1),("shell", 3),("boundary_surface", 3),("boundary_curve", 3),("boundary_point", 3),("boundary_mesh", 3),("boundary_polyline", 3),("region", 3),("edge", 3),("vertex", 3),("index_map_offset", 4),("index_map", 3),("node_id_index_map", 3),("schema_embedding_map", 3),("child", 3),("lowest_node_id", 4),("mesh_offset_data", 3)]),
        // Raw stream fa221adfbb694b8b49157cbc60fcf1cbe7d7c6915936630d3c830d267432e109, ordinary BODY at 60816; form 35.
        35 => ("000c2343434349076c61747469636500de000143434349046d65736803ee00014908706f6c796c696e6503f00001434343434343434449056f776e657204100001434343434343490d626f756e646172795f6d65736803ee00014911626f756e646172795f706f6c796c696e6503f000014343434110696e6465785f6d61705f6f66667365740000000101644109696e6465785f6d61700052000141116e6f64655f69645f696e6465785f6d6170005200014114736368656d615f656d62656464696e675f6d61700052000141056368696c64000c0001410e6c6f776573745f6e6f64655f696400000001016441106d6573685f6f66667365745f6461746100ce00015a", "000c0003000013d6149d01000701000101000101000101000101000101000101000101408f4000000000013e45798ee2308c3a000101000101000101010002010101000801000901000a0100b401000101000101000c01000d0100ab010000000000010100010100010100010100000000000101", &[("highest_node_id", 4),("attributes_features", 3),("attribute_chains", 3),("lattice", 3),("surface", 3),("curve", 3),("point", 3),("mesh", 3),("polyline", 3),("key", 3),("res_size", 8),("res_linear", 8),("ref_instance", 3),("next", 3),("previous", 3),("state", 1),("owner", 3),("body_type", 1),("nom_geom_state", 1),("shell", 3),("boundary_surface", 3),("boundary_curve", 3),("boundary_point", 3),("boundary_mesh", 3),("boundary_polyline", 3),("region", 3),("edge", 3),("vertex", 3),("index_map_offset", 4),("index_map", 3),("node_id_index_map", 3),("schema_embedding_map", 3),("child", 3),("lowest_node_id", 4),("mesh_offset_data", 3)]),
        // Raw stream 7f55d42b25951befcb5767eb27956124b9393c3adc8ba091da4a0a193a2f3ea0, ordinary BODY at 5686; form 36.
        36 => ("000c2443434349076c61747469636500de000143434349046d65736803ee00014908706f6c796c696e6503f00001434343434343434449056f776e6572041000014343434910626f756e646172795f6c61747469636500de0001434343490d626f756e646172795f6d65736803ee00014911626f756e646172795f706f6c796c696e6503f000014343434110696e6465785f6d61705f6f66667365740000000101644109696e6465785f6d61700052000141116e6f64655f69645f696e6465785f6d6170005200014114736368656d615f656d62656464696e675f6d61700052000141056368696c64000c0001410e6c6f776573745f6e6f64655f696400000001016441106d6573685f6f66667365745f6461746100ce00015a", "000c0003000000e10a6601000601000101000101000101000101000101000101000101408f4000000000013e45798ee2308c3a00010100010100010101000201010100070100010100080106f70114ce01000101000101000b01000c010090010000000000010100010100010100010100000000000101", &[("highest_node_id", 4),("attributes_features", 3),("attribute_chains", 3),("lattice", 3),("surface", 3),("curve", 3),("point", 3),("mesh", 3),("polyline", 3),("key", 3),("res_size", 8),("res_linear", 8),("ref_instance", 3),("next", 3),("previous", 3),("state", 1),("owner", 3),("body_type", 1),("nom_geom_state", 1),("shell", 3),("boundary_lattice", 3),("boundary_surface", 3),("boundary_curve", 3),("boundary_point", 3),("boundary_mesh", 3),("boundary_polyline", 3),("region", 3),("edge", 3),("vertex", 3),("index_map_offset", 4),("index_map", 3),("node_id_index_map", 3),("schema_embedding_map", 3),("child", 3),("lowest_node_id", 4),("mesh_offset_data", 3)]),
        _ => panic!("BODY fixture form"),
    };
    (hex(declaration), hex(instance), fields.to_vec())
}

pub(crate) fn body_stream(form: u8) -> Vec<u8> {
    let (declaration, instance, _) = body(form);
    let mut bytes = super::test_pmarks::prefix(1);
    bytes.extend(declaration);
    bytes.extend_from_slice(&instance[2..]);
    bytes
}

pub(crate) fn region(layout: u8, xmt: u16, region_type: u8) -> Vec<u8> {
    let mut bytes = match layout {
        7 => vec![0, 19, 255],
        8 => b"\0\x13\x08CCCCCCCA\x05owner\0\x0c\0\x01Z".to_vec(),
        9 => b"\0\x13\x09CCCCCCI\x05frame\0\xe6\0\x01CA\x05owner\0\x0c\0\x01Z".to_vec(),
        _ => panic!("REGION fixture layout"),
    };
    bytes.extend(xmt.to_be_bytes());
    bytes.extend(u32::MAX.to_be_bytes());
    for target in [1u16, 3, 1, 12, 8] {
        bytes.extend(target.to_be_bytes());
        bytes.push(1);
    }
    if layout == 9 {
        bytes.extend([0, 89, 0]);
    }
    bytes.push(region_type);
    if layout >= 8 {
        bytes.extend([0, 90, 1]);
    }
    bytes
}

pub(crate) fn stream(bytes: Vec<u8>) -> crate::parasolid::Stream<'static> {
    crate::parasolid::Stream {
        source: None,
        file_offset: 0,
        consumed: u64::try_from(bytes.len()).unwrap(),
        inflated: bytes.into(),
        body: crate::parasolid::StreamBody::Parasolid {
            subtype: crate::parasolid::ParasolidSubtype::Deltas,
            schema: Some(
                cadmpeg_parasolid::OwnedSchemaToken::try_from("SCH_3501171_35102_13006").unwrap(),
            ),
        },
    }
}
