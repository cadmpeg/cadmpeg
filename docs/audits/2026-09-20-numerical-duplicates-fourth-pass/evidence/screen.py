from pathlib import Path
import json,re
out=Path('/tmp/cadmpeg-numeric-audit-4'); fs=json.loads((out/'functions.json').read_text()); cache={}; by={}
pat=re.compile(r'\.sqrt\(|\.powi\(2\)|\.acos\(|\.asin\(|\.round\(\)\s+as|/\s*\([^\n]+\*|\.exp\(|\.ln\(|\.sinh\(|\.cosh\(|rem_euclid|fn (?:distance|norm|unit|intersect|.*match|.*agree|.*close)')
for f in fs:
 s=cache.setdefault(f['path'],Path(f['path']).read_bytes()); body=s[f['start']:f['end']].decode()
 hits=[(f['line']+n,l.strip()) for n,l in enumerate(body.splitlines()) if pat.search(l)]
 if hits:by.setdefault(f['crate'],[]).append((f,hits))
for c,v in by.items():
 (out/(c+'-numeric.txt')).write_text('\n'.join(f"{f['path']}:{f['line']} {f['name']}\n"+'\n'.join(f' {n}: {l}' for n,l in h) for f,h in v))
print({c:len(v) for c,v in by.items()})
