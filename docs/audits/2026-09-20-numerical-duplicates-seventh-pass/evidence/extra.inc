f=source('cadmpeg-codec-rhino/src/extrusion.rs')
s+='mod rhino {use super::*;type GeometryError=String;fn error(_:usize,s:impl ToString)->String{s.to_string()}'+consts(f)+''.join(fn(f,n)for n in ['active_miter','normalize','rodrigues','mitered_local'])+'''
#[test]fn shallow_miter_tilt_is_lost(){let normal=active_miter(true,Vector3::new(1e-8,0.,1.)).unwrap();let p=mitered_local(Vector3::new(1e8,0.,0.),Some(normal),0).unwrap();let residual=normal.dot(p);println!("Rhino shallow miter point {p:?}, plane residual {residual}");assert_eq!(p.z,0.);assert!(residual>0.99);}}
'''
f=source('cadmpeg-codec-step/src/reader/validation.rs')
s+='''mod step {use super::*;use std::collections::BTreeMap;
#[derive(Debug)]struct MeshProperties{area:f64,volume:f64,centroid:Point3}
struct Body{id:usize}struct Mesh{body:Option<usize>,vertices:Vec<Point3>,triangles:Vec<[u32;3]>}
impl Mesh{fn vertices(&self)->&[Point3]{&self.vertices}fn triangles(&self)->&[[u32;3]]{&self.triangles}}
struct Model{bodies:Vec<Body>,tessellations:Vec<Mesh>}struct CadIr{model:Model}
'''+fn(f,'mesh_properties')+'''
#[test]fn finite_mass_properties_are_lost(){let shape=|scale:f64|CadIr{model:Model{bodies:vec![Body{id:1}],tessellations:vec![Mesh{body:Some(1),vertices:vec![Point3::new(0.,0.,0.),Point3::new(2.*scale,0.,0.),Point3::new(0.,scale,0.),Point3::new(0.,0.,scale)],triangles:vec![[0,2,1],[0,1,3],[0,3,2],[1,2,3]]}]}};let unit=mesh_properties(&shape(1.)).unwrap();assert!((unit.volume-1./3.).abs()<f64::EPSILON);let result=mesh_properties(&shape(1e90));println!("STEP scale 1e90 mass properties: {result:?}");assert!(result.is_none());assert!((unit.area*1e180).is_finite());assert!((unit.volume*1e270).is_finite());}}
'''
f=source('cadmpeg-codec-nx/src/decode/blend.rs')
s+='mod nx {use super::*;'+''.join(fn(f,n)for n in ['homogeneous_residual_distance','rational_squared_distance_derivative','bernstein_product','binomial_coefficient','add_bernstein_polynomials','sum_bernstein_polynomials','subtract_bernstein_polynomials'])+'''
#[test]fn common_weight_scale_changes_projection(){let controls=|w:f64|[[-0.25*w,0.,0.,w],[0.75*w,0.,0.,w]];let plain=rational_squared_distance_derivative(&controls(1.)).unwrap();let large=rational_squared_distance_derivative(&controls(1e200));let tiny=rational_squared_distance_derivative(&controls(1e-200)).unwrap();let large_distance=homogeneous_residual_distance(&controls(1e200),0.,[0.,1.]);let tiny_distance=homogeneous_residual_distance(&controls(1e-200),0.,[0.,1.]);println!("NX derivative ordinary {plain:?}; large {large:?}; tiny {tiny:?}; residual distances {large_distance}, {tiny_distance}");assert!(large.is_none());assert!(tiny.iter().all(|x|*x==0.));assert!(large_distance.is_infinite());assert_eq!(tiny_distance,0.);let curve=NurbsCurve::from_lanes(1,vec![0.,0.,1.,1.],vec![Point3::new(0.,0.,0.),Point3::new(1.,0.,0.)],Some(vec![1e200;2]),false).unwrap();assert!(cadmpeg_ir::eval::curve_point(&CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),0.25).unwrap().is_finite());}}
'''
f=source('cadmpeg-codec-sldprt/src/resolved_features/write_prepare.rs')
a=f.index('let first = [first_end.u - first_start.u, first_end.v - first_start.v];',f.index('fn validate_solved_dimension'))
b=f.index('\n        DimensionDefinition::Radius',a)
block=f[a:b].rstrip();assert block.endswith('}');block=block[:-1].rstrip()
# Preserve the numeric branch; replace only its surrounding result carrier.
block=block.replace('((first[0] * second[0] + first[1] * second[1]) / denominator)','Ok(((first[0] * second[0] + first[1] * second[1]) / denominator)')+')'
s+='''mod sld {use super::*;const SKETCH_POINT_TOLERANCE:f64=1e-9;
mod cadmpeg_core{#[derive(Debug)]pub struct CodecError;impl CodecError{pub fn malformed(_:impl std::fmt::Display)->Self{Self}}}
struct Constraint{id:String}
fn angle(first_start:Point2,first_end:Point2,second_start:Point2,second_end:Point2)->Result<f64,cadmpeg_core::CodecError>{let constraint=Constraint{id:String::new()};
'''+block+'}\n'+fn(f,'vector2_length')+'''
#[test]fn valid_angular_dimensions_are_refused(){let o=Point2::new(0.,0.);let short=angle(o,Point2::new(1e-5,0.),o,Point2::new(0.,1e-5));let a=1e-8_f64;let shallow=angle(o,Point2::new(1.,0.),o,Point2::new(a.cos(),a.sin())).unwrap();println!("SLDPRT short perpendicular dimension {short:?}; shallow 1e-8 angle {shallow}");assert!(short.is_err());assert_eq!(shallow,0.);assert!((shallow-a).abs()>SKETCH_POINT_TOLERANCE*(1.+shallow.abs().max(a)));}}
'''
