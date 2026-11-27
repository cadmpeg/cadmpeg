# SPDX-License-Identifier: Apache-2.0
"""Synthetic stored Spreadsheet cells for the FreeCAD expression oracle."""

from itertools import product
from xml.etree import ElementTree as ET


UNITS = """
nm um µm μm mm cm dm m km in " ft ' thou mil yd mi sqft cft l ml
ug µg μg mg g kg t lb lbm oz st cwt s min h Hz kHz MHz GHz THz mph
mN N kN MN lbf Pa kPa MPa GPa bar mbar Torr mTorr uTorr µTorr μTorr psi ksi Mpsi
nW uW µW μW mW W VA kW mJ J Nm VAs CV Ws kJ kWh eV keV MeV cal kcal
° deg rad gon M ′ AS ″ A nA uA µA μA mA kA MA K mK uK µK μK mol nmol
umol µmol μmol mmol cd V kV mV MS kS S mS uS µS μS Ohm kOhm MOhm
C T mT G Wb F mF uF µF μF nF pF H mH uH µH μH nH
""".split()

V = "vector(1; 2; 3)"
X = "vector(1; 0; 0)"
Y = "vector(0; 1; 0)"
M = "matrix(1)"
COLLECTORS = "sum average stddev min max count and or".split()


def calls():
    groups = {
        "cos sin tan": ("15 deg", "2 mm"),
        "acos asin atan exp log log10 sinh tanh cosh": ("0.5", "2 mm"),
        "ceil floor abs": ("2.5 mm", "1 mm / 0"),
        "round trunc": ("2.5 mm", "None"),
        "sqrt": ("4 mm^2", "2 mm"),
        "cbrt": ("8 mm^3", "2 mm"),
        "atan2 hypot cath": ("3 mm; 4 mm", "2 mm; 1 s"),
        "mod": ("3 mm; 2 mm", "3 mm; 0"),
        "pow": ("4 mm; 2", "16 mm^4; 0.25"),
        "not": ("0", "2 mm"),
        "rotationx rotationy rotationz": ("30 deg", "2 mm"),
        "translationm": (V, "1 mm; 2; 3 mm", "tuple(1; 2; 3)", "list(1; 2; 3)", "1"),
        "list tuple": ("1; 2 mm", "A1:A2", "A1:A2; 3", "1 / 0"),
        "href hiddenref": ("A1", "A1:A2", "1 / 0"),
        "str": ("1 mm", "None", "list(1)", "1 / 0"),
        "parsequant": ("<<2 mm>>", "<<1. mm>>", "<<2 mm/s>>", "<<mm>>", "<<>>", "<<bad>>", "1"),
        "address": ("1; 1", "1; 1; 1", "1; 1; 5", "1; 1; 2.5", "0; 1"),
        "vector": ("1; 2; 3", "1; <<bad>>; 3", "1; 2"),
        "placement": (V, f"{V}; rotationx(0)", f"{V}; rotationx(0); {X}"),
        "matrix": ("1", "1; 2; 3", "; ".join(["1"] * 16)),
        "rotation": (f"{V}; 30 deg", "10; 20; 30"),
        "create": ("<<vector>>", "<<Vector>>; 1; 2; 3", "<<placement>>", "<<matrix>>",
                   "<<rotation>>", "<<bad>>", "1"),
        "minvert": (M, "create(<<placement>>)", "1"),
        "mrotate": (f"{M}; rotationx(0)", f"{M}; {V}; 1", f"{M}; 1; 2; 3"),
        "mrotatex mrotatey mrotatez": (f"{M}; 1", "1; 2"),
        "mscale mtranslate": (f"{M}; {V}", f"{M}; 1; 2; 3", "1; 2"),
        "vangle vdot vcross": (f"{V}; {X}", "1; 2"),
        "vnormalize": (V, "1"),
        "vscale": (f"{V}; 1; 2; 3", f"{V}; 1 mm; 2 mm; 3 mm", f"{V}; 1; 2 deg; 3"),
        "vscalex vscaley vscalez": (f"{V}; 2", f"{V}; 2 mm", f"{V}; 2 deg"),
        "vlinedist vplanedist vlinesegdist vlineproj vplaneproj": (
            f"{V}; {X}; {Y}", f"{X}; {X}; {X}", "1; 2; 3",
        ),
    }
    for names, arguments in groups.items():
        for name in names.split():
            for args in ("", *arguments):
                yield f"={name}({args})"
    yield "=hypot(2 mm; 3 mm; 4 mm)"
    yield "=cath(5 mm; 3 mm; 2 mm)"


def operators():
    operands = ("2", "2.5", "2 mm", "2 s", "True", "<<x>>", "None")
    for op in ("+", "-", "*", "/", "%", "^", "==", "!=", "<", "<=", ">", ">="):
        for left, right in product(operands, repeat=2):
            yield f"={left} {op} {right}"
    for sign, operand in product(("+", "-", "−"), (*operands, V)):
        yield f"={sign}{operand}"
    for op in ("+", "-", "*", "/", "%", "^", "==", "<"):
        for left, right in ((V, X), (V, "2"), ("2", V), (V, "2 mm")):
            yield f"=sum(({left}) {op} ({right}); 2 mm)"
    yield from (
        "=2 ^ 3 ^ 2", "=-2 ^ 2", "=1 / 0", "=1 % 0", "=1 mm % 0", "=1 mm / 0",
        "=0 ^ -1", "=-2 ^ 0.5", "=0.5 ^ -1e308", "=1e200 ^ 2", "=cos(0) % 2 mm",
        "=tuple(1; 2) == 1", "=None == 1", "=(2 > 1 ? 4 mm : 5)",
        "=(0 ? 1 / 0 : 3 mm)", "=(1 / 0 ? 4 mm : 5)",
        "=((1 / 0) == 1 ? 2 mm : 4 mm)", "=(B4 ? None : 1) == 1",
        "=sum(list(1; 2) + list(3); 2 mm)", "=sum(list(1) * True; 2 mm)",
        "=sum(list(1) * 2.5; 2 mm)", "=sum(list(1) * (2 ^ 64); 2 mm)",
        "=<<>> * (2 ^ 64)", "=<<x>> * B4", "=sum(<<x>> * B4; 2 mm)",
        "=<<x>> * average(2; 3)", "=<<x>> * average(A1; 4)", "=2 * <<x>>",
        "=<<x>> * -2", "=<<x>> * True", "=<<>> * Vars.WideInt",
        "=(Vars.JsonFloat == Vars.JsonFloat ? 4 mm : 5)",
        "=(Vars.JsonFloat != Vars.JsonFloat ? 4 mm : 5)",
        "=(A9 == A9 ? 4 mm : 5)", "=(0 mm / 0 ? 4 mm : 5)",
        "=(vector(1; 2; 3) * vector(4; 5; 6)) + 2",
        "=vangle(vector(1; 2; 3) + vector(1; 2; 3); vector(1; 0; 0))",
    )


def formats():
    for conversion in ("s", "r", "a", "d", "i", "u", "o", "x", "X", "e", "E", "f", "F",
                       "g", "G", "c", "z"):
        for value in ("1", "2.5", "<<x>>", "(1 mm / 0)", "None"):
            yield f"=sum(<<%{conversion}>> % {value}; 2 mm)"
    yield from (
        "=<<%s>> % Vars.Material.Width", "=<<%d>> % <<bad>>",
        "=<<%s %.2f>> % tuple(<<x>>; 2.5)", "=<<%s %%>> % <<x>>",
        "=<<%5.2f>> % 3.5", "=<<%*.*f>> % tuple(6; 2; 3.5)",
        "=<<%s>> % tuple(1; 2)", "=<<%>> % 1", "=<<ok>> % list(1)",
        "=sum(<<%d>> % (2 ^ 64); 2 mm)", "=<<%(x)s>> % Vars.Items",
    )


def lexemes():
    yield from (
        "=0", "=1", "=2.0", "=2,0", "=.5", "=,5", "=1.", "=5e-2", "=2E+4",
        "=1e309", "=1e-999", "=9223372036854775806", "=9223372036854775807 + 1",
        "=-9223372036854775808", "=0x1A", "=nan", "=inf", "=None", "=none",
        "=True", "=False", "=true", "=false", "=TRUE", "=fAlSe", "=pi", "=e", "=Pi",
        "=<<a>b>> + <<c>>", "=<<a\\>b>> + <<c>>", "=<<a\\nb>>",
        "=<<a\nb>>", "=<<a\\\nb>>", "=<<a\\tb>>", "=<<a\\\\b>>", "=<<a\\qb>>",
        "=<<aéμ🙂>>", "=3 +", "=1 ! 2", "=1 | 2", "=1\r+2", "=1\u00a0+2",
        "=cos (1)", "=cos\t(1)", "=cos\n(1)", "=COS(1)", "=cos(0; 0) == 1",
        "=sum(no_such_function(1); 2 mm)", "=cos(1, 2)", "=cos(1,2)",
        "=list(1; 2)[0] + 4 mm", "=mm", "=mm/mm", "=1 μm + 2 mm", "=1 µm + 2 mm",
        "=1 m m", "=1 m\tm", "=1 m\nm", "=1 m\u00a0m", "=1' 6\"",
        "=1\" 2\"", "=1' 6 mm", "=1 (-mm)", "=((2 mm))",
    )


def unit_arithmetic():
    yield from (
        "=1 (mm^8 / mm)", "=1 (mm^7 * mm / mm)", "=1 (mm^7 / mm)",
        "=1 (mm^-8 / mm * mm)", "=1 (mm^-8 * mm)", "=1 mm^8",
        "=1 mm^-9", "=1 mm^-8", "=1 mm^0", "=1 mm^+2", "=1 mm^1.0",
        "=1 mm^2^2", "=2 (mm/s)^2", "=2 (mm/s)^-2", "=1 / mm / s",
        "=2 mm/mm", "=2 mm/s/kg", "=2 mm / (1 s / 1 kg)", "=4 A / 2 A",
        "=2 mm / mm^2", "=1 mm + 2 cm", "=1 mm - 2 s",
        "=(2 mm)^2", "=(2 mm^2)^0.5", "=(2 mm)^0.5", "=(2 mm)^(2 A / 1 A)",
        "=pow(4 mm; 1 cal / 4.184 J)", "=pow(4 mm; 1 cal / 4.1868 J)",
        "=pow(4 mm; 1 kcal / 4186.8 J)", "=pow(4 mm^2; 0.5)",
        "=pow(4 mm; A1)", "=pow(4 mm; count(A1; 4))",
    )


def references():
    yield from (
        "=A1", "=abs(A1)", "=A1^2", "=str(A1)", "=VarSet.L", "=Params.Length", "=.A1",
        "=$A$1", "=a1", "=A$1", "=$A1", "=Sheet.A1", "=<<w>>", "=w", "=w + 2",
        "=Vars.K", "=Vars.Mode", "=Vars.Mode.String", "=Vars.Mode.Enum", "=Vars.Mode.All",
        "=Vars.SpringRate", "=Vars.Heat", "=Vars.Density", "=Vars.Path + <<suffix>>",
        "=Vars.Target == 1", "=Vars.Self == 1", "=Vars.Material.Label", "=Vars.Material.Mode",
        "=Vars . Material . Width", "=Vars.Material.Shape == 1", "=Stock.Shape.Length",
        "=Vars.Material.Placement.Base.x", "=<<Stock Label>>.Width",
        "=Vars.Flag == 1", "=Vars.Other", "=Vars.Current / 2 A", "=Vars.Toggle",
        "=Vars.JsonFloat", "=Vars.WideInt", "=Vars.Proxy", "=Vars.Items[1]",
        "=cos(Vars.Placement) == 1", "=cos(vector(1; 2; 3)) == 1", "=cos(Vars.Proxy) == 1",
        "=sum(Vars.Placement; 2 mm)", "=sum(vector(1; 2; 3); 2 mm)", "=sum(Vars.Proxy; 2 mm)",
        "=sum(Vars.Items; 2 mm)", "=sum(Vars.WideInt; 2 mm)", "=str(Vars.Words)",
        "=str(NullThing)", "=<<Missing>>#Sheet.A1", "=<<x>> * Vars.Count",
        "=<<x>> * Vars.Ratio", "=<<x>> * A1", "=<<x>> * count(A1; 2)",
        "=sum(create(<<vector>>); 2 mm)", "=sum(minvert(create(<<placement>>)); 2 mm)",
        "=sum(translationm(vector(1; 2; 3)); 2 mm)", "=sum(translationm(tuple(1; 2; 3)); 2 mm)",
        "=vangle(create(<<vector>>; 1; 2; 3); vector(1; 0; 0))",
        "=sum(create(<<vector>>; 1; <<bad>>; 3); 2 mm)",
        "=sum(A1; 2 mm)", "=sum(A4; 2 mm)", "=sum(A5; 2 mm)", "=sum(B3; 2 mm)",
        "=B6 == 1", "=cos(5 mm) == 1", "=sqrt(2 mm) == 1",
        "=stddev(5 mm) == 1", "=(1 / 0) == 1", "=stddev(2 mm^4; 3 mm^4)",
    )


def aggregates():
    for name, args in product(COLLECTORS, (
        "1; 2", "1; 2.5", "1 mm; 2 mm", "1 mm; 2 s", "A1:A2", "A1:A3",
        "A4:A5", "B1:B2", "B1; B2", "B6:B8", "A1:A2; 3", "True; 2",
        "<<x>>; 2 mm", "None; 2 mm", "tuple(1; 2); 2 mm", "1 / 0; 2 mm",
        "5 mm", "<<x>>", "Z1:Z2", "",
    )):
        yield f"={name}({args})"


def cases():
    fixed = (
        ("A1", "reference", "2", "w"), ("A2", "reference", "4", ""),
        ("A4", "literal", "2 mm^2", ""), ("A5", "literal", "1/0", ""),
        ("A6", "literal", "  ", ""), ("A7", "reference", "=1 / 0", ""),
        ("A8", "reference", "mixed", ""), ("A9", "literal", "nan", ""),
        ("B1", "lexeme", "=true", ""), ("B2", "lexeme", "=false", ""),
        ("B3", "reference", "=Stock.Shape.Length", ""), ("B4", "call", "=abs(2.5)", ""),
        ("B6", "reference", "=None", ""),
        ("B8", "reference", "3", ""),
        ("C1", "operator", "=<<x>> * (Vars.Huge - Vars.Huge)", ""),
    )
    yield from fixed
    families = {
        "literal": ("3 bolts", "-abc", "2*3", "10mm+5mm", "'2 mm", "'=1 + 1", "hello",
                    "1.25", "1,25", "0x1A", "0x1.8p1", "0x", "inf", "-Infinity", "NaN",
                    "nan(5)", "1e309", "1e-999", " \t", "\r", "1/2", "1 mm/2", "1/2 s",
                    "1 mm/2 s", "1/mm", "0/0", "1 mm/0", "1/0 mm", "2 mm", "-5 mm",
                    "+5 mm", "2 mm/s", "2 mm/mm", "1/2 mm^2", "2 mm^1", "1 mm / s^2",
                    "1 m m", "1 µm", "1 μm", "1' 6\"", "2 (mm)", "2 (mm/s)", "1,5 mm"),
        "lexeme": lexemes(),
        "unit": (f"=2 {unit}" for unit in dict.fromkeys(UNITS + [u.swapcase() for u in UNITS])),
        "unit arithmetic": unit_arithmetic(),
        "operator": operators(),
        "format": formats(),
        "call": calls(),
        "reference": references(),
        "aggregate": aggregates(),
    }
    for row, (family, content) in enumerate(
        ((family, content) for family, inputs in families.items() for content in inputs), 10
    ):
        yield f"Z{row}", family, content, ""


def properties(data, name, items):
    owner = ET.SubElement(data, "Object", name=name)
    props = ET.SubElement(owner, "Properties", Count=str(len(items)))
    for name, kind, carrier in items:
        prop = ET.SubElement(props, "Property", name=name, type=kind, group="Oracle", status="2097152")
        prop.extend(ET.fromstring(f"<carrier>{carrier}</carrier>"))


def document(rows):
    root = ET.Element("Document", SchemaVersion="4", FileVersion="1")
    ET.SubElement(root, "Properties", Count="0")
    kinds = {
        "Sheet": "Spreadsheet::Sheet", "Vars": "App::FeaturePython",
        "Stock": "Part::Feature", "VarSet": "App::FeaturePython", "Params": "App::FeaturePython",
    }
    objects = ET.SubElement(root, "Objects", Count=str(len(kinds)))
    for name, kind in kinds.items():
        ET.SubElement(objects, "Object", name=name, type=kind)
    data = ET.SubElement(root, "ObjectData", Count=str(len(kinds)))

    enum = ('<Integer value="0" CustomEnum="true"/><CustomEnumList count="2">'
            '<Enum value="one"/><Enum value="two"/></CustomEnumList>')
    properties(data, "Vars", [
        ("Mode", "App::PropertyEnumeration", enum),
        ("SpringRate", "App::PropertyStiffness", '<Float value="7"/>'),
        ("Heat", "App::PropertyHeatFlux", '<Float value="5"/>'),
        ("Density", "App::PropertyStiffnessDensity", '<Float value="3"/>'),
        ("Current", "App::PropertyElectricCurrent", '<Float value="4"/>'),
        ("Path", "App::PropertyPath", '<Path value="model"/>'),
        ("Width", "App::PropertyLength", '<Float value="4"/>'),
        ("Count", "App::PropertyInteger", '<Integer value="3"/>'),
        ("WideInt", "App::PropertyInteger", '<Integer value="2147483648"/>'),
        ("Ratio", "App::PropertyFloat", '<Float value="2.5"/>'),
        ("Toggle", "App::PropertyBool", '<Bool value="true"/>'),
        ("Words", "App::PropertyString", '<String value="x"/>'),
        ("Placement", "App::PropertyPlacement", '<PropertyPlacement Px="0" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/>'),
        ("Material", "App::PropertyLink", '<Link value="Stock"/>'),
        ("Target", "App::PropertyLink", '<Link value="Missing"/>'),
        ("Self", "App::PropertyLink", '<Link value="Vars"/>'),
        ("Flag", "App::PropertyPythonObject", '<Python value="dHJ1ZQ==" encoded="yes" json="yes"/>'),
        ("Other", "App::PropertyPythonObject", '<Python value="ZmFsc2U=" encoded="yes" json="yes"/>'),
        ("JsonFloat", "App::PropertyPythonObject", '<Python value="NaN" json="yes"/>'),
        ("Huge", "App::PropertyPythonObject",
         '<Python value="99999999999999999999999999999999999999999999999" json="yes"/>'),
        ("Items", "App::PropertyPythonObject", '<Python value="[1, 2]" json="yes"/>'),
    ])
    properties(data, "Stock", [
        ("Label", "App::PropertyString", '<String value="Stock Label"/>'),
        ("Shape", "Part::PropertyPartShape", '<Part file=""/>'),
        ("Width", "App::PropertyLength", '<Float value="7"/>'),
        ("Mode", "App::PropertyEnumeration", enum),
    ])
    properties(data, "VarSet", [("L", "App::PropertyLength", '<Float value="4"/>')])
    properties(data, "Params", [("Length", "App::PropertyLength", '<Float value="4"/>')])
    sheet = ET.SubElement(data, "Object", name="Sheet")
    props = ET.SubElement(sheet, "Properties", Count="1")
    prop = ET.SubElement(props, "Property", name="cells", type="Spreadsheet::PropertySheet")
    cells = ET.SubElement(prop, "Cells", Count=str(len(rows)))
    for address, _, content, alias in rows:
        cell = ET.SubElement(cells, "Cell", address=address, content=content)
        if alias:
            cell.set("alias", alias)
    ET.indent(root, space="  ")
    return ET.tostring(root, encoding="utf-8", xml_declaration=True) + b"\n"
