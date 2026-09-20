#!/usr/bin/env python3
# Extracts current production arithmetic; assertions reproduce known failures.
from pathlib import Path
import re,json,hashlib,subprocess
import tempfile
D=Path(tempfile.mkdtemp(prefix='cadmpeg-numeric-followup-'));print('Evidence directory:',D);manifest=[]
def read(p):
 p=Path('crates')/p;s=p.read_text();manifest.append({'path':str(p),'sha256':hashlib.sha256(s.encode()).hexdigest()});return s
def fn(s,n):
 m=re.search(r'(?m)^\s*(?:pub(?:\([^\n]*\))? )?fn '+re.escape(n)+r'\b',s);assert m,n
 a=s.index('{',m.start());d=1;i=a+1
 while d:d+=(s[i]=='{')-(s[i]=='}');i+=1
 return re.sub(r'^pub\([^)]*\) ', '',s[m.start():i].strip())+'\n'
s='''#![allow(dead_code,unused_imports,unused_variables)]
use cadmpeg_ir::math::{Point2,Point3,Vector3};
use cadmpeg_ir::geometry::{nurbs::{NurbsCurve,NurbsSurface,NurbsSurfaceAxis,NurbsSurfaceLanes}};
use cadmpeg_core::decode::alloc_filled;
use std::borrow::Cow;
fn curve(knots:Vec<f64>,points:Vec<Point3>,weights:Option<Vec<f64>>,degree:u32)->NurbsCurve{NurbsCurve::from_lanes(degree,knots,points,weights,false).unwrap()}
fn knots_nondecreasing(k:&[f64])->bool { k.windows(2).all(|x|x[0]<=x[1]) }
#[derive(Clone)] struct HomogeneousBezierSpan {domain:[f64;2],controls:Vec<[f64;4]>}
fn midpoint(p:&[[f64;4]])->[f64;4]{let mut p=p.to_vec();for n in (1..p.len()).rev(){for i in 0..n{for j in 0..4{p[i][j]=0.5*p[i][j]+0.5*p[i+1][j]}}}p[0]}
'''
ir=read('cadmpeg-ir/src/eval.rs')
s+='mod ir_spans {use super::*;'+fn(ir,'insert_homogeneous_knot')+fn(ir,'homogeneous_bezier_spans')+'''
#[test]fn unclamped(){let poles=vec![Point3::new(0.,0.,0.),Point3::new(1.,1.,0.),Point3::new(2.,0.,0.)];let k=vec![-1.,-1.,0.,1.,2.,2.];let c=curve(k.clone(),poles.clone(),None,2);let actual=cadmpeg_ir::eval::nurbs_curve_point(2,&k,&poles,None,0.5).unwrap();let p=homogeneous_bezier_spans(2,&k,poles.iter().map(|p|[p.x,p.y,p.z,1.]).collect()).unwrap();let got=midpoint(&p[0].controls);println!("IR unclamped: decomposed_y={} actual_y={}",got[1],actual.y);assert_eq!(got[1],0.5);assert_eq!(actual.y,0.75);}
#[test]fn discontinuous(){let k=vec![0.,0.,0.,1.,1.,1.,2.,2.,2.];let poles=(0..6).map(|i|Point3::new(i as f64,0.,0.)).collect::<Vec<_>>();let c=curve(k.clone(),poles.clone(),None,2);let p=homogeneous_bezier_spans(2,&k,poles.iter().map(|p|[p.x,0.,0.,1.]).collect()).unwrap();let got=midpoint(&p[1].controls)[0];let actual=cadmpeg_ir::eval::nurbs_curve_point(2,&k,&poles,None,1.5).unwrap().x;println!("IR discontinuous: decomposed_x={got} actual_x={actual}");assert_eq!((got,actual),(3.,4.));}
}
'''
ig=read('cadmpeg-codec-iges/src/entities/surfaces.rs')
s+='mod iges_spans {use super::*;'+fn(ig,'insert_homogeneous_curve_knot')+fn(ig,'homogeneous_bezier_spans')+'''
#[test]fn unclamped(){let c=curve(vec![-1.,-1.,0.,1.,2.,2.],vec![Point3::new(0.,0.,0.),Point3::new(1.,1.,0.),Point3::new(2.,0.,0.)],None,2);let p=homogeneous_bezier_spans(&c).unwrap();let got=midpoint(&p[0].controls)[1];println!("IGES unclamped decomposed_y={got} expected_y=0.75");assert_eq!(got,0.5);}
#[test]fn discontinuous(){let c=curve(vec![0.,0.,0.,1.,1.,1.,2.,2.,2.],(0..6).map(|i|Point3::new(i as f64,0.,0.)).collect(),None,2);let p=homogeneous_bezier_spans(&c).unwrap();let got=midpoint(&p[1].controls)[0];println!("IGES discontinuous decomposed_x={got} expected_x=4");assert_eq!(got,3.);}}
'''
s+='mod ir_speed {use super::*;'+fn(ir,'nurbs_curve_speed_bound_about')+'''
#[test]fn common_weights(){let c=curve(vec![0.,0.,1.,1.],vec![Point3::new(0.,0.,0.),Point3::new(1.,0.,0.)],None,1);for w in [1.,1e-200]{let got=nurbs_curve_speed_bound_about(&c,&[w,w],Point3::new(0.,0.,0.));println!("IR speed weight={w:e} bound={got:?}");if w==1.{assert_eq!(got,Some(1.))}else{assert_eq!(got,None)}}}}
'''
s+='mod ir_laws {use super::*;#[derive(Clone)]enum LawExpression{Null{},Integer{value:i64},Double{value:f64},Text{value:String},Algebraic{operator:String,operands:Vec<Self>},Point{},Vector{},Transform{},TransformVec{},Edge{},Spline{}}#[derive(Clone,Copy,Debug)]struct ScalarSweepDifferential{value:f64,derivative:f64}\n'+''.join(fn(ir,n) for n in ['finite_sweep_differential','scalar_sweep_law_differential','scalar_unary_sweep_law_differential'])+'''
#[test]fn quotient(){let x=LawExpression::Text{value:"X".to_owned()};for d in [1e200,1e-200]{let expr=LawExpression::Algebraic{operator:"DIV".into(),operands:vec![x.clone(),LawExpression::Double{value:d}]};let got=scalar_sweep_law_differential(&expr,1.);println!("IR DIV denominator={d:e} got={got:?} expected_derivative={:e}",1./d);if d>1.{assert_eq!(got.unwrap().derivative,0.)}else{assert!(got.is_none())}}}
#[test]fn hyperbolic(){let d=scalar_unary_sweep_law_differential("TANH",ScalarSweepDifferential{value:20.,derivative:1e20}).unwrap();let expected=4.*(-40.0f64).exp()/(1.+(-40.0f64).exp()).powi(2)*1e20;println!("IR TANH(20) chain derivative={} expected={expected}",d.derivative);assert_eq!(d.derivative,0.);assert!(expected>1000.);let a=scalar_unary_sweep_law_differential("ARCOTH",ScalarSweepDifferential{value:1e20,derivative:1.}).unwrap();println!("IR ARCOTH(1e20) value={} expected~1e-20",a.value);assert_eq!(a.value,0.);let a=scalar_unary_sweep_law_differential("ARCSINH",ScalarSweepDifferential{value:1e200,derivative:1e200}).unwrap();println!("IR ARCSINH large chain derivative={} expected~1",a.derivative);assert_eq!(a.derivative,0.);}}
'''
f=read('cadmpeg-codec-f3d/src/design/geometry.rs')
s+='mod f3d {use super::*;enum ProfileBoundarySegment {Line{start:Point2,end:Point2},Arc{center:Point2,radius:f64,start_angle:f64,end_angle:f64}}\n'+''.join(fn(f,n) for n in ['point_distance','directed_angle_parameter','line_arc_intersects','line_arc_intersection_points'])+'''
#[test]fn copies_disagree(){let scale=1e-4;let start=Point2::new(-scale,2.*scale);let end=Point2::new(scale,2.*scale);let center=Point2::new(0.,0.);let got=line_arc_intersects((start,end),center,scale,0.,std::f64::consts::TAU);let points=line_arc_intersection_points((start,end),&ProfileBoundarySegment::Arc{center,radius:scale,start_angle:0.,end_angle:std::f64::consts::TAU}).unwrap();println!("F3D disjoint line/circle: bool={got} point_count={}",points.len());assert!(got);assert!(points.is_empty());}}
'''
s+='mod iges_closure {use super::*;'+''.join(fn(ig,n) for n in ['insert_homogeneous_curve_knot','homogeneous_bezier_spans','homogeneous_curve_boundary_matches'])+'''
#[test]fn omitted_control(){let k=vec![0.,0.,0.,1.,1.,1.,2.,2.,2.];let p=(0..6).map(|i|Point3::new(i as f64,0.,0.)).collect::<Vec<_>>();let a=curve(k.clone(),p.clone(),None,2);let mut q=p;q[5].y=4.;let b=curve(k,q,None,2);let got=homogeneous_curve_boundary_matches(&a,&b,[0.,2.],0.);println!("IGES discontinuous boundary equality={got:?}; actual difference at t=1.5 is 1");assert_eq!(got,Some(true));}
#[test]fn small_weights(){let k=vec![0.,0.,1.,1.];let a=curve(k.clone(),vec![Point3::new(0.,0.,0.),Point3::new(1.,0.,0.)],Some(vec![1e-200;2]),1);let b=curve(k,vec![Point3::new(0.,2.,0.),Point3::new(1.,2.,0.)],Some(vec![1e-200;2]),1);let got=homogeneous_curve_boundary_matches(&a,&b,[0.,1.],1e-6);println!("IGES offset parallel boundaries equality={got:?}; true separation=2 tolerance=1e-6");assert_eq!(got,Some(true));}}
'''
freecad=read('cadmpeg-codec-freecad/src/topology_transfer.rs')
s+='mod freecad {use super::*;use cadmpeg_ir::transform::Transform;use cadmpeg_core::CodecError;const EPS_TOPOLOGY_TRANSFER_DEGENERATE:f64=1e-10;'+''.join(fn(freecad,n) for n in ['uniform_scale','transform_normalized_vector'])+'''
#[test]fn small_shear(){let a=1e-6;let t=Transform::affine([[a,0.5*a,0.,0.],[0.,0.75f64.sqrt()*a,0.,0.],[0.,0.,a,0.]]).unwrap();let got=uniform_scale(t);println!("FreeCAD angle-60-degree columns accepted as similarity: {got:?}");assert_eq!(got.unwrap(),a);}
#[test]fn transformed_normal(){for a in [1e200,1e-200]{let t=Transform::affine([[a,0.,0.,0.],[0.,a,0.,0.],[0.,0.,a,0.]]).unwrap();let got=transform_normalized_vector(t,Vector3::new(1.,0.,0.)).unwrap();println!("FreeCAD scale={a:e} transformed normal length={:e} expected=1",got.norm());assert_eq!(got.x,a);}}}
'''
s+='mod ir_surface {use super::*;use cadmpeg_ir::eval::{nurbs_surface_point,nurbs_surface_partials};'+fn(ir,'nurbs_surface_parameter_near_point')+'''
#[test]fn small_plane(){let a=1e-5;let axis=NurbsSurfaceAxis::new(1,vec![0.,0.,1.,1.],false);let sf=NurbsSurface::from_lanes(axis.clone(),axis,NurbsSurfaceLanes::new(vec![vec![Point3::new(0.,0.,0.),Point3::new(0.,a,0.)],vec![Point3::new(a,0.,0.),Point3::new(a,a,0.)]],None),false).unwrap();let target=Point3::new(0.3*a,0.4*a,0.);let got=nurbs_surface_parameter_near_point(&sf,target,Some(Point2::new(0.,0.))).unwrap();println!("IR small plane UV=({},{}) expected=(0.3,0.4)",got.u,got.v);assert_eq!(got,Point2::new(0.,0.));}
#[test]fn cached_public_curve_residual(){let c=curve(vec![0.,0.,1.,1.],vec![Point3::new(0.,0.,0.),Point3::new(1.,0.,0.)],None,1);let got=cadmpeg_ir::eval::nurbs_curve_parameter_near_point(&c,Point3::new(0.,1e-200,0.),0.,0.);println!("IR cached public API: zero-tolerance witness for point off line={got:?}");assert_eq!(got,Some(0.));}}
'''
step=read('cadmpeg-codec-step/src/reader/validation.rs')
s+='''mod step {use super::*;use std::collections::BTreeMap;
struct MeshProperties{area:f64,volume:f64,centroid:Point3}
struct Body{id:String} struct Model{bodies:Vec<Body>,tessellations:Vec<Mesh>} struct CadIr{model:Model}
struct Mesh{body:Option<String>,points:Vec<Point3>,faces:Vec<[u32;3]>}
impl Mesh{fn vertices(&self)->&[Point3]{&self.points}fn triangles(&self)->&[[u32;3]]{&self.faces}}
'''+fn(step,'mesh_properties')+'''
#[test]fn translation(){for d in [0.,1e6,1e9]{let points=[[0.,0.,0.],[2.,0.,0.],[0.,1.,0.],[0.,0.,1.]].map(|p|Point3::new(p[0]+d,p[1]+d,p[2]+d)).to_vec();let ir=CadIr{model:Model{bodies:vec![Body{id:"body".into()}],tessellations:vec![Mesh{body:Some("body".into()),points,faces:vec![[0,2,1],[0,1,3],[0,3,2],[1,2,3]]}]}};let got=mesh_properties(&ir).unwrap();println!("STEP tetra translation={d}: volume={} (expected 1/3); relative centroid=({},{},{}) expected=(0.5,0.25,0.25)",got.volume,got.centroid.x-d,got.centroid.y-d,got.centroid.z-d);if d==0.{assert!((got.volume-1./3.).abs()<1e-15)}else{assert!((got.centroid.x-d-0.5).abs()>1e-3)}}}}
'''
nx=read('cadmpeg-codec-nx/src/native/display_jt.rs')
s+='mod nx {use super::*;use cadmpeg_core::decode::View;'+fn(nx,'parse_jt9_geometric_transform_body')+'''
#[test]fn f32_scale(){for a in [1f32,1e20,1e-30]{let mut b=Vec::new();b.extend(1u16.to_le_bytes());b.push(0);b.extend(0u32.to_le_bytes());b.extend(1u16.to_le_bytes());b.extend(0x8420u16.to_le_bytes());for _ in 0..3{b.extend(a.to_le_bytes())}let got=parse_jt9_geometric_transform_body(&b);println!("NX JT finite scale={a:e} accepted={}",got.is_some());assert_eq!(got.is_some(),a==1.);}}}
'''
sl=read('cadmpeg-codec-sldprt/src/tessellation.rs')
s+='mod sld {use super::*;const EPS_CYLINDER_ANGLE:f64=1e-12;const MAX_PLANAR_TRIM_ARC_SEGMENTS:usize=4096;'+fn(sl,'planar_arc_segments')+'''
#[test]fn sagitta(){let span=1e-5;let radius=1e12;let tolerance=1e-9;let (n,claimed)=planar_arc_segments(span,radius,tolerance);let actual=2.*radius*(span/(4.*n as f64)).sin().powi(2);println!("SLD arc segments={n} claimed_error={claimed:e} stable_error={actual:e} tolerance={tolerance:e}");assert_eq!(claimed,0.);assert!(actual>tolerance);}}
'''
creo=read('cadmpeg-codec-creo/src/decode/analytic/equations.rs');sketch=read('cadmpeg-codec-creo/src/decode/sketch/equations_coordinate.rs');defs=read('cadmpeg-codec-creo/src/feature/definitions.rs')
s+='mod creo {use super::*;const EPS_NEAR_ZERO:f64=1e-12;const EPS_SOLVER_SCALE:f64=1e-12;const EPS_DISCRIMINANT_SCALE:f64=1e-12;const EPS_SOLUTION_AGREEMENT:f64=1e-9;const EPS_DISTANCE_AGREEMENT:f64=1e-9;const TRIM_INTERSECTION_EPS:f64=1e-12;const TRIM_COORDINATE_EPS:f64=1e-9;'+fn(creo,'quadratic_real_roots')+fn(sketch,'quadratic_roots')+fn(sketch,'approximately_equal')+fn(defs,'trim_circle_circle_intersection')+'''
#[test]fn coefficient_scale(){for scale in [1.,1e-10,1e-20]{let analytic=quadratic_real_roots(scale,0.,-scale);let sketch=quadratic_roots((scale,0.,-scale));println!("Creo {scale:e}*(x*x-1): analytic={analytic:?}, sketch={sketch:?}, expected=[-1,1]");if scale==1.{assert_eq!(analytic,vec![-1.,1.]);assert_eq!(sketch,vec![-1.,1.]);}else{assert_ne!(analytic,vec![-1.,1.]);assert_ne!(sketch,vec![-1.,1.]);}}}
#[test]fn two_circle_intersections(){let r=1e-6;let got=trim_circle_circle_intersection([0.,0.],r,[r,0.],r);println!("Creo two intersections: returned unique point={got:?}; true points=(0.5e-6,+/-sqrt(3)*0.5e-6)");assert_eq!(got,Some([0.5e-6,0.]));}}
'''
a=read('cadmpeg-asm/src/nurbs/proc_surface.rs')
s+='mod asm {use super::*;const LEN_TO_MM:f64=10.;'+fn(a,'ellipse_to_nurbs')+'''
#[test]fn radii_scale(){for radius in [1.,1e200,1e-200]{let got=ellipse_to_nurbs([0.,0.,0.],[0.,0.,1.],[radius,0.,0.],0.5);println!("ASM finite ellipse radius={radius:e} accepted={}",got.is_some());assert_eq!(got.is_some(),radius==1.);}}}
'''
s+='mod f3d_speed {use super::*;use cadmpeg_ir::geometry::pcurve::PcurveNurbs;'+fn(f,'nurbs_speed_bound')+'''
#[test]fn weights(){for w in [1.,1e-200]{let c=PcurveNurbs::from_lanes(1,vec![0.,0.,1.,1.],vec![Point2::new(0.,0.),Point2::new(1.,0.)],Some(vec![w;2]),false).unwrap();let got=nurbs_speed_bound(&c);println!("F3D identical pcurve weights={w:e}, speed bound={got:?}");assert_eq!(got.is_some(),w==1.);}}}
'''
s+='mod ir_phase {use super::*;use cadmpeg_ir::eval::nurbs_curve_parameter_domain;'+fn(ir,'map_nurbs_curve_parameter')+'''
#[test]fn finite_phase(){let c=NurbsCurve::from_lanes(1,vec![-1e308,-1e308,-9e307,-9e307],vec![Point3::new(0.,0.,0.),Point3::new(1.,0.,0.)],None,true).unwrap();let got=map_nurbs_curve_parameter(&c,1e308).unwrap();println!("IR public periodic map finite parameter yields {got}");assert!(got.is_nan());}}
'''
# Extract the actual current distance closure and boundary helper; no replacement arithmetic.
dist=fn(ir,'nurbs_curve_parameter_near_point');dist=dist[dist.index('let distance ='):dist.index('let seed = seed.clamp')]
s+='mod ir_witness {use super::*;use cadmpeg_ir::eval::nurbs_curve_point;#[derive(Debug,PartialEq)]enum BoundaryWitness{Invalid,NoMatch,Found(f64)}'+fn(ir,'nearest_boundary_witness')+'''
#[test]fn false_zero_residual(){let curve=curve(vec![0.,0.,1.,1.],vec![Point3::new(0.,0.,0.),Point3::new(1.,0.,0.)],None,1);let weights=vec![1.,1.];let point=Point3::new(0.,1e-200,0.);
'''+dist+'''
let got=nearest_boundary_witness(&[0.,1.],0.,0.,distance);println!("IR current boundary witness with off-curve distance=1e-200 and tolerance=0: {got:?}");assert_eq!(got,BoundaryWitness::Found(0.));}}
'''
# The source mean is in a large decoder; reproduce its unchanged update/division expressions.
rh=read('cadmpeg-codec-rhino/src/brep.rs')
update=rh[rh.index('vertex.point_sum[0] +='):rh.index('vertex.point_count += 1;')+len('vertex.point_count += 1;')]
avg=rh[rh.index('accumulated.vertex.point = Point3(['):rh.index('accumulated.vertex.point = Point3([')+len('accumulated.vertex.point = Point3([')]
# Include the whole assignment, through the closing ]);.
a=rh.index('accumulated.vertex.point = Point3([');b=rh.index(']);',a)+3;avg=rh[a:b]
s+='''mod rhino_mean {#[derive(Debug,Clone,Copy)]struct Point3([f64;3]);struct Vertex{point:Point3}struct Accum{point_sum:[f64;3],point_count:usize,vertex:Vertex}
#[test]fn finite_endpoint_average(){let mut v=Accum{point_sum:[0.;3],point_count:0,vertex:Vertex{point:Point3([0.;3])}};for point in [Point3([1e308,0.,0.]);2]{let vertex=&mut v;
'''+update+'''
}let accumulated=&mut v;let count=accumulated.point_count as f64;
'''+avg+'''
println!("Rhino two identical finite endpoints: mean={:?} expected=[1e308,0,0]",v.vertex.point);assert!(v.vertex.point.0[0].is_infinite());}}
'''
geo=read('cadmpeg-ir/src/geometry.rs')
guard=geo[geo.index('let major_length =',geo.index('impl HelixPathConstruction')):geo.index('let angle_range = FiniteVector::new',geo.index('impl HelixPathConstruction'))]
s+='mod ir_helix {use super::*;const EPS_HELIX_SURFACE_RADIUS_RELATIVE:f64=1e-9;fn admit(major:Vector3,minor:Vector3)->Result<(),&\'static str>{'+guard+'Ok(())}'+r"""
#[test]fn finite_circular_frame(){for r in [1.,1e200,1e-200]{let got=admit(Vector3::new(r,0.,0.),Vector3::new(0.,r,0.));println!("IR circular helix radius={r:e} accepted={}",got.is_ok());assert_eq!(got.is_ok(),r==1.);}}}
"""
(D/'final-probes.rs').write_text('\n'.join(line.rstrip() for line in s.splitlines())+'\n');(D/'final-probes-manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
libs={n:max(Path('target/debug/deps').glob('lib'+n+'-*.rlib'),key=lambda p:p.stat().st_mtime) for n in ['cadmpeg_ir','cadmpeg_core']}
cmd=['rustc','--edition=2021','--test',str(D/'final-probes.rs'),'-L','dependency=target/debug/deps','-o',str(D/'final-probes')]
for n,p in libs.items():cmd+=['--extern',n+'='+str(p)]
(D/'final-probes.command').write_text(' '.join(cmd)+'\n')
with (D/'final-probes-build.log').open('w') as log:r=subprocess.run(cmd,stdout=log,stderr=subprocess.STDOUT)
(D/'final-probes-build.exit').write_text(str(r.returncode)+'\n');print('compile',r.returncode)
if r.returncode:print((D/'final-probes-build.log').read_text());raise SystemExit(r.returncode)
with (D/'final-probes.log').open('w') as log:r=subprocess.run([str(D/'final-probes'),'--nocapture','--test-threads=1'],stdout=log,stderr=subprocess.STDOUT)
(D/'final-probes.exit').write_text(str(r.returncode)+'\n');print((D/'final-probes.log').read_text())

raise SystemExit(r.returncode)
