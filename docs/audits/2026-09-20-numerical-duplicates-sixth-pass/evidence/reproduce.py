from pathlib import Path
import sys,re,json,hashlib,subprocess,tempfile
from tree_sitter import Language,Parser
import tree_sitter_rust
parser=Parser(Language(tree_sitter_rust.language()))
HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[3]
OUT=Path(tempfile.mkdtemp(prefix='cadmpeg-numeric-sixth-')); manifest=[]
print('Evidence directory:',OUT,flush=True)
def source(p):
 p=ROOT/'crates'/p; b=p.read_bytes();manifest.append(dict(path=str(p.relative_to(ROOT)),sha256=hashlib.sha256(b).hexdigest()));return b.decode()
def fn(s,name):
 b=s.encode(); t=parser.parse(b); stack=[t.root_node]
 while stack:
  n=stack.pop()
  if n.type=='function_item' and n.child_by_field_name('name').text.decode()==name:
   return re.sub(r'^pub(?:\([^)]*\))?\s+','',b[n.start_byte:n.end_byte].decode())+'\n'
  stack.extend(reversed(n.children))
 raise ValueError(name)
def consts(s):return '\n'.join(re.findall(r'(?m)^const [A-Z][A-Z0-9_]*: f64 = [^;]+;',s))+'\n'
s='''#![allow(dead_code,unused_imports,unused_variables)]
pub use cadmpeg_ir::math;
use cadmpeg_ir::math::{Point2,Point3,Vector3};
use cadmpeg_ir::geometry::{CurveGeometry,SolvedCurveGeometry};
use cadmpeg_ir::geometry::analytic::{CircleCurve,EllipseCurve};
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::sketches::{SketchGeometry,SketchGeometryDefinition};
fn line(a:[f64;2],b:[f64;2])->SketchGeometry {SketchGeometryDefinition::Line {start:Point2::new(a[0],a[1]),end:Point2::new(b[0],b[1])}.try_into().unwrap()}
'''
f=source('cadmpeg-asm/src/brep/geometry.rs')
s+='mod asm {use super::*;'+consts(f)+''.join(fn(f,n) for n in ['rational_four_arc_circle','reduce_homogeneous_bezier_to_quadratic','point_sum_difference','point_vector'])+'''
fn point_distance(a:Point3,b:Point3)->f64 {a.distance(b)}
fn rounded_square(w:f64)->NurbsCurve {NurbsCurve::from_lanes(2,vec![0.,0.,0.,1.,1.,2.,2.,3.,3.,4.,4.,4.],vec![(1.,0.),(1.,1.),(0.,1.),(-1.,1.),(-1.,0.),(-1.,-1.),(0.,-1.),(1.,-1.),(1.,0.)].into_iter().map(|(x,y)|Point3::new(x,y,0.)).collect(),Some(vec![w;9]),false).unwrap()}
#[test]fn noncircle_small_weights(){let normal=rational_four_arc_circle(&rounded_square(1.));let small=rational_four_arc_circle(&rounded_square(1e-12));println!("ASM polynomial rounded square, common weights 1: {normal:?}; 1e-12: {small:?}; midpoint (.75,.75) radius {}",0.75f64.hypot(0.75));assert!(normal.is_none());assert!(small.is_some());assert!((0.75f64.hypot(0.75)-1.).abs()>0.06);}
}
'''
f=source('cadmpeg-ir/src/validate/sketches.rs')
s+='mod ir_parallel {use super::*;'+consts(f)+'''struct PlanarParallelLines {first:[Point2;2],second:[Point2;2],distance:f64}
'''+''.join(fn(f,n) for n in ['planar_parallel_lines','planar_parallel_line_distance','planar_parallel_line_span_distance'])+'''
#[test]fn perpendicular_admitted_parallel(){let a=line([0.,0.],[1e200,0.]);let b=line([0.,1.],[0.,1e200]);let got=planar_parallel_line_distance(&a,&b);println!("IR perpendicular lines reported parallel distance: {got:?}; should None");assert_eq!(got,Some(1.));}
#[test]fn separated_spans_admitted_overflow(){let a=line([0.,0.],[1e200,0.]);let b=line([2e200,1.],[3e200,1.]);let got=planar_parallel_line_span_distance(&a,&b,1e-9);println!("IR disjoint projected spans report {got:?}; should None");assert_eq!(got,Some(1.));}
}
'''
f=source('cadmpeg-codec-catia/src/families/standard/decode.rs')
s+='mod catia {use super::*;const ANALYTIC_CURVE_ENDPOINT_TOLERANCE:f64=2e-3;'+fn(f,'standard_analytic_curve_angle')+'''
#[test]fn finite_circle_endpoint_angle(){for radius in [1e200,1e-200]{let g=CurveGeometry::Solved(SolvedCurveGeometry::Circle(CircleCurve::try_new(Point3::new(0.,0.,0.),Vector3::new(0.,0.,1.),Vector3::new(1.,0.,0.),radius).unwrap()));let got=standard_analytic_curve_angle(&g,Point3::new(radius,0.,0.));println!("CATIA circle radius{radius:e} first endpoint angle {got:?}, should Some(0)");assert!(got.is_none());}}
}
'''
f=source('cadmpeg-codec-sldprt/src/resolved_features/relation_records.rs')
s+='mod sld {use super::*;const SKETCH_POINT_TOLERANCE:f64=1e-9;'+fn(f,'line_direction')+fn(f,'line_line_angle')+'''
#[test]fn shallow_angle_erased(){let got=line_line_angle([[0.,0.],[1.,0.]],[[0.,0.],[1.,1e-8]]);println!("SLDPRT angle atan(1e-8): {got:?}; expected {}",1e-8_f64.atan());assert_eq!(got,Some(0.));}
}
'''
f=source('cadmpeg-codec-creo/src/decode/surfaces/intersection_candidates.rs')
s+='mod creo {use super::*;'+consts(f)+'''
#[derive(Clone,Copy)]struct CylinderEquation {origin:[f64;3],axis:[f64;3],ref_direction:[f64;3],radius:f64}
#[derive(Clone,Copy)]struct SphereEquation {center:[f64;3],ref_direction:[f64;3],radius:f64}
#[derive(Clone,Copy)]struct PlaneEquation {origin:[f64;3],normal:[f64;3]}
#[derive(Clone,Copy)]struct TorusEquation {center:[f64;3],axis:[f64;3],ref_direction:[f64;3],major_radius:f64,minor_radius:f64}
#[derive(Clone,Copy)]struct ConeEquation {radius:f64}
impl ConeEquation {fn origin(self)->[f64;3]{[0.;3]}fn axis(self)->[f64;3]{[0.,0.,1.]}fn ref_direction(self)->[f64;3]{[1.,0.,0.]}fn radius(self)->f64{self.radius}fn half_angle(self)->f64{std::f64::consts::FRAC_PI_4}}
fn circular_cone(_:ConeEquation)->bool{true}
#[derive(Clone,Copy)]enum CarrierEquation {Cylinder(CylinderEquation),Sphere(SphereEquation),Plane(PlaneEquation),Torus(TorusEquation),Cone(ConeEquation)}
'''
v=source('cadmpeg-codec-creo/src/vecmath.rs')
s+=consts(v)+''.join(fn(v,n) for n in ['dot','cross','normalize','normalize_with_length'])
s+=''.join(fn(f,n) for n in ['parallel_cylinder_generator_candidates','coaxial_cylinder_sphere_circle_candidates','parallel_plane_cylinder_generator_candidates','coaxial_cylinder_torus_circle_candidates','axis_normal_plane_torus_circle_candidates','coaxial_cone_sphere_circle_candidates','coaxial_cone_torus_circle_candidates'])+'''
fn cylinder(radius:f64,x:f64)->CarrierEquation {CarrierEquation::Cylinder(CylinderEquation{origin:[x,0.,0.],axis:[0.,0.,1.],ref_direction:[1.,0.,0.],radius})}
fn torus()->CarrierEquation {CarrierEquation::Torus(TorusEquation{center:[0.;3],axis:[0.,0.,1.],ref_direction:[1.,0.,0.],major_radius:3e-6,minor_radius:1e-6})}
#[test]fn torus_fabricated_intersections(){let a=coaxial_cylinder_torus_circle_candidates(cylinder(5e-6,0.),torus());let b=axis_normal_plane_torus_circle_candidates(CarrierEquation::Plane(PlaneEquation{origin:[0.,0.,2e-6],normal:[0.,0.,1.]}),torus());println!("Creo disjoint cylinder/torus circles={}, disjoint plane/torus circles={}, shouldzero",a.len(),b.len());assert_eq!(a.len(),1);assert_eq!(b.len(),1);}
#[test]fn cone_false_tangent_circles(){let a=coaxial_cone_sphere_circle_candidates(CarrierEquation::Cone(ConeEquation{radius:1e-6}),sphere(2e-6));let b=coaxial_cone_torus_circle_candidates(CarrierEquation::Cone(ConeEquation{radius:3e-6}),torus());println!("Creo cone/sphere {a:?}; cone/torus {b:?}; expected two valid crossings each");assert_eq!(a.len(),1);assert_eq!(b.len(),1);}
#[test]fn plane_cylinder_secants_lost(){let got=parallel_plane_cylinder_generator_candidates(CarrierEquation::Plane(PlaneEquation{origin:[0.;3],normal:[1.,0.,0.]}),cylinder(1e-10,0.));println!("Creo central plane/r1e-10 cylinder generators {}, expected2",got.len());assert!(got.is_empty());}
fn sphere(radius:f64)->CarrierEquation {CarrierEquation::Sphere(SphereEquation{center:[0.,0.,0.],ref_direction:[1.,0.,0.],radius})}
#[test]fn small_cylinder_secants_lost(){let count=parallel_cylinder_generator_candidates(cylinder(1e-6,0.),cylinder(1e-6,1e-6)).len();println!("Creo r=1e-6 cylinders spaced1e-6: {count} generators; expected2");assert_eq!(count,0);}
#[test]fn disjoint_cylinder_sphere_invents_circle(){let got=coaxial_cylinder_sphere_circle_candidates(cylinder(2e-6,0.),sphere(1e-6));println!("Creo r2e-6 cylinder outside r1e-6 sphere: {got:?}; expectedempty");assert_eq!(got.len(),1);}
#[test]fn secant_cylinder_sphere_becomes_tangent(){let got=coaxial_cylinder_sphere_circle_candidates(cylinder(1e-6,0.),sphere(2e-6));println!("Creo r1e-6 cylinder vs r2e-6 sphere: {got:?}; expected2 circles at +/-sqrt(3)e-6");assert_eq!(got.len(),1);}
}
'''
for p in sorted(HERE.glob('extra-*.inc')):exec(p.read_text())
(OUT/'probes.rs').write_text(s)
lib=max((ROOT/'target/debug/deps').glob('libcadmpeg_ir*.rlib'),key=lambda p:p.stat().st_mtime)
manifest.append(dict(cached_library=str(lib),sha256=hashlib.sha256(lib.read_bytes()).hexdigest()))
(OUT/'probe-sources.json').write_text(json.dumps(manifest,indent=2)+'\n')
cmd=['rustc','--edition=2021','--test',str(OUT/'probes.rs'),'--extern','cadmpeg_ir='+str(lib),'-L','dependency='+str(ROOT/'target/debug/deps'),'-o',str(OUT/'probes')]
(OUT/'probe-build.command').write_text(' '.join(cmd)+'\n')
for name,cmd in [('probe-build',cmd),('probes',[str(OUT/'probes'),'--nocapture','--test-threads=1'])]:
 with (OUT/(name+'.log')).open('w') as log:r=subprocess.run(cmd,stdout=log,stderr=subprocess.STDOUT)
 (OUT/(name+'.exit')).write_text(str(r.returncode)+'\n')
 print((OUT/(name+'.log')).read_text());print(name,'exit',r.returncode)
 if r.returncode:raise SystemExit(r.returncode)
