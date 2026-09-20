from pathlib import Path
import re, subprocess, json, hashlib
root=Path('/tmp/cadmpeg-audit-fixes')
src=Path('crates/cadmpeg-codec-rhino/src/curves.rs').read_text()
def extract(name):
 m=re.search(r'(?m)^\s*(?:pub(?:\([^\n]*\))? )?fn '+name+r'\b',src);assert m,name
 a=src.index('{',m.start());d=1;i=a+1
 while d:d+=(src[i]=='{')-(src[i]=='}');i+=1
 return src[m.start():i].strip()
a=src.index('#[derive(Clone, Copy)]\nstruct Homogeneous');b=src.index('fn elevate_bezier',a)
s='''#![allow(dead_code,unused_imports)]
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::math::Point3;
use cadmpeg_core::decode::alloc_filled;
#[derive(Debug)]struct GeometryError(String);
impl GeometryError {fn malformed(_:usize,s:impl Into<String>)->Self{Self(s.into())}}
fn error(o:usize,s:&str)->GeometryError{GeometryError::malformed(o,s)}
mod loss {pub enum RhinoLossCode{PolycurveJoinGap}}
#[derive(Default)]struct Diagnostics(Vec<String>);
impl Diagnostics {fn new()->Self{Self::default()} fn push_coded(&mut self,_:loss::RhinoLossCode,s:String){self.0.push(s)} fn len(&self)->usize{self.0.len()}}
impl std::ops::Index<usize> for Diagnostics {type Output=String;fn index(&self,i:usize)->&String{&self.0[i]}}
struct NurbsJoin{curve:NurbsCurve,warnings:Diagnostics}
'''+src[a:b]
for name in ['remap_nurbs_domain','elevate_bezier','insert_knot_once','elevate_to_degree','join_nurbs_segments']:s+=extract(name)+'\n'
s+='\n#[cfg(test)]mod tests { use super::*;\n'
for name in ['numerical_audit_nurbs_elevation_preserves_active_spans_and_discontinuities','numerical_audit_join_preserves_independently_scaled_rational_segments','numerical_audit_remap_and_join_keep_large_finite_values','join_elevates_degree_and_midpoints_a_gap']:s+='#[test]\n'+extract(name)+'\n'
s+='}\n'
(root/'rhino-probes.rs').write_text(s)
libs={name:max(Path('target/debug/deps').glob('lib'+name+'-*.rlib'),key=lambda p:p.stat().st_mtime) for name in ['cadmpeg_ir','cadmpeg_core']}
command=['rustc','--edition=2021','--test',str(root/'rhino-probes.rs'),'-L','dependency=target/debug/deps','-o',str(root/'rhino-probes')]
for name,path in libs.items():command+=['--extern',name+'='+str(path)]
(root/'rhino-probes.command').write_text(' '.join(command)+'\n')
with (root/'rhino-probes-build.log').open('w') as log:r=subprocess.run(command,stdout=log,stderr=subprocess.STDOUT)
(root/'rhino-probes-build.exit').write_text(str(r.returncode)+'\n')
print((root/'rhino-probes-build.log').read_text())
if r.returncode==0:
 with (root/'rhino-probes.log').open('w') as log:r=subprocess.run([str(root/'rhino-probes')],stdout=log,stderr=subprocess.STDOUT)
 (root/'rhino-probes.exit').write_text(str(r.returncode)+'\n');print((root/'rhino-probes.log').read_text())
