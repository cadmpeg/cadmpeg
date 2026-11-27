# SPDX-License-Identifier: CC0-1.0
"""scrub_cc0.py SRC DST: copy Alibre CC0 parts and assemblies and overwrite in place, at the same length:
the ACIS key string in SAB_DATA (-> X), every repository GUID (VERSION, CONSTITUENTS, STEP_DATA, even where
Part 21 line wrapping splits it; -> zeros), and every local time (STEP header time_stamp, STEP LOCAL_TIME +
CALENDAR_DATE + COORDINATED_UNIVERSAL_TIME_OFFSET, the SAB save date) -> UTC, rolling the date over when
the UTC time falls on the next or previous day, so no time zone remains. Prints what changed."""
import olefile, re, shutil, sys, os
from datetime import datetime, timedelta

src, dst = sys.argv[1:3]
SAB_DATE = re.compile(rb'(\x07\x18)(\w{3} \w{3} [ \d]\d \d\d:\d\d:\d\d \d{4})')
STAMP = re.compile(rb"(time_stamp \*/ ')(\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d)([+-]\d\d:\d\d)'")


def pad(old, new):
    """Same-length replacement for a Part 21 entity: spaces after the first '('."""
    assert len(new) <= len(old), (old, new)
    return new.replace(b'(', b'(' + b' ' * (len(old) - len(new)), 1)


def offset_hours(t):
    """UTC - local, in whole hours, from the STEP header stamp (0 if none)."""
    m = STAMP.search(t)
    if not m:
        return 0
    sign, hh, mm = m.group(3)[:1], int(m.group(3)[1:3]), int(m.group(3)[4:6])
    assert mm == 0, 'fractional-hour offset not handled'
    return hh if sign == b'-' else -hh


def step_utc(t, hours):
    """Rewrite the header stamp, LOCAL_TIME, CALENDAR_DATE and offsets to UTC at the same length."""
    def stamp(m):
        d = datetime.strptime(m.group(2).decode(), '%Y-%m-%dT%H:%M:%S') + timedelta(hours=hours)
        return m.group(1) + d.strftime('%Y-%m-%dT%H:%M:%S').encode() + b"+00:00'"
    t = STAMP.sub(stamp, t)
    # DATE_AND_TIME(#date,#time): shift each pair together so a day rollover moves its date
    ents = dict(re.findall(rb'#(\d+)\s*=\s*([A-Z_]+\([^;]*\));', t))
    edits = {}
    for date_id, time_id in re.findall(rb'DATE_AND_TIME\(\s*#(\d+)\s*,\s*#(\d+)\s*\)', t):
        dm = re.fullmatch(rb'CALENDAR_DATE\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*\)', ents.get(date_id, b''))
        tm = re.fullmatch(rb'LOCAL_TIME\(\s*(\d+)\s*,([^)]*)\)', ents.get(time_id, b''))
        if not (dm and tm):
            continue
        y, day, mon = (int(x) for x in dm.groups())
        when = datetime(y, mon, day, int(tm.group(1))) + timedelta(hours=hours)
        edits[date_id] = (ents[date_id], b'CALENDAR_DATE(%d,%d,%d)' % (when.year, when.day, when.month))
        edits[time_id] = (ents[time_id], b'LOCAL_TIME(%d,%s)' % (when.hour, tm.group(2)))
    for key, (old, new) in edits.items():
        t = re.sub(rb'(#' + key + rb'\s*=\s*)' + re.escape(old), lambda m, o=old, n=new: m.group(1) + pad(o, n), t)
    t = re.sub(rb'COORDINATED_UNIVERSAL_TIME_OFFSET\([^)]*\)',
               lambda m: pad(m.group(0), b'COORDINATED_UNIVERSAL_TIME_OFFSET(0,0,.AHEAD.)'), t)
    assert not re.search(rb'LOCAL_TIME\(', t) or hours == 0 or edits, 'LOCAL_TIME outside DATE_AND_TIME'
    return t


def sab_utc(s, hours):
    def sd(m):
        old = m.group(2).decode()
        d = datetime.strptime(old.replace('  ', ' '), '%a %b %d %H:%M:%S %Y') + timedelta(hours=hours)
        new = d.strftime('%a %b ') + ('%2d' % d.day) + d.strftime(' %H:%M:%S %Y')
        assert len(new) == len(old)
        return m.group(1) + new.encode()
    return SAB_DATE.sub(sd, s, count=1)


for n in sorted(os.listdir(src)):
    a, b = os.path.join(src, n), os.path.join(dst, n)
    shutil.copyfile(a, b)
    if n.endswith('.stp'):
        t = open(b, 'rb').read(); t2 = step_utc(t, offset_hours(t)); assert len(t2) == len(t)
        open(b, 'wb').write(t2); print(n, 'utc' if t2 != t else '-'); continue
    if not n.endswith(('.AD_PRT', '.AD_ASM')):
        continue
    o = olefile.OleFileIO(b, write_mode=True); ch = []
    st = o.openstream('STEP_DATA').read(); hours = offset_hours(st)
    guids = set(re.findall(rb'Repository Guid="([0-9A-F-]{36})"', o.openstream('VERSION').read()))

    def zero_guids(t):
        for g in guids:
            pat = rb'(?:\r?\n)?'.join(re.escape(bytes([c])) for c in g)
            t = re.sub(pat, lambda m: re.sub(rb'[0-9A-F]', b'0', m.group(0)), t)
        return t

    for name in ('VERSION', 'CONSTITUENTS', 'PROP_DATA'):
        if o.exists(name):
            c = o.openstream(name).read(); c2 = zero_guids(c); assert len(c2) == len(c)
            if c2 != c: o.write_stream(name, c2); ch.append('guid-' + name.lower())
    st2 = step_utc(zero_guids(st), hours); assert len(st2) == len(st)
    if st2 != st: o.write_stream('STEP_DATA', st2); ch.append('step')
    if o.exists('SAB_DATA'):
        s = o.openstream('SAB_DATA').read(); s2 = s
        m = re.search(rb'\x07(.)([A-Z0-9_]{40,})\r', s2, re.S)
        if m and m.group(1)[0] == len(m.group(2)):
            s2 = s2[:m.start(2)] + b'X' * len(m.group(2)) + s2[m.end(2):]; ch.append('key')
        s3 = sab_utc(s2, hours)
        if s3 != s2: ch.append('sab-date-utc')
        assert len(s3) == len(s); o.write_stream('SAB_DATA', s3)
    o.close(); print(n, ','.join(ch), 'offset_h', hours)
