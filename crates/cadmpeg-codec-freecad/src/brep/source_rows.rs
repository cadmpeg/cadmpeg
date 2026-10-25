// SPDX-License-Identifier: Apache-2.0
//! One-pass semantic decoders for individual shape-set rows.

use super::*;

pub(super) fn text_location(
    cursor: &mut TokenCursor<'_>,
    index: usize,
    locations: &[TextLocation],
) -> Result<TextLocation, CodecError> {
    let kind = cursor.integer("location type")?;
    let location = match kind {
        1 => {
            let mut rows = [[FiniteReal::ZERO; 4]; 3];
            for row in &mut rows {
                for value in row {
                    *value = cursor.finite_real("location transform value")?;
                }
            }
            let transform = Transform::from_finite_rows(rows);
            invert_affine(transform)?;
            TextLocation {
                factors: Vec::new(),
                transform,
            }
        }
        2 => {
            let mut factors = Vec::new();
            let mut transform = Transform::identity();
            loop {
                let referenced = cursor.integer("location factor index")?;
                if referenced == 0 {
                    break;
                }
                let referenced = usize::try_from(referenced)
                    .map_err(|_| CodecError::Malformed("negative location factor index".into()))?;
                if referenced == 0 || referenced > locations.len() {
                    return Err(CodecError::malformed(format_args!(
                        "location {} references unavailable location {referenced}",
                        index + 1
                    )));
                }
                if factors.len() >= 1_000_000 {
                    return Err(CodecError::Malformed(
                        "location factor-count limit exceeded".into(),
                    ));
                }
                let power = cursor.integer("location factor power")?;
                let powered = transform_power(locations[referenced - 1].transform, power)?;
                transform = powered
                    .compose(transform)
                    .map_err(location_transform_error)?;
                factors.push(LocationFactor {
                    location: referenced,
                    power,
                });
            }
            TextLocation { factors, transform }
        }
        other => {
            return Err(CodecError::malformed(format_args!(
                "invalid location type {other} at table index {}",
                index + 1
            )));
        }
    };
    Ok(location)
}

pub(super) fn text_polygon3d(cursor: &mut TokenCursor<'_>) -> Result<TextPolygon3d, CodecError> {
    cursor.mark_census(source::census::Lane::Polygon3d)?;
    let node_count = cursor.count("3D polygon node count", 1_000_000)?;
    let has_parameters = cursor.boolean("3D polygon parameter flag")?;
    let deflection = cursor.finite_real("3D polygon deflection")?;
    // Each node consumes its three point tokens.
    let mut nodes = Vec::new();
    for _ in 0..node_count {
        nodes.push(cursor.finite_point("3D polygon node")?);
    }
    let parameters = if has_parameters {
        // Each parameter consumes its one token.
        let mut parameters = Vec::new();
        for _ in 0..node_count {
            parameters.push(cursor.finite_real("3D polygon parameter")?);
        }
        Some(parameters)
    } else {
        None
    };
    Ok(TextPolygon3d {
        deflection: admit_polygon_deflection(deflection)?,
        nodes,
        parameters,
    })
}

pub(super) fn text_indexed_polygon(
    cursor: &mut TokenCursor<'_>,
) -> Result<TextPolygonOnTriangulation, CodecError> {
    cursor.mark_census(source::census::Lane::IndexedPolygon)?;
    let node_count = cursor.count("polygon-on-triangulation node count", 1_000_000)?;
    // Each node index consumes its one token.
    let mut nodes = Vec::new();
    for _ in 0..node_count {
        let node = cursor.count(
            "polygon-on-triangulation node index",
            usize::try_from(u32::MAX).map_err(|_| {
                CodecError::Malformed("B-rep node limit exceeds address space".into())
            })?,
        )?;
        if node == 0 {
            return Err(CodecError::Malformed(
                "polygon-on-triangulation node index is zero".into(),
            ));
        }
        nodes.push(
            u32::try_from(node)
                .map_err(|_| CodecError::Malformed("B-rep integer exceeds u32".into()))?,
        );
    }
    if cursor.next("polygon-on-triangulation parameter marker")? != "p" {
        return Err(CodecError::Malformed(
            "polygon-on-triangulation has no parameter marker".into(),
        ));
    }
    let deflection = cursor.finite_real("polygon-on-triangulation deflection")?;
    let has_parameters = cursor.boolean("polygon-on-triangulation parameter flag")?;
    let parameters = if has_parameters {
        // Each parameter consumes its one token.
        let mut parameters = Vec::new();
        for _ in 0..node_count {
            parameters.push(cursor.finite_real("polygon-on-triangulation parameter")?);
        }
        Some(parameters)
    } else {
        None
    };
    Ok(TextPolygonOnTriangulation {
        nodes,
        deflection: admit_polygon_deflection(deflection)?,
        parameters,
    })
}

pub(super) fn text_triangulation(
    cursor: &mut TokenCursor<'_>,
    topology_version: u8,
) -> Result<TextTriangulation, CodecError> {
    cursor.mark_census(source::census::Lane::Triangulation)?;
    let node_count = cursor.count("triangulation node count", 1_000_000)?;
    let triangle_count = cursor.count("triangulation triangle count", 1_000_000)?;
    let has_uv = cursor.boolean("triangulation UV flag")?;
    let has_normals = topology_version >= 3 && cursor.boolean("triangulation normal flag")?;
    let deflection = cursor.finite_real("triangulation deflection")?;
    // Each node consumes its three point tokens.
    let mut nodes = Vec::new();
    for _ in 0..node_count {
        nodes.push(cursor.finite_point("triangulation node")?);
    }
    let uv_nodes = if has_uv {
        // Each UV node consumes its two point2 tokens.
        let mut uv_nodes = Vec::new();
        for _ in 0..node_count {
            uv_nodes.push(cursor.finite_point2("triangulation UV node")?);
        }
        Some(uv_nodes)
    } else {
        None
    };
    // Each triangle consumes its three index tokens.
    let mut triangles = Vec::new();
    for _ in 0..triangle_count {
        let mut triangle = [0_u32; 3];
        for node in &mut triangle {
            let index = cursor.count("triangulation node index", node_count)?;
            if index == 0 {
                return Err(CodecError::Malformed(
                    "triangulation node index is zero".into(),
                ));
            }
            *node = u32::try_from(index)
                .map_err(|_| CodecError::Malformed("B-rep integer exceeds u32".into()))?;
        }
        triangles.push(triangle);
    }
    let normals = if has_normals {
        // Each normal consumes its three vector tokens.
        let mut normals = Vec::new();
        for _ in 0..node_count {
            normals.push(cursor.finite_vector("triangulation normal")?);
        }
        Some(normals)
    } else {
        None
    };
    Ok(
        TextTriangulation::from_admitted_parts(deflection, nodes, uv_nodes, triangles, normals)
            .map_err(CodecError::Malformed)?,
    )
}

pub(super) fn binary_location(
    cursor: &mut BinaryCursor<'_>,
    index: usize,
    locations: &[TextLocation],
) -> Result<TextLocation, CodecError> {
    let kind = cursor.u8("binary location kind")?;
    let location = match kind {
        1 => {
            let mut rows = [[FiniteReal::ZERO; 4]; 3];
            for row in &mut rows {
                for value in row {
                    *value = cursor.finite_f64("binary location transform")?;
                }
            }
            let transform = Transform::from_finite_rows(rows);
            invert_affine(transform)?;
            TextLocation {
                factors: Vec::new(),
                transform,
            }
        }
        2 => {
            let mut factors = Vec::new();
            let mut transform = Transform::identity();
            loop {
                if factors.len() >= 1_000_000 {
                    return Err(CodecError::Malformed(
                        "binary location factor-count limit exceeded".into(),
                    ));
                }
                let referenced = cursor.i32("binary location factor")?;
                if referenced == 0 {
                    break;
                }
                let referenced = usize::try_from(referenced)
                    .map_err(|_| CodecError::Malformed("negative binary location factor".into()))?;
                if referenced == 0 || referenced > locations.len() {
                    return Err(CodecError::malformed(format_args!(
                        "binary location {} references unavailable location {referenced}",
                        index + 1
                    )));
                }
                let power = cursor.i32("binary location power")?;
                let powered =
                    transform_power(locations[referenced - 1].transform, i64::from(power))?;
                transform = powered
                    .compose(transform)
                    .map_err(location_transform_error)?;
                factors.push(LocationFactor {
                    location: referenced,
                    power: i64::from(power),
                });
            }
            TextLocation { factors, transform }
        }
        other => {
            return Err(CodecError::malformed(format_args!(
                "invalid binary location type {other}"
            )));
        }
    };
    Ok(location)
}

pub(super) fn binary_polygon3d(cursor: &mut BinaryCursor<'_>) -> Result<TextPolygon3d, CodecError> {
    cursor.mark_census(source::census::Lane::Polygon3d, 4)?;
    let node_count = cursor.count("binary 3D polygon node count")?;
    let has_parameters = cursor.bool("binary 3D polygon parameter flag")?;
    let deflection = cursor.finite_f64("binary 3D polygon deflection")?;
    let nodes = cursor.read_counted(node_count, |cursor| {
        cursor.finite_point3("binary 3D polygon node")
    })?;
    let parameters = has_parameters
        .then(|| {
            cursor.read_counted(node_count, |cursor| {
                cursor.finite_f64("binary 3D polygon parameter")
            })
        })
        .transpose()?;
    Ok(TextPolygon3d {
        deflection: admit_polygon_deflection(deflection)?,
        nodes,
        parameters,
    })
}

pub(super) fn binary_indexed_polygon(
    cursor: &mut BinaryCursor<'_>,
) -> Result<TextPolygonOnTriangulation, CodecError> {
    cursor.mark_census(source::census::Lane::IndexedPolygon, 4)?;
    let node_count = cursor.count("binary indexed polygon node count")?;
    let nodes = cursor.read_counted(node_count, |cursor| {
        let node = cursor.i32("binary indexed polygon node")?;
        u32::try_from(node)
            .map_err(|_| CodecError::Malformed("non-positive binary indexed polygon node".into()))
    })?;
    if nodes.contains(&0) {
        return Err(CodecError::Malformed(
            "binary indexed polygon node indices are one-based".into(),
        ));
    }
    let deflection = cursor.finite_f64("binary indexed polygon deflection")?;
    let has_parameters = cursor.bool("binary indexed polygon parameter flag")?;
    let parameters = has_parameters
        .then(|| {
            cursor.read_counted(node_count, |cursor| {
                cursor.finite_f64("binary indexed polygon parameter")
            })
        })
        .transpose()?;
    Ok(TextPolygonOnTriangulation {
        nodes,
        deflection: admit_polygon_deflection(deflection)?,
        parameters,
    })
}

pub(super) fn binary_triangulation(
    cursor: &mut BinaryCursor<'_>,
    version: u8,
) -> Result<TextTriangulation, CodecError> {
    cursor.mark_census(source::census::Lane::Triangulation, 4)?;
    let node_count = cursor.count("binary triangulation node count")?;
    let triangle_count = cursor.count("binary triangulation triangle count")?;
    let has_uv = cursor.bool("binary triangulation UV flag")?;
    let has_normals = version >= 4 && cursor.bool("binary triangulation normal flag")?;
    let deflection = cursor.finite_f64("binary triangulation deflection")?;
    let nodes = cursor.read_counted(node_count, |cursor| {
        cursor.finite_point3("binary triangulation node")
    })?;
    let uv_nodes = has_uv
        .then(|| {
            cursor.read_counted(node_count, |cursor| {
                cursor.finite_point2("binary triangulation UV node")
            })
        })
        .transpose()?;
    let triangles = cursor.read_counted(triangle_count, |cursor| {
        let mut triangle = [0_u32; 3];
        for node in &mut triangle {
            let value = cursor.i32("binary triangulation triangle node")?;
            *node = u32::try_from(value)
                .map_err(|_| CodecError::Malformed("negative binary triangle node".into()))?;
        }
        Ok(triangle)
    })?;
    let normals = has_normals
        .then(|| {
            cursor.read_counted(node_count, |cursor| {
                cursor.finite_vector3_f32("binary triangulation normal")
            })
        })
        .transpose()?;
    Ok(
        TextTriangulation::from_admitted_parts(deflection, nodes, uv_nodes, triangles, normals)
            .map_err(CodecError::Malformed)?,
    )
}

pub(super) fn text_tshape(
    cursor: &mut TokenCursor<'_>,
    index: usize,
    count: usize,
    section_counts: &BTreeMap<String, usize>,
    topology_version: u8,
    available_shapes: &BTreeMap<usize, usize>,
) -> Result<TextTShape, CodecError> {
    cursor.mark_census(source::census::Lane::Shape)?;
    let token = cursor.next("TShape kind")?;
    let kind = parse_shape_kind(token)?;
    let geometry = parse_tshape_geometry(kind, cursor, section_counts, topology_version)?;
    let flags_token = cursor.next("TShape flags")?;
    let flags = parse_shape_flags(flags_token, topology_version)?;
    let mut children = Vec::new();
    loop {
        if cursor.peek()? == Some("*") {
            cursor.next("TShape child terminator")?;
            break;
        }
        let child = parse_shape_use(cursor, count, section_counts)?;
        if child.shape >= index {
            return Err(CodecError::malformed(format_args!(
                "TShape {index} references non-prior child {}",
                child.shape
            )));
        }
        if available_shapes.contains_key(&child.shape) {
            children.push(child);
        }
    }
    Ok(TextTShape {
        geometry,
        flags,
        children,
    })
}
