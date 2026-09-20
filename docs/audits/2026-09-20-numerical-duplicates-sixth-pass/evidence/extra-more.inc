f=source('cadmpeg-codec-f3d/src/design/dimensions.rs')
s+='mod f3d {use super::*;'+fn(f,'reflect_point')+'''
#[test]fn reflection_axis_length_overflow(){let got=reflect_point(Point2::new(1e200,1.),Point2::new(0.,0.),Point2::new(1e200,0.)).unwrap();println!("F3D reflect (1e200,1) in X axis: {got:?}; expected (1e200,-1)");assert!(!got.is_finite());}
#[test]fn reflection_finite_fixed_point(){let p=Point2::new(1e308,1.);let got=reflect_point(p,Point2::new(1e308,0.),Point2::new(1e308,1.)).unwrap();println!("F3D reflect point on axis x1e308: {got:?}; expected fixed point {p:?}");assert!(!got.is_finite());}
}
'''
f=source('cadmpeg-codec-rhino/src/writer.rs')
s+="""mod rhino {use super::*;struct Vertex{point:Point3}struct Loop{coedges:Vec<usize>}struct WritableModel<\'a>{vertices:Vec<Vertex>,loops:Vec<Loop>,edges:Vec<(usize,usize)>,marker:std::marker::PhantomData<&\'a ()>}
impl WritableModel<'_>{fn endpoints(&self,i:usize)->(usize,usize){self.edges[i]}}
"""+fn(f,'planar_solid_orientation')+'''
fn tetra(t:f64)->WritableModel<'static>{let vertices=vec![(0.,0.,0.),(2.,0.,0.),(0.,1.,0.),(0.,0.,1.)].into_iter().map(|(x,y,z)|Vertex{point:Point3::new(x+t,y+t,z+t)}).collect();let mut edges=vec![];let mut loops=vec![];for face in [[0,2,1],[0,1,3],[0,3,2],[1,2,3]]{let first=edges.len();for j in 0..3{edges.push((face[j],face[(j+1)%3]));}loops.push(Loop{coedges:(first..first+3).collect()});}WritableModel{vertices,loops,edges,marker:std::marker::PhantomData}}
#[test]fn solid_orientation_lost_on_translation(){let original=planar_solid_orientation(&tetra(0.));let moved=planar_solid_orientation(&tetra(1e8));println!("Rhino outward tetra solid code original={original}, translated1e8={moved}; expected1 both");assert_eq!(original,1);assert_eq!(moved,0);}
}
'''
f=source('cadmpeg-codec-nx/src/decode/pcurves.rs')
s+='mod nx {use super::*;use cadmpeg_ir::geometry::pcurve::{PcurveGeometry,LinePcurve,HyperbolaPcurve,HyperbolicPcurve};'+fn(f,'reverse_analytic_pcurve_over_range')+'''
#[test]fn reversal_finite_line_origin(){let p=PcurveGeometry::Line(LinePcurve::try_new(Point2::new(0.,0.),Point2::new(0.5,0.)).unwrap());let got=reverse_analytic_pcurve_over_range(&p,[1e308,1.4e308]);println!("NX line reversal domain[1e308,1.4e308]: {got:?}; finite reversed origin1.2e308 direction-0.5");assert!(got.is_none());}
#[test]fn reversal_finite_hyperbolic_coefficients(){for p in [PcurveGeometry::Hyperbola(HyperbolaPcurve::try_new(Point2::new(0.,0.),Point2::new(1.,0.),Point2::new(0.,1.),1e-10,1e-10).unwrap()),PcurveGeometry::Hyperbolic(HyperbolicPcurve::try_new(Point2::new(0.,0.),Point2::new(1e-10,0.),Point2::new(0.,1e-10)).unwrap())]{let got=reverse_analytic_pcurve_over_range(&p,[359.,361.]);let finite=0.5*(720f64+1e-10_f64.ln()).exp();println!("NX hyperbolic reversal on[359,361]: {got:?}; coefficients ~{finite:e} finite");assert!(finite.is_finite());assert!(got.is_none());}}
}
'''
f=source('cadmpeg-ir/src/eval.rs')
s+='mod ir_offset {use super::*;const EPS_EVAL_FITTED_NURBS_OFFSET_CANDIDATE_E12:f64=1e-12;const EPS_EVAL_FITTED_NURBS_OFFSET_CANDIDATE_E9:f64=1e-9;'+fn(f,'fitted_nurbs_offset_candidate')+'''
#[test]fn perpendicular_offset_frames_admitted(){let a=[(Point2::new(0.,0.),Point2::new(1e200,0.)),(Point2::new(1.,0.),Point2::new(1e200,0.))];let b=[(Point2::new(0.,1.),Point2::new(0.,1e200)),(Point2::new(1.,1.),Point2::new(0.,1e200))];let got=fitted_nurbs_offset_candidate(a,b,1e-9);println!("IR perpendicular endpoint tangent frames produce offset {got:?}, should None");assert_eq!(got,Some(1.));}
}
'''
