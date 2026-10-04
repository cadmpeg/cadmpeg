// SPDX-License-Identifier: Apache-2.0
//! Recursive BREP geometry nesting contracts.

use super::{
    in_decode_context, BinaryCursor, FinitePoint3, FiniteReal, FiniteVector3, ShapePayload,
    ShapePayloadRecord, TextCurve, TextCurve2d, TextSurface, TokenCursor,
};

/// `wrappers` offset surface records over one plane leaf, as text tokens.
fn text_offset_surface_tokens(wrappers: usize) -> Vec<String> {
    let mut tokens = Vec::new();
    for _ in 0..wrappers {
        tokens.push("11".to_owned());
        tokens.push("1.0".to_owned());
    }
    tokens.push("1".to_owned());
    for token in [
        "0.0", "0.0", "0.0", "0.0", "0.0", "1.0", "1.0", "0.0", "0.0", "0.0", "1.0", "0.0",
    ] {
        tokens.push(token.to_owned());
    }
    tokens
}

/// `wrappers` offset surface records over one plane leaf, as binary bytes.
fn binary_offset_surface_bytes(wrappers: usize) -> Vec<u8> {
    let mut bytes = Vec::new();
    for _ in 0..wrappers {
        bytes.push(11);
        bytes.extend_from_slice(&1.0_f64.to_le_bytes());
    }
    bytes.push(1);
    bytes.extend_from_slice(&[0; 96]);
    bytes
}

/// `wrappers` offset parameter-curve records over one line leaf.
fn binary_offset_curve2d_bytes(wrappers: usize) -> Vec<u8> {
    let mut bytes = Vec::new();
    for _ in 0..wrappers {
        bytes.push(9);
        bytes.extend_from_slice(&1.0_f64.to_le_bytes());
    }
    bytes.push(1);
    bytes.extend_from_slice(&[0; 32]);
    bytes
}

fn text_offset_surface_wrappers(surface: &TextSurface) -> usize {
    match surface {
        TextSurface::Offset { basis, .. } => 1 + text_offset_surface_wrappers(basis.surface()),
        _ => 0,
    }
}

fn binary_offset_curve2d_wrappers(curve: &TextCurve2d) -> usize {
    match curve {
        TextCurve2d::Offset { basis, .. } => 1 + binary_offset_curve2d_wrappers(basis.curve()),
        _ => 0,
    }
}

/// One extrusion surface over `wrappers` trimmed curve records and a line
/// leaf: `wrappers + 2` records that cross the surface/curve boundary.
fn text_extrusion_tokens(wrappers: usize) -> Vec<String> {
    let mut tokens = vec![
        "6".to_owned(),
        "0.0".to_owned(),
        "0.0".to_owned(),
        "1.0".to_owned(),
    ];
    for _ in 0..wrappers {
        for token in ["8", "0.0", "1.0"] {
            tokens.push(token.to_owned());
        }
    }
    for token in ["1", "0.0", "0.0", "0.0", "0.0", "0.0", "1.0"] {
        tokens.push(token.to_owned());
    }
    tokens
}

/// The binary form of `text_extrusion_tokens`.
fn binary_extrusion_bytes(wrappers: usize) -> Vec<u8> {
    let mut bytes = vec![6];
    bytes.extend_from_slice(&[0; 24]);
    for _ in 0..wrappers {
        bytes.push(8);
        bytes.extend_from_slice(&0.0_f64.to_le_bytes());
        bytes.extend_from_slice(&1.0_f64.to_le_bytes());
    }
    bytes.push(1);
    bytes.extend_from_slice(&[0; 48]);
    bytes
}

#[test]
fn a_directrix_spends_the_same_budget_as_its_surface_on_both_routes() {
    in_decode_context(|ctx| {
        // The extrusion record and the line leaf are two of the records.
        let admitted = super::super::MAX_GEOMETRY_NESTING_DEPTH - 1;

        let tokens = text_extrusion_tokens(admitted);
        let tokens: Vec<&str> = tokens.iter().map(String::as_str).collect();
        let mut cursor = TokenCursor::new(ctx, &tokens);
        super::super::parse_surface(&mut cursor, 0, 1).expect("the budget is admitted");
        assert!(cursor.is_empty());

        let tokens = text_extrusion_tokens(admitted + 1);
        let tokens: Vec<&str> = tokens.iter().map(String::as_str).collect();
        let error = super::super::parse_surface(&mut TokenCursor::new(ctx, &tokens), 0, 1)
            .expect_err("one record past the budget is refused");
        assert!(
            error
                .to_string()
                .contains("text B-rep 3D curve nesting exceeds 64"),
            "{error}"
        );

        let bytes = binary_extrusion_bytes(admitted);
        let mut cursor = BinaryCursor::new(ctx, &bytes);
        super::super::parse_binary_surface(&mut cursor, 0).expect("the budget is admitted");
        assert_eq!(cursor.remaining(), 0);

        let bytes = binary_extrusion_bytes(admitted + 1);
        let error = super::super::parse_binary_surface(&mut BinaryCursor::new(ctx, &bytes), 0)
            .expect_err("one record past the budget is refused");
        assert!(
            error
                .to_string()
                .contains("binary 3D curve nesting exceeds 64"),
            "{error}"
        );
    });
}

#[test]
fn the_text_surface_parser_admits_the_budget_and_refuses_one_record_past_it() {
    in_decode_context(|ctx| {
        let admitted = text_offset_surface_tokens(super::super::MAX_GEOMETRY_NESTING_DEPTH);
        let tokens: Vec<&str> = admitted.iter().map(String::as_str).collect();
        let mut cursor = TokenCursor::new(ctx, &tokens);
        let surface =
            super::super::parse_surface(&mut cursor, 0, 1).expect("the budget is admitted");
        assert_eq!(
            text_offset_surface_wrappers(&surface),
            super::super::MAX_GEOMETRY_NESTING_DEPTH
        );
        assert!(cursor.is_empty());

        let refused = text_offset_surface_tokens(super::super::MAX_GEOMETRY_NESTING_DEPTH + 1);
        let tokens: Vec<&str> = refused.iter().map(String::as_str).collect();
        let error = super::super::parse_surface(&mut TokenCursor::new(ctx, &tokens), 0, 1)
            .expect_err("one record past the budget is refused");
        assert!(
            error
                .to_string()
                .contains("text B-rep surface nesting exceeds 64"),
            "{error}"
        );
    });
}

#[test]
fn the_binary_surface_parser_admits_the_budget_and_refuses_one_record_past_it() {
    in_decode_context(|ctx| {
        let admitted = binary_offset_surface_bytes(super::super::MAX_GEOMETRY_NESTING_DEPTH);
        let mut cursor = BinaryCursor::new(ctx, &admitted);
        let surface =
            super::super::parse_binary_surface(&mut cursor, 0).expect("the budget is admitted");
        assert_eq!(
            text_offset_surface_wrappers(&surface),
            super::super::MAX_GEOMETRY_NESTING_DEPTH
        );
        assert_eq!(cursor.remaining(), 0);

        let refused = binary_offset_surface_bytes(super::super::MAX_GEOMETRY_NESTING_DEPTH + 1);
        let error = super::super::parse_binary_surface(&mut BinaryCursor::new(ctx, &refused), 0)
            .expect_err("one record past the budget is refused");
        assert!(
            error
                .to_string()
                .contains("binary surface nesting exceeds 64"),
            "{error}"
        );
    });
}

#[test]
fn the_binary_parameter_curve_parser_admits_the_budget_and_refuses_one_record_past_it() {
    in_decode_context(|ctx| {
        let admitted = binary_offset_curve2d_bytes(super::super::MAX_GEOMETRY_NESTING_DEPTH);
        let mut cursor = BinaryCursor::new(ctx, &admitted);
        let curve =
            super::super::parse_binary_curve2d(&mut cursor, 0).expect("the budget is admitted");
        assert_eq!(
            binary_offset_curve2d_wrappers(&curve),
            super::super::MAX_GEOMETRY_NESTING_DEPTH
        );
        assert_eq!(cursor.remaining(), 0);

        let refused = binary_offset_curve2d_bytes(super::super::MAX_GEOMETRY_NESTING_DEPTH + 1);
        let error = super::super::parse_binary_curve2d(&mut BinaryCursor::new(ctx, &refused), 0)
            .expect_err("one record past the budget is refused");
        assert!(
            error
                .to_string()
                .contains("binary parameter-curve nesting exceeds 64"),
            "{error}"
        );
    });
}

/// `wrappers` offset surface records over one plane leaf, built through the
/// carrier.
fn nested_offset_surface(wrappers: usize) -> Result<TextSurface, String> {
    let mut surface = TextSurface::Plane {
        origin: FinitePoint3::ZERO,
        axis: FiniteVector3::new(cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)).unwrap(),
        u_axis: FiniteVector3::new(cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0)).unwrap(),
        v_reversed: false,
    };
    for _ in 0..wrappers {
        surface = TextSurface::Offset {
            distance: FiniteReal::ONE,
            basis: super::super::NestedSurface::try_new(surface)?,
        };
    }
    Ok(surface)
}

/// `wrappers` trimmed 3D curve records over one line leaf.
fn nested_trimmed_curve(wrappers: usize) -> Result<TextCurve, String> {
    let mut curve = TextCurve::Line {
        origin: FinitePoint3::ZERO,
        direction: FiniteVector3::new(cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)).unwrap(),
    };
    for _ in 0..wrappers {
        curve = TextCurve::Trimmed {
            parameter_range: [FiniteReal::ZERO, FiniteReal::ONE],
            basis: super::super::NestedCurve::try_new(curve)?,
        };
    }
    Ok(curve)
}

#[test]
fn the_surface_carrier_admits_the_bound_and_refuses_one_record_past_it() {
    let admitted = nested_offset_surface(super::super::MAX_GEOMETRY_NESTING_DEPTH)
        .expect("the bound is admitted");
    assert_eq!(
        text_offset_surface_wrappers(&admitted),
        super::super::MAX_GEOMETRY_NESTING_DEPTH
    );
    let error = super::super::NestedSurface::try_new(admitted)
        .expect_err("one record past the bound is refused");
    assert_eq!(error, "surface nesting exceeds 64");
}

#[test]
fn a_directrix_and_its_surface_share_one_nesting_count() {
    let curve = nested_trimmed_curve(super::super::MAX_GEOMETRY_NESTING_DEPTH - 1)
        .expect("the bound is admitted");
    let surface = TextSurface::Extrusion {
        direction: FiniteVector3::new(cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)).unwrap(),
        directrix: super::super::NestedCurve::try_new(curve)
            .expect("the directrix is the last admitted record"),
    };
    assert!(matches!(surface, TextSurface::Extrusion { .. }));

    let curve = nested_trimmed_curve(super::super::MAX_GEOMETRY_NESTING_DEPTH)
        .expect("the bound is admitted");
    let error = super::super::NestedCurve::try_new(curve)
        .expect_err("a directrix over the whole count leaves no record for the surface");
    assert_eq!(error, "3D curve nesting exceeds 64");
}

#[test]
fn a_cadir_document_cannot_carry_a_surface_chain_past_the_bound() {
    let facts = super::super::ShapeSet {
        locations: Vec::new(),
        curve2ds: Vec::new(),
        curves: Vec::new(),
        polygons3d: Vec::new(),
        polygons_on_triangulations: Vec::new(),
        surfaces: vec![
            nested_offset_surface(super::super::MAX_GEOMETRY_NESTING_DEPTH)
                .expect("the bound is admitted"),
        ],
        triangulations: Vec::new(),
        tshapes: Vec::new().into(),
        roots: Vec::new(),
    };
    let record = ShapePayloadRecord {
        id: "fcstd:native:shape_payload#1".to_owned(),
        property: "fcstd:native:property#1".to_owned(),
        entry: "Shape.brp".to_owned(),
        payload: ShapePayload::Binary {
            version: super::super::BinaryTopologyVersion::V4,
            facts,
        },
    };

    let admitted = serde_json::to_value(&record).expect("a retained payload serializes");
    assert_eq!(
        serde_json::from_value::<ShapePayloadRecord>(admitted.clone())
            .expect("the bound round-trips"),
        record
    );

    let mut refused = admitted;
    let basis = refused["binary"]["surfaces"][0].take();
    refused["binary"]["surfaces"][0] = serde_json::json!({
        "kind": "offset",
        "distance": 1.0,
        "basis": basis,
    });
    let error = serde_json::from_value::<ShapePayloadRecord>(refused)
        .expect_err("one record past the bound is refused");
    assert!(
        error.to_string().contains("surface nesting exceeds 64"),
        "{error}"
    );
}
