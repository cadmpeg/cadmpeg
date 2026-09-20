# Run from the repository root; requires tree-sitter, tree-sitter-rust, rustc, and a cached cadmpeg-ir rlib.
from pathlib import Path
import sys,re,json,hashlib,subprocess,tempfile
from tree_sitter import Language,Parser
import tree_sitter_rust
parser=Parser(Language(tree_sitter_rust.language()))
ROOT=Path.cwd();OUT=Path(tempfile.mkdtemp(prefix='cadmpeg-seventh-audit-'));manifest=[]
print('Evidence directory:',OUT,flush=True)
def source(p):
 p=ROOT/'crates'/p;b=p.read_bytes();manifest.append(dict(path=str(p.relative_to(ROOT)),sha256=hashlib.sha256(b).hexdigest()));return b.decode()
def fn(s,name):
 b=s.encode();stack=[parser.parse(b).root_node]
 while stack:
  n=stack.pop()
  if n.type=='function_item' and n.child_by_field_name('name').text.decode()==name:return re.sub(r'^pub(?:\([^)]*\))?\s+','',b[n.start_byte:n.end_byte].decode())+'\n'
  stack.extend(reversed(n.children))
 raise ValueError(name)
def consts(s):return '\n'.join(re.findall(r'(?m)^const [A-Z][A-Z0-9_]*: f64 = [^;]+;',s))+'\n'
s='''#![allow(dead_code,unused_imports,unused_variables)]
mod math {pub use cadmpeg_ir::math::*;
'''+f'#[path="{OUT}/sum.rs"]pub mod sum;'+'''}
use math::*;
use cadmpeg_ir::geometry::{CurveGeometry,SolvedCurveGeometry};
use cadmpeg_ir::geometry::analytic::CircleCurve;
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::geometry::pcurve::{PcurveGeometry,PcurveNurbs,PcurveNurbsPoles};
use cadmpeg_ir::transform::Transform;
'''
(OUT/'sum.rs').write_text(source('cadmpeg-ir/src/math/sum.rs').replace('#[cfg(test)]\nmod tests;', ''))
f=source('cadmpeg-codec-freecad/src/topology_transfer.rs')
s+='mod fc_range {use super::*;'+consts(f)+fn(f,'normalize_pcurve_parameter_range')+'''
#[test]fn exact_small_domain_collapses(){let n=PcurveNurbs::new(1,vec![0.,0.,1e-10,1e-10],PcurveNurbsPoles::from_lanes(vec![Point2::new(0.,0.),Point2::new(1.,0.)],None).unwrap(),false).unwrap();let g=PcurveGeometry::Nurbs{nurbs:n};let r=normalize_pcurve_parameter_range(&g,Some([0.,1e-10]));println!("FreeCAD exact domain [0,1e-10]: {r:?}");assert_eq!(r,Some([0.,0.]));}}
'''
f=source('cadmpeg-codec-freecad/src/brep.rs')
s+='mod fc_periodic {#[derive(Debug)]enum CodecError{Malformed(String)}'+fn(f,'normalize_periodic_knots')+'''
#[test]fn finite_periodic_extension_overflows(){let (knots,pad)=normalize_periodic_knots(vec![-1e308,-9e307,9e307,1e308],1,true).unwrap();println!("FreeCAD periodic knots: {knots:?}, padding {pad}");assert_eq!(pad,1);assert_eq!(knots[0],f64::NEG_INFINITY);assert_eq!(knots[5],f64::INFINITY);let expected:f64=9e307-1e308-1e308;assert!(expected.is_finite());}}
'''
f=source('cadmpeg-codec-f3d/src/design/feature_project.rs')
s+='mod fusion {use super::*;'+consts(f)+fn(f,'matrix_axis_angle')+'''
#[test]fn representable_rotation_is_discarded(){let a=1e-8_f64;let(s,c)=a.sin_cos();let m=[[c,-s,0.,0.],[s,c,0.,0.],[0.,0.,1.,0.],[0.,0.,0.,1.]];assert!(Transform::affine([m[0],m[1],m[2]]).unwrap().is_proper_rigid());let result=matrix_axis_angle(&m);println!("Fusion 1e-8-radian rotation: {result:?}");assert!(result.is_none());assert!(s.atan2(c)>EPS_FEATURE_PROJECT_MATRIX_AXIS_ANGLE_E12);}}
'''
f=source('cadmpeg-ir/src/math/planar.rs')
s+='mod planar {use super::*;'+fn(f,'scaled_displacement')+fn(f,'circle_intersections')+'''
#[test]fn unequal_circles_intersect_at_wrong_center(){let points=circle_intersections(Point2::new(0.,0.),1.,Point2::new(1e200,0.),1e200).unwrap();println!("IR unequal circles: {points:?}");assert_eq!(points,vec![Point2::new(0.,0.)]);assert_eq!(points[0].u.hypot(points[0].v),0.);}}
'''
f=source('cadmpeg-ir/src/eval.rs')
s+='mod eval {use super::*;'+''.join(fn(f,n)for n in ['offset','scale_vector','vector_sum','unit_vector_with_derivative','linear_sweep_rail_point','linear_sweep_rail_vector'])+'''
#[test]fn parallel_derivative_is_refused(){let v=Vector3::new(1.,1.,1.);let d=Vector3::new(f64::MAX,f64::MAX,f64::MAX);let r=unit_vector_with_derivative(v,d);println!("IR parallel normalized derivative: {r:?}");assert!(r.is_none());assert!(unit_vector_with_derivative(Vector3::new(f64::MAX,f64::MAX,0.),Vector3::new(0.,0.,0.)).is_none());}
#[test]fn proper_rotation_produces_nonfinite_rail(){let a=2./3.;let b=-1./3.;let basis=[Vector3::new(b,a,a),Vector3::new(a,b,a),Vector3::new(a,a,b)];let transform=Transform::affine([[b,a,a,0.],[a,b,a,0.],[a,a,b,0.]]).unwrap();assert!(transform.is_proper_rigid());let point=Point3::new(f64::MAX,f64::MAX,f64::MAX);let p=linear_sweep_rail_point(basis,point);let v=linear_sweep_rail_vector(basis,Vector3::new(point.x,point.y,point.z));let checked=transform.apply_point(point);println!("IR rail {p:?}; vector {v:?}; checked transform {checked:?}");assert!(!p.is_finite());assert!(!v.is_finite());assert!(checked.unwrap().is_finite());}}
'''
f=source('cadmpeg-codec-creo/src/decode/analytic/equations.rs');a=f.index('#[derive(Clone, Copy)]');b=f.index('impl CarrierEquation',a);types=re.sub(r'pub\([^)]*\)','pub',f[a:b]);v=source('cadmpeg-codec-creo/src/vecmath.rs');p=source('cadmpeg-codec-creo/src/decode/analytic/planes.rs')
s+='mod creo {use super::*;'+consts(p)+types+''.join(fn(v,n)for n in ['dot','cross','normalize','normalize_with_length'])+''.join(fn(p,n)for n in ['point_on_carrier','tangent_sphere_point','tangent_plane_sphere_point'])+fn(f,'intersect_plane_with_circle')+'''
fn sphere(center:[f64;3],radius:f64)->SphereEquation{SphereEquation{center,radius,ref_direction:[1.,0.,0.]}}
#[test]fn tiny_plane_circle_returns_center(){let points=intersect_plane_with_circle(PlaneEquation{normal:[1.,0.,0.],origin:[0.;3]},[0.;3],[0.,0.,1.],1e-7);println!("Creo plane-circle r=1e-7: {points:?}");assert_eq!(points,vec![[0.;3]]);}
#[test]fn disjoint_tiny_spheres_invent_tangent(){let a=sphere([0.;3],1e-10);let b=sphere([3e-10,0.,0.],1e-10);let p=tangent_sphere_point(a,b).unwrap();println!("Creo disjoint tiny spheres: {p:?}");assert_eq!(p,[1.5e-10,0.,0.]);assert!(point_on_carrier(p,CarrierEquation::Sphere(a)));assert!(point_on_carrier(p,CarrierEquation::Sphere(b)));let plane=PlaneEquation{normal:[1.,0.,0.],origin:[0.;3]};let sphere=sphere([3e-10,0.,0.],1e-10);let p=tangent_plane_sphere_point(plane,sphere).unwrap();println!("Creo disjoint tiny plane-sphere: {p:?}");assert!(point_on_carrier(p,CarrierEquation::Sphere(sphere)));}
#[test]fn large_sphere_point_is_refused(){let s=sphere([0.;3],1e200);assert!(!point_on_carrier([1e200,0.,0.],CarrierEquation::Sphere(s)));println!("Creo exact point on radius-1e200 sphere refused");}}
'''
f=source('cadmpeg-asm/src/brep/geometry.rs')
s+='mod asm {use super::*;'+consts(f)+fn(f,'linear_nurbs_spine')+fn(f,'point_vector')+'use cadmpeg_ir::geometry::nurbs::knots_nondecreasing;' +'''
#[test]fn curved_spine_becomes_line(){let curve=NurbsCurve::from_lanes(2,vec![0.,0.,0.,1.,1.,1.],vec![Point3::new(0.,0.,0.),Point3::new(5e-11,4e-11,0.),Point3::new(1e-10,0.,0.)],None,false).unwrap();let result=linear_nurbs_spine(&curve);let midpoint=cadmpeg_ir::eval::curve_point(&CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve.clone())),0.5).unwrap();println!("ASM curved spine: {result:?}, midpoint {midpoint:?}");assert!(result.is_some());assert!(midpoint.y>1e-11);}}
'''
f=source('cadmpeg-codec-catia/src/nurbs.rs')
s+='mod nurbs {use super::*;'+consts(f)+fn(f,'canonical_periodic_range').replace('fn canonical_periodic_range','pub(crate) fn canonical_periodic_range')+'}\n'
f=source('cadmpeg-codec-catia/src/families/standard/decode.rs');a=source('cadmpeg-codec-catia/src/assemble.rs')
s+='mod catia {use super::*;'+consts(f)+fn(a,'unwrap_angle')+''.join(fn(f,n)for n in ['witness_arc_end','standard_analytic_curve_angle','standard_analytic_curve_parameter_range'])+'''
#[test]fn short_witnessed_arc_becomes_full_circle(){let g=CurveGeometry::Solved(SolvedCurveGeometry::Circle(CircleCurve::try_new(Point3::new(0.,0.,0.),Vector3::new(0.,0.,1.),Vector3::new(1.,0.,0.),1.).unwrap()));let at=|a:f64|Point3::new(a.cos(),a.sin(),0.);let range=standard_analytic_curve_parameter_range(&g,at(0.),at(0.001),Some(at(0.0005)));println!("CATIA witnessed 0.001-radian arc: {range:?}");assert_eq!(range,Some([0.,std::f64::consts::TAU]));}}
'''
exec(Path(__file__).with_name('extra.inc').read_text())
(OUT/'probes.rs').write_text(s)
lib=max((ROOT/'target/debug/deps').glob('libcadmpeg_ir*.rlib'),key=lambda p:p.stat().st_mtime)
manifest.append(dict(cached_library=str(lib.relative_to(ROOT)),sha256=hashlib.sha256(lib.read_bytes()).hexdigest()))
(OUT/'probe-sources.json').write_text(json.dumps(manifest,indent=2)+'\n')
cmd=['rustc','--edition=2021','--test',str(OUT/'probes.rs'),'--extern','cadmpeg_ir='+str(lib),'-L','dependency='+str(ROOT/'target/debug/deps'),'-o',str(OUT/'probes')]
for name,command in [('probe-build',cmd),('probes',[str(OUT/'probes'),'--nocapture','--test-threads=1'])]:
 (OUT/(name+'.command.json')).write_text(json.dumps([x.replace(str(ROOT),'$REPO_ROOT').replace(str(OUT),'$EVIDENCE_DIR') for x in command],indent=2)+'\n')
 with (OUT/(name+'.log')).open('w')as log:r=subprocess.run(command,stdout=log,stderr=subprocess.STDOUT)
 (OUT/(name+'.exit')).write_text(str(r.returncode)+'\n');print((OUT/(name+'.log')).read_text());print(name,'exit',r.returncode)
 if r.returncode:raise SystemExit(r.returncode)
