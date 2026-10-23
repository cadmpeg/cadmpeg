// SPDX-License-Identifier: Apache-2.0
//! Cross-codec conformance contracts that drive the codec crates in-process.
//!
//! Both module trees share one harness binary. Tests that only spawn the CLI
//! belong in `cli.rs`.

/// Cross-codec resource-budget classification contracts.
mod budget {
    #[cfg(all(
        feature = "catia",
        feature = "creo",
        feature = "fcstd",
        feature = "nx",
        feature = "sldprt"
    ))]
    use std::io::Cursor;

    #[cfg(all(
        feature = "catia",
        feature = "creo",
        feature = "fcstd",
        feature = "nx",
        feature = "sldprt"
    ))]
    use cadmpeg_codec_catia::CatiaCodec;
    #[cfg(all(
        feature = "catia",
        feature = "creo",
        feature = "fcstd",
        feature = "nx",
        feature = "sldprt"
    ))]
    use cadmpeg_codec_creo::CreoCodec;
    #[cfg(all(
        feature = "catia",
        feature = "creo",
        feature = "fcstd",
        feature = "nx",
        feature = "sldprt"
    ))]
    use cadmpeg_codec_freecad::FcstdCodec;
    #[cfg(all(
        feature = "catia",
        feature = "creo",
        feature = "fcstd",
        feature = "nx",
        feature = "sldprt"
    ))]
    use cadmpeg_codec_nx::NxCodec;
    #[cfg(all(
        feature = "catia",
        feature = "creo",
        feature = "fcstd",
        feature = "nx",
        feature = "sldprt"
    ))]
    use cadmpeg_codec_sldprt::SldprtCodec;
    #[cfg(all(
        feature = "catia",
        feature = "creo",
        feature = "fcstd",
        feature = "nx",
        feature = "sldprt"
    ))]
    use cadmpeg_core::decode::ResourceLimits;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    #[cfg(all(
        feature = "catia",
        feature = "creo",
        feature = "fcstd",
        feature = "nx",
        feature = "sldprt"
    ))]
    use cadmpeg_ir::codec::{Codec, DecodeOptions};

    #[cfg(all(
        feature = "catia",
        feature = "creo",
        feature = "fcstd",
        feature = "nx",
        feature = "sldprt"
    ))]
    const CATIA: &[u8] = include_bytes!("../../cadmpeg-fuzz/seeds/catia_container/standard_nested");
    #[cfg(all(
        feature = "catia",
        feature = "creo",
        feature = "fcstd",
        feature = "nx",
        feature = "sldprt"
    ))]
    const CREO: &[u8] = include_bytes!("../../cadmpeg-fuzz/seeds/creo_container/with_surface_rows");
    #[cfg(all(
        feature = "catia",
        feature = "creo",
        feature = "fcstd",
        feature = "nx",
        feature = "sldprt"
    ))]
    const FREECAD: &[u8] =
        include_bytes!("../../cadmpeg-fuzz/seeds/fcstd_container/core_design_product.FCStd");
    #[cfg(all(
        feature = "catia",
        feature = "creo",
        feature = "fcstd",
        feature = "nx",
        feature = "sldprt"
    ))]
    const NX: &[u8] = include_bytes!("fixtures/nx_topology_part.prt");
    #[cfg(all(
        feature = "catia",
        feature = "creo",
        feature = "fcstd",
        feature = "nx",
        feature = "sldprt"
    ))]
    const SLDPRT: &[u8] = include_bytes!("fixtures/sldprt_triangle_body.sldprt");

    #[cfg(all(
        feature = "catia",
        feature = "creo",
        feature = "fcstd",
        feature = "nx",
        feature = "sldprt"
    ))]
    #[derive(Clone, Copy)]
    enum Starvation {
        Entities,
        RecursionDepth,
        DecompressedBytes,
        RetainedBytes,
    }

    #[cfg(all(
        feature = "catia",
        feature = "creo",
        feature = "fcstd",
        feature = "nx",
        feature = "sldprt"
    ))]
    impl Starvation {
        fn apply(self, limits: &mut ResourceLimits) {
            match self {
                Self::Entities => limits.max_entities = 0,
                Self::RecursionDepth => limits.max_recursion_depth = 0,
                Self::DecompressedBytes => {
                    limits.max_decompressed_bytes_total = 0;
                    limits.max_decompressed_bytes_per_expand = 0;
                }
                Self::RetainedBytes => limits.max_retained_bytes = 0,
            }
        }

        const fn dimension(self) -> ResourceDimension {
            match self {
                Self::Entities => ResourceDimension::Entities,
                Self::RecursionDepth => ResourceDimension::RecursionDepth,
                Self::DecompressedBytes => ResourceDimension::DecompressedBytes,
                Self::RetainedBytes => ResourceDimension::RetainedBytes,
            }
        }
    }

    #[cfg(all(
        feature = "catia",
        feature = "creo",
        feature = "fcstd",
        feature = "nx",
        feature = "sldprt"
    ))]
    struct Case {
        name: &'static str,
        codec: &'static dyn Codec,
        bytes: &'static [u8],
        starvation: Starvation,
    }

    #[cfg(all(
        feature = "catia",
        feature = "creo",
        feature = "fcstd",
        feature = "nx",
        feature = "sldprt"
    ))]
    static CATIA_CODEC: CatiaCodec = CatiaCodec;
    #[cfg(all(
        feature = "catia",
        feature = "creo",
        feature = "fcstd",
        feature = "nx",
        feature = "sldprt"
    ))]
    static CREO_CODEC: CreoCodec = CreoCodec;
    #[cfg(all(
        feature = "catia",
        feature = "creo",
        feature = "fcstd",
        feature = "nx",
        feature = "sldprt"
    ))]
    static FREECAD_CODEC: FcstdCodec = FcstdCodec;
    #[cfg(all(
        feature = "catia",
        feature = "creo",
        feature = "fcstd",
        feature = "nx",
        feature = "sldprt"
    ))]
    static NX_CODEC: NxCodec = NxCodec;
    #[cfg(all(
        feature = "catia",
        feature = "creo",
        feature = "fcstd",
        feature = "nx",
        feature = "sldprt"
    ))]
    static SLDPRT_CODEC: SldprtCodec = SldprtCodec;

    #[cfg(all(
        feature = "catia",
        feature = "creo",
        feature = "fcstd",
        feature = "nx",
        feature = "sldprt"
    ))]
    fn cases() -> [Case; 8] {
        [
            Case {
                name: "catia/entities",
                codec: &CATIA_CODEC,
                bytes: CATIA,
                starvation: Starvation::Entities,
            },
            Case {
                name: "creo/entities",
                codec: &CREO_CODEC,
                bytes: CREO,
                starvation: Starvation::Entities,
            },
            Case {
                name: "freecad/entities",
                codec: &FREECAD_CODEC,
                bytes: FREECAD,
                starvation: Starvation::Entities,
            },
            Case {
                name: "freecad/recursion_depth",
                codec: &FREECAD_CODEC,
                bytes: FREECAD,
                starvation: Starvation::RecursionDepth,
            },
            Case {
                name: "freecad/retained_bytes",
                codec: &FREECAD_CODEC,
                bytes: FREECAD,
                starvation: Starvation::RetainedBytes,
            },
            Case {
                name: "nx/entities",
                codec: &NX_CODEC,
                bytes: NX,
                starvation: Starvation::Entities,
            },
            Case {
                name: "nx/decompressed_bytes",
                codec: &NX_CODEC,
                bytes: NX,
                starvation: Starvation::DecompressedBytes,
            },
            Case {
                name: "sldprt/entities",
                codec: &SLDPRT_CODEC,
                bytes: SLDPRT,
                starvation: Starvation::Entities,
            },
        ]
    }

    #[cfg(all(
        feature = "catia",
        feature = "creo",
        feature = "fcstd",
        feature = "nx",
        feature = "sldprt"
    ))]
    #[test]
    fn migrated_codec_paths_classify_starvation_identically_across_profiles() {
        for case in cases() {
            case.codec
                .decode(&mut Cursor::new(case.bytes), &DecodeOptions::default())
                .unwrap_or_else(|error| panic!("{} baseline failed: {error}", case.name));

            let mut observed = None;
            for mut policy in [DecodePolicy::desktop(), DecodePolicy::service()] {
                case.starvation.apply(&mut policy.limits);
                let error = case
                    .codec
                    .decode(
                        &mut Cursor::new(case.bytes),
                        &DecodeOptions {
                            depth: cadmpeg_core::decode::DecodeDepth::Full,
                            policy,
                        },
                    )
                    .expect_err("starved decode must fail");
                let cadmpeg_ir::DecodeFailure::Codec(CodecError::ResourceLimit(limit)) = error
                else {
                    panic!(
                        "{} returned the wrong error classification: {error}",
                        case.name
                    );
                };
                assert_eq!(
                    limit.dimension,
                    case.starvation.dimension(),
                    "{}",
                    case.name
                );
                assert_eq!(observed.get_or_insert(limit.dimension), &limit.dimension);
            }
        }
    }

    #[test]
    fn a_refusal_permanently_fuses_the_decode_session() {
        for policy in [DecodePolicy::desktop(), DecodePolicy::service()] {
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(b"root", &arena, &policy)
                .expect("context construction");
            let requested = policy.limits.max_entities.saturating_add(1);
            assert!(matches!(
                ctx.charge_entities(requested, "force refusal"),
                Err(CodecError::ResourceLimit(limit))
                    if limit.dimension == ResourceDimension::Entities
            ));
            assert!(matches!(
                ctx.charge_entities(0, "post-refusal charge"),
                Err(CodecError::ResourceLimit(limit))
                    if limit.dimension == ResourceDimension::Entities
            ));
            assert!(matches!(
                ctx.finish_session(),
                Err(CodecError::ResourceLimit(limit))
                    if limit.dimension == ResourceDimension::Entities
            ));
        }
    }
}

/// Sketch profile projection preserves explicit connectivity across decode policies.
#[cfg(feature = "fcstd")]
mod sketch_profiles {
    use std::fmt::Write as _;
    use std::io::{Cursor, Write as _};

    use cadmpeg_codec_freecad::FcstdCodec;
    use cadmpeg_core::decode::{DecodeDepth, DecodePolicy};
    use cadmpeg_ir::codec::{Codec, DecodeOptions};

    const DENSE_SEGMENTS: usize = 4096;

    fn fcstd(document: &str) -> Vec<u8> {
        let mut bytes = Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(&mut bytes);
        zip.start_file(
            "Document.xml",
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored),
        )
        .expect("start entry");
        zip.write_all(document.as_bytes()).expect("write entry");
        zip.finish().expect("finish ZIP");
        bytes.into_inner()
    }

    /// Unconstrained overlapping segments followed by an explicitly closed triangle.
    fn unconstrained_segments_then_constrained_triangle() -> Vec<u8> {
        let dense_count = DENSE_SEGMENTS;
        let dense = r#"<Geometry type="Part::GeomLineSegment"><LineSegment StartX="1" StartY="1" EndX="2" EndY="1"/></Geometry>"#
            .repeat(dense_count);
        let mut document = String::from(
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="Sketcher::SketchObject" name="Dense" id="1"/><Object type="Sketcher::SketchObject" name="Triangle" id="2"/></Objects>
<ObjectData Count="2">"#,
        );
        write!(
            document,
            r#"<Object name="Dense"><Properties Count="1">
<Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="{dense_count}">{dense}</GeometryList></Property>
</Properties></Object>
<Object name="Triangle"><Properties Count="2">
<Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="3">
<Geometry type="Part::GeomLineSegment"><LineSegment StartX="0" StartY="0" EndX="1" EndY="0"/></Geometry>
<Geometry type="Part::GeomLineSegment"><LineSegment StartX="1" StartY="0" EndX="0" EndY="1"/></Geometry>
<Geometry type="Part::GeomLineSegment"><LineSegment StartX="0" StartY="1" EndX="0" EndY="0"/></Geometry>
</GeometryList></Property>
<Property name="Constraints" type="Sketcher::PropertyConstraintList"><ConstraintList count="3">
<Constrain Type="1" First="0" FirstPos="2" Second="1" SecondPos="1"/>
<Constrain Type="1" First="1" FirstPos="2" Second="2" SecondPos="1"/>
<Constrain Type="1" First="2" FirstPos="2" Second="0" SecondPos="1"/>
</ConstraintList></Property>
</Properties></Object></ObjectData></Document>"#
        )
        .expect("format document");
        fcstd(&document)
    }

    #[test]
    fn only_explicit_constraints_join_sketch_entities_under_every_profile() {
        let bytes = unconstrained_segments_then_constrained_triangle();
        for (profile, policy) in [
            ("desktop", DecodePolicy::desktop()),
            ("service", DecodePolicy::service()),
        ] {
            let result = FcstdCodec
                .decode(
                    &mut Cursor::new(&bytes),
                    &DecodeOptions {
                        depth: DecodeDepth::Full,
                        policy,
                    },
                )
                .unwrap_or_else(|error| panic!("{profile}: sketch projection refused: {error}"));
            let sketch = |name: &str| {
                result
                    .ir()
                    .model
                    .sketches
                    .iter()
                    .find(|sketch| sketch.name.as_deref() == Some(name))
                    .unwrap_or_else(|| panic!("{profile}: sketch {name} was dropped"))
            };
            let dense = &sketch("Dense").profiles;
            assert_eq!(dense.len(), DENSE_SEGMENTS, "{profile}");
            assert!(dense.iter().all(|chain| chain.len() == 1), "{profile}");
            let triangle = &sketch("Triangle").profiles;
            assert_eq!(triangle.len(), 1, "{profile}");
            assert_eq!(triangle[0].len(), 3, "{profile}");
            let bounded = result
                .report()
                .losses
                .iter()
                .filter(|loss| loss.code.local_code() == "sketch.profile-matching-bounded")
                .collect::<Vec<_>>();
            assert!(bounded.is_empty(), "{profile}: {bounded:#?}");
        }
    }
}

/// Cross-codec product-structure regression tests.
#[cfg(all(feature = "fcstd", feature = "step"))]
mod product_roundtrip {
    #![allow(clippy::unwrap_used)]

    use cadmpeg_test_support::assembly;
    use std::collections::{BTreeMap, HashMap, HashSet};
    use std::io::Cursor;

    use cadmpeg_codec_freecad::FcstdCodec;
    use cadmpeg_codec_step::StepCodec;
    use cadmpeg_ir::codec::write::target::TargetRequest;
    use cadmpeg_ir::codec::write::EncodeInput;
    use cadmpeg_ir::codec::write::Encoder;
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use cadmpeg_ir::products::{AssemblyGraph, Occurrence, OccurrenceParent, PrototypeReference};
    use cadmpeg_ir::CadIr;

    const CORE_DESIGN_PRODUCT: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../corpus/freecad_fcstd/fixtures/core_design_product.FCStd"
    ));

    fn definition_names(ir: &CadIr) -> HashMap<&str, String> {
        ir.model
            .product_definitions
            .iter()
            .map(|definition| {
                (
                    definition.id.as_str(),
                    definition
                        .part_number
                        .as_ref()
                        .or(definition.source_name.as_ref())
                        .or(definition.label.as_ref())
                        .expect("definition has a stable product name")
                        .clone(),
                )
            })
            .collect()
    }

    fn occurrence_paths(ir: &CadIr) -> BTreeMap<String, [[f64; 4]; 4]> {
        fn path(
            occurrence: &Occurrence,
            occurrences: &HashMap<&str, &Occurrence>,
            definitions: &HashMap<&str, String>,
            memo: &mut HashMap<String, String>,
        ) -> String {
            if let Some(path) = memo.get(occurrence.id.as_str()) {
                return path.clone();
            }
            let definition = match &occurrence.prototype {
                PrototypeReference::Local { definition } => definitions
                    .get(definition.as_str())
                    .expect("local prototype resolves"),
                _ => panic!("round-trip fixture contains only local prototypes"),
            };
            let segment = format!("{}:{definition}", occurrence.ordinal);
            let resolved = match &occurrence.parent {
                OccurrenceParent::Root {} => segment,
                OccurrenceParent::Occurrence { occurrence: parent } => format!(
                    "{}/{}",
                    path(
                        occurrences
                            .get(parent.as_str())
                            .expect("parent occurrence resolves"),
                        occurrences,
                        definitions,
                        memo,
                    ),
                    segment
                ),
            };
            memo.insert(occurrence.id.as_str().to_owned(), resolved.clone());
            resolved
        }

        let definitions = definition_names(ir);
        let occurrences = ir
            .model
            .occurrences
            .iter()
            .map(|occurrence| (occurrence.id.as_str(), occurrence))
            .collect::<HashMap<_, _>>();
        let graph = AssemblyGraph::new(&ir.model.occurrences).expect("valid assembly graph");
        let mut memo = HashMap::new();
        ir.model
            .occurrences
            .iter()
            .map(|occurrence| {
                (
                    path(occurrence, &occurrences, &definitions, &mut memo),
                    assembly::resolved_transform(&graph, &occurrence.id)
                        .expect("resolved transform")
                        .rows(),
                )
            })
            .collect()
    }

    #[test]
    fn fcstd_assembly_round_trips_through_step_without_losing_its_tree() {
        let mut source = FcstdCodec
            .decode(
                &mut Cursor::new(CORE_DESIGN_PRODUCT),
                &DecodeOptions::default(),
            )
            .expect("decode FCStd assembly")
            .into_parts()
            .0;

        let assembly_root = source
            .model
            .occurrences
            .iter()
            .find(|occurrence| {
                matches!(occurrence.parent, OccurrenceParent::Root {})
                    && matches!(
                        &occurrence.prototype,
                        PrototypeReference::Local { definition }
                            if source.model.product_definitions.iter().any(|candidate| {
                                candidate.id == *definition
                                    && candidate.source_name.as_deref() == Some("Product")
                            })
                    )
            })
            .expect("Product assembly root")
            .id
            .clone();
        let mut retained = HashSet::from([assembly_root.clone()]);
        loop {
            let before = retained.len();
            for occurrence in &source.model.occurrences {
                if matches!(
                    &occurrence.parent,
                    OccurrenceParent::Occurrence { occurrence: parent }
                        if retained.contains(parent)
                ) {
                    retained.insert(occurrence.id.clone());
                }
            }
            if retained.len() == before {
                break;
            }
        }
        source
            .model
            .occurrences
            .retain(|occurrence| retained.contains(&occurrence.id));
        source
            .model
            .occurrences
            .iter_mut()
            .find(|occurrence| occurrence.id == assembly_root)
            .expect("retained root")
            .ordinal = 0;

        assert_eq!(
            source
                .model
                .product_definitions
                .iter()
                .map(|definition| definition.bodies.len())
                .sum::<usize>(),
            source.model.bodies.len(),
            "each FCStd body belongs to its source product definition"
        );
        let expected_definitions = definition_names(&source)
            .into_values()
            .collect::<HashSet<_>>();
        let expected_occurrences = occurrence_paths(&source);
        assert_eq!(expected_definitions.len(), 6);
        assert_eq!(expected_occurrences.len(), 6);

        let mut step = Vec::new();
        StepCodec::default()
            .plan(EncodeInput::new(&source, None), TargetRequest::Inherit)
            .and_then(|plan| plan.write_to(&mut step))
            .expect("write STEP assembly");
        let decoded = StepCodec::default()
            .decode(&mut Cursor::new(step), &DecodeOptions::default())
            .expect("decode STEP assembly")
            .into_parts()
            .0;

        assert_eq!(
            definition_names(&decoded)
                .into_values()
                .collect::<HashSet<_>>(),
            expected_definitions
        );
        let actual_occurrences = occurrence_paths(&decoded);
        assert_eq!(
            actual_occurrences.keys().collect::<Vec<_>>(),
            expected_occurrences.keys().collect::<Vec<_>>()
        );
        for (path, expected) in expected_occurrences {
            let actual = actual_occurrences.get(&path).expect("same occurrence path");
            for row in 0..4 {
                for column in 0..4 {
                    assert!(
                        (actual[row][column] - expected[row][column]).abs() <= 1.0e-9,
                        "resolved transform differs at {path}[{row}][{column}]: expected {}, got {}",
                        expected[row][column],
                        actual[row][column]
                    );
                }
            }
        }
    }

    const CORE_OPERATIONS: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../corpus/freecad_fcstd/fixtures/core_operations.FCStd"
    ));

    #[test]
    fn fcstd_trimmed_edges_round_trip_through_step_onto_their_vertices() {
        let source = FcstdCodec
            .decode(&mut Cursor::new(CORE_OPERATIONS), &DecodeOptions::default())
            .expect("decode FCStd chamfer and fillet")
            .into_parts()
            .0;
        let mut step = Vec::new();
        StepCodec::default()
            .plan(EncodeInput::new(&source, None), TargetRequest::Inherit)
            .and_then(|plan| plan.write_to(&mut step))
            .expect("write STEP");
        let decoded = StepCodec::default()
            .decode(&mut Cursor::new(step), &DecodeOptions::default())
            .expect("decode STEP");
        let validation =
            cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
                .expect("validate the round-tripped model");
        let errors = validation
            .findings
            .iter()
            .filter(|finding| finding.severity == cadmpeg_ir::report::Severity::Error)
            .map(|finding| finding.message.as_str())
            .collect::<Vec<_>>();
        assert!(errors.is_empty(), "{errors:#?}");
    }
}

/// Subset curves whose local cache is no conic or spline: an ellipse subset
/// whose angle 0 is off the ellipse axes, and a parabola subset with no local
/// cache. Each is the curve of one unit-cube edge, through both of its
/// vertices, in either sense.
#[cfg(all(feature = "step", feature = "iges", feature = "f3d"))]
mod subset_carriers {
    #![allow(clippy::unwrap_used)]

    use std::io::Cursor;

    use cadmpeg_codec_iges::IgesCodec;
    use cadmpeg_codec_step::StepCodec;
    use cadmpeg_ir::codec::write::target::TargetRequest;
    use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use cadmpeg_ir::eval::admission::EvaluationAdmission;
    use cadmpeg_ir::eval::decode::curve_point;
    use cadmpeg_ir::features::FinitePoint3;
    use cadmpeg_ir::geometry::analytic::{EllipseCurve, ParabolaCurve};
    use cadmpeg_ir::geometry::curve_payloads::SubsetCurveConstruction;
    use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsPoles3};
    use cadmpeg_ir::geometry::{
        CurveGeometry, ProceduralCurve, ProceduralCurveDefinition, SolvedCurveGeometry,
    };
    use cadmpeg_ir::ids::ProceduralCurveId;
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::topology::ParameterInterval;
    use cadmpeg_ir::CadIr;

    #[derive(Clone, Copy, Debug)]
    enum Basis {
        Ellipse,
        Parabola,
        Nurbs,
    }

    const RANGE: [f64; 2] = [0.7, 2.0];

    /// Absolute tolerance for an edge range to lie in its subset interval.
    const EPS_SUBSET_RANGE: f64 = 1.0e-9;

    fn canonical(
        basis: Basis,
        size: f64,
        origin: Point3,
        x: Vector3,
        normal: Vector3,
    ) -> SolvedCurveGeometry {
        match basis {
            Basis::Ellipse => SolvedCurveGeometry::Ellipse(
                EllipseCurve::try_new(origin, normal, x, 3.0 * size, size).unwrap(),
            ),
            Basis::Parabola => SolvedCurveGeometry::Parabola(
                ParabolaCurve::try_new(origin, normal, x, size).unwrap(),
            ),
            Basis::Nurbs => {
                let y = cross(normal, x);
                let poles = [[0.0, 0.0], [1.0, 1.0], [2.0, 0.0], [3.0, 1.0]]
                    .into_iter()
                    .map(|[u, v]| {
                        let offset = sum(scaled(x, u * size), scaled(y, v * size));
                        FinitePoint3::new(Point3::new(
                            origin.x + offset.x,
                            origin.y + offset.y,
                            origin.z + offset.z,
                        ))
                        .unwrap()
                    })
                    .collect();
                SolvedCurveGeometry::Nurbs(
                    NurbsCurve::new(
                        2,
                        vec![0.0, 0.0, 0.0, 1.5, 3.0, 3.0, 3.0],
                        NurbsPoles3::Polynomial { points: poles },
                        false,
                    )
                    .unwrap(),
                )
            }
        }
    }

    fn at(geometry: &SolvedCurveGeometry, parameter: f64) -> Point3 {
        curve_point(
            EvaluationAdmission::Standard,
            &CurveGeometry::Solved(geometry.clone()),
            parameter,
        )
        .unwrap()
        .get()
    }

    fn sub(a: Point3, b: Point3) -> Vector3 {
        Vector3::new(a.x - b.x, a.y - b.y, a.z - b.z)
    }

    fn cross(a: Vector3, b: Vector3) -> Vector3 {
        Vector3::new(
            a.y * b.z - a.z * b.y,
            a.z * b.x - a.x * b.z,
            a.x * b.y - a.y * b.x,
        )
    }

    fn norm(v: Vector3) -> f64 {
        (v.x * v.x + v.y * v.y + v.z * v.z).sqrt()
    }

    fn scaled(v: Vector3, s: f64) -> Vector3 {
        Vector3::new(v.x * s, v.y * s, v.z * s)
    }

    fn sum(a: Vector3, b: Vector3) -> Vector3 {
        Vector3::new(a.x + b.x, a.y + b.y, a.z + b.z)
    }

    /// The basis curve placed so that `RANGE[0]` lies at `first` and
    /// `RANGE[1]` at `second`, in a plane through both with normal `normal`.
    fn through(
        basis: Basis,
        first: Point3,
        second: Point3,
        normal: Vector3,
    ) -> SolvedCurveGeometry {
        let origin = Point3::new(0.0, 0.0, 0.0);
        let (reference, axis) = (Vector3::new(1.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0));
        let unit = canonical(basis, 1.0, origin, reference, axis);
        let size = norm(sub(first, second)) / norm(sub(at(&unit, RANGE[1]), at(&unit, RANGE[0])));
        let local = canonical(basis, size, origin, reference, axis);
        let (start, end) = (at(&local, RANGE[0]), at(&local, RANGE[1]));
        let chord = sub(end, start);
        let (chord_x, chord_y) = (chord.x / norm(chord), chord.y / norm(chord));
        let along = scaled(sub(second, first), 1.0 / norm(sub(second, first)));
        let across = cross(normal, along);
        let placed_x = sum(scaled(along, chord_x), scaled(across, -chord_y));
        let placed_y = sum(scaled(along, chord_y), scaled(across, chord_x));
        let offset = sum(scaled(placed_x, start.x), scaled(placed_y, start.y));
        let placed = Point3::new(first.x - offset.x, first.y - offset.y, first.z - offset.z);
        canonical(basis, size, placed, placed_x, normal)
    }

    /// The unit cube with its first edge on a subset of `basis`.
    fn cube(basis: Basis, sense: bool) -> CadIr {
        chain(basis, &[(RANGE, sense)])
    }

    /// The links of a two-level chain whose outer local range maps onto
    /// `RANGE` of the basis through an inner subset over `[0.5, 3.0]`.
    fn nested_links(inner: bool, outer: bool) -> [([f64; 2], bool); 2] {
        let outer_range = if inner { [0.2, 1.5] } else { [1.0, 2.3] };
        [([0.5, 3.0], inner), (outer_range, outer)]
    }

    /// The unit cube with its first edge on a chain of subsets over `basis`,
    /// innermost first, that maps the edge onto `RANGE` of the basis.
    fn chain(basis: Basis, links: &[([f64; 2], bool)]) -> CadIr {
        let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let edge = ir.model.edges[0].clone();
        let position = |vertex: &cadmpeg_ir::ids::VertexId| {
            let vertex = ir.model.vertices.iter().find(|v| v.id == *vertex).unwrap();
            let point = ir
                .model
                .points
                .iter()
                .find(|p| p.id == vertex.point)
                .unwrap();
            point.position().get()
        };
        let (start, end) = (position(&edge.start), position(&edge.end));
        let d = sub(end, start);
        let normal = if d.x.abs() < 0.5 {
            Vector3::new(1.0, 0.0, 0.0)
        } else {
            Vector3::new(0.0, 1.0, 0.0)
        };
        let forward = links.iter().filter(|(_, sense)| !sense).count() % 2 == 0;
        let (first, second) = if forward { (start, end) } else { (end, start) };
        let root = cadmpeg_ir::ids::CurveId::mint("test:model:curve#subset-source").unwrap();
        let mut source = (root.clone(), Some(through(basis, first, second, normal)));
        ir.model.curves.push(cadmpeg_ir::geometry::Curve {
            id: root,
            geometry: CurveGeometry::Solved(source.1.clone().unwrap()),
            parameter_range: cadmpeg_ir::topology::IncreasingParameterInterval::new(RANGE),
            source_object: None,
        });
        let edge_curve = edge.curve().unwrap().clone();
        let mut span = 0.0;
        for (index, (range, sense)) in links.iter().enumerate() {
            let curve_id = if index + 1 == links.len() {
                edge_curve.clone()
            } else {
                let curve_id =
                    cadmpeg_ir::ids::CurveId::mint(format!("test:model:curve#subset-{index}"))
                        .unwrap();
                ir.model.curves.push(cadmpeg_ir::geometry::Curve {
                    id: curve_id.clone(),
                    geometry: CurveGeometry::Solved(line_placeholder()),
                    parameter_range: None,
                    source_object: None,
                });
                curve_id
            };
            let subset =
                SubsetCurveConstruction::try_new(source.0.clone(), *range, *sense, None).unwrap();
            let cache = source
                .1
                .as_ref()
                .and_then(|geometry| subset.local_cache(geometry));
            if links.len() == 1 {
                assert_eq!(
                    matches!(cache, Some(SolvedCurveGeometry::Transformed(_))),
                    matches!(basis, Basis::Ellipse),
                    "{basis:?}"
                );
            }
            let construction =
                ProceduralCurveId::mint(format!("test:model:construction#subset-{index}")).unwrap();
            ir.model
                .curves
                .iter_mut()
                .find(|curve| curve.id == curve_id)
                .unwrap()
                .geometry = match &cache {
                Some(cache) => CurveGeometry::Solved(cache.clone()),
                None => CurveGeometry::Procedural {
                    construction: construction.clone(),
                    cache: None,
                },
            };
            let mut definition = ProceduralCurveDefinition::Subset(subset);
            if cache.is_some() {
                definition
                    .set_legacy_cache(cadmpeg_ir::geometry::LegacyCache::new(
                        cadmpeg_ir::geometry::FitTolerance::ZERO,
                    ))
                    .unwrap();
            }
            ir.model
                .add_procedural_curve(&curve_id, ProceduralCurve::new(construction, definition))
                .unwrap();
            span = range[1] - range[0];
            source = (curve_id, cache);
        }
        ir.model.edges[0].set_param_range(Some(ParameterInterval::new([0.0, span]).unwrap()));
        ir.model
            .curves
            .sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
        ir
    }

    /// A carrier that `chain` replaces before the curve is used.
    fn line_placeholder() -> SolvedCurveGeometry {
        SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )
    }

    fn assert_valid(ir: &CadIr, label: &str) {
        let validation = cadmpeg_ir::validate_neutral(ir, Vec::new()).unwrap();
        let errors = validation
            .findings
            .iter()
            .filter(|finding| finding.severity == cadmpeg_ir::report::Severity::Error)
            .map(|finding| finding.message.as_str())
            .collect::<Vec<_>>();
        assert!(errors.is_empty(), "{label}: {errors:#?}");
    }

    #[test]
    fn step_writes_each_subset_edge_as_a_trimmed_curve() {
        for basis in [Basis::Ellipse, Basis::Parabola] {
            for sense in [true, false] {
                let ir = cube(basis, sense);
                let validation = cadmpeg_ir::validate_neutral(&ir, Vec::new()).unwrap();
                assert!(
                    validation
                        .findings
                        .iter()
                        .all(|finding| finding.severity != cadmpeg_ir::report::Severity::Error),
                    "{basis:?} {sense}: {:#?}",
                    validation.findings
                );
                let mut step = Vec::new();
                let report = StepCodec::default()
                    .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
                    .and_then(|plan| plan.write_to(&mut step))
                    .unwrap();
                let losses = report
                    .losses
                    .iter()
                    .map(|loss| loss.code.to_string())
                    .collect::<Vec<_>>();
                assert!(
                    !losses
                        .iter()
                        .any(|code| code == "topology.edge-curve-unsupported"),
                    "{basis:?} {sense}: {losses:?}"
                );
                let text = String::from_utf8(step).unwrap();
                assert_eq!(
                    text.matches("TRIMMED_CURVE(").count(),
                    1,
                    "{basis:?} {sense}"
                );
                assert_eq!(text.matches("EDGE_CURVE(").count(), 12, "{basis:?} {sense}");
            }
        }
    }

    #[test]
    fn iges_writes_each_subset_edge_on_its_source_carrier() {
        for basis in [Basis::Ellipse, Basis::Parabola] {
            for sense in [true, false] {
                let ir = cube(basis, sense);
                let mut iges = Vec::new();
                IgesCodec
                    .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
                    .and_then(|plan| plan.write_to(&mut iges))
                    .unwrap_or_else(|error| panic!("{basis:?} {sense}: {error}"));
                assert!(!iges.is_empty());
            }
        }
    }

    #[test]
    fn iges_writes_a_nested_subset_edge_on_its_root_carrier() {
        for inner in [true, false] {
            for outer in [true, false] {
                let label = format!("inner {inner} outer {outer}");
                let ir = chain(Basis::Ellipse, &nested_links(inner, outer));
                assert_valid(&ir, &label);
                let mut iges = Vec::new();
                IgesCodec
                    .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
                    .and_then(|plan| plan.write_to(&mut iges))
                    .unwrap_or_else(|error| panic!("{label}: {error}"));
                let decoded = IgesCodec
                    .decode(&mut Cursor::new(iges), &DecodeOptions::default())
                    .unwrap_or_else(|error| panic!("{label}: {error}"));
                assert_valid(decoded.ir(), &label);
            }
        }
    }

    #[test]
    fn f3d_writes_nurbs_subset_edges_on_the_parameter_of_their_source() {
        let mut models = vec![
            ("sense true".to_owned(), cube(Basis::Nurbs, true)),
            ("sense false".to_owned(), cube(Basis::Nurbs, false)),
        ];
        for inner in [true, false] {
            for outer in [true, false] {
                models.push((
                    format!("inner {inner} outer {outer}"),
                    chain(Basis::Nurbs, &nested_links(inner, outer)),
                ));
            }
        }
        for (label, ir) in models {
            assert_valid(&ir, &label);
            let mut f3d = Vec::new();
            cadmpeg_codec_f3d::F3dCodec
                .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
                .and_then(|plan| plan.write_to(&mut f3d))
                .unwrap_or_else(|error| panic!("{label}: {error}"));
            let decoded = cadmpeg_codec_f3d::F3dCodec
                .decode(&mut Cursor::new(f3d), &DecodeOptions::default())
                .unwrap_or_else(|error| panic!("{label}: {error}"));
            assert_valid(decoded.ir(), &label);
            let decoded = decoded.ir();
            let mut subset_edges = 0;
            for procedural in &decoded.model.procedural_curves {
                let ProceduralCurveDefinition::Subset(subset) = procedural.definition() else {
                    continue;
                };
                let [first, last] = subset.parameter_range().endpoints();
                let span = last - first;
                let owner = decoded.model.procedural_curve_owner(&procedural.id);
                for edge in decoded
                    .model
                    .edges
                    .iter()
                    .filter(|edge| edge.curve() == owner)
                {
                    let [start, end] = edge.param_range().unwrap().get();
                    assert!(
                        start >= -EPS_SUBSET_RANGE && end <= span + EPS_SUBSET_RANGE,
                        "{label}: edge range outside the local subset domain"
                    );
                    subset_edges += 1;
                }
            }
            assert_eq!(subset_edges, 1, "{label}");
        }
    }
}
