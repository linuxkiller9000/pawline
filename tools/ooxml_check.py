#!/usr/bin/env python3
"""Structural OOXML check for Pawline .pptx files. Validation only: it is NOT PowerPoint or Google Slides.
Checks: zip integrity, every part is well-formed XML (embedded .xlsx are checked recursively), [Content_Types] covers
every part, every relationship target exists, no duplicate zip names, slide/shape ids unique per slide,
every r:id / r:embed used in a part is declared in its .rels file. Exit 1 on any problem."""
import sys, zipfile, io, posixpath, re
import xml.etree.ElementTree as ET

R = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
CT = "{http://schemas.openxmlformats.org/package/2006/content-types}"
PR = "{http://schemas.openxmlformats.org/package/2006/relationships}"

def check(data, name, errs):
    try:
        z = zipfile.ZipFile(io.BytesIO(data))
    except Exception as e:
        errs.append(f"{name}: not a zip: {e}"); return
    if z.testzip() is not None: errs.append(f"{name}: bad zip entry {z.testzip()}")
    names = z.namelist()
    if len(set(names)) != len(names): errs.append(f"{name}: duplicate part names")
    xml = {}
    for n in names:
        if n.endswith((".xml", ".rels")):
            try: xml[n] = ET.fromstring(z.read(n))
            except Exception as e: errs.append(f"{name}:{n}: XML not well-formed: {e}")
    if "[Content_Types].xml" not in xml: errs.append(f"{name}: no [Content_Types].xml"); return
    defaults = {e.get("Extension").lower() for e in xml["[Content_Types].xml"].iter(CT + "Default")}
    overrides = {e.get("PartName") for e in xml["[Content_Types].xml"].iter(CT + "Override")}
    for n in names:
        if n == "[Content_Types].xml": continue
        if "/" + n not in overrides and n.rsplit(".", 1)[-1].lower() not in defaults:
            errs.append(f"{name}:{n}: no content type")
    for o in overrides:
        if o.lstrip("/") not in names: errs.append(f"{name}: override for missing part {o}")
    for n, root in xml.items():
        if not n.endswith(".rels"): continue
        base = posixpath.dirname(posixpath.dirname(n))
        ids = set()
        for r in root.iter(PR + "Relationship"):
            if r.get("Id") in ids: errs.append(f"{name}:{n}: duplicate rel id {r.get('Id')}")
            ids.add(r.get("Id"))
            if r.get("TargetMode") == "External": continue
            t = r.get("Target")
            p = t.lstrip("/") if t.startswith("/") else posixpath.normpath(posixpath.join(base, t))
            if p not in names: errs.append(f"{name}:{n}: target missing {t}")
        part = posixpath.join(base, posixpath.basename(n)[:-5]) if base or True else ""
        part = posixpath.normpath(posixpath.join(base, posixpath.basename(n)[:-5])) if n != "_rels/.rels" else None
        if part and part in xml:
            used = set(re.findall(r'r:(?:id|embed|link)="([^"]+)"', z.read(part).decode("utf8", "replace")))
            for u in used - ids: errs.append(f"{name}:{part}: uses undeclared relationship {u}")
    for n, root in xml.items():
        if re.match(r"ppt/slides/slide\d+\.xml$", n):
            seen = []
            for e in root.iter():
                if e.tag.endswith("}cNvPr"): seen.append(e.get("id"))
            if len(seen) != len(set(seen)): errs.append(f"{name}:{n}: duplicate shape ids {seen}")
    for n in names:
        if n.endswith(".xlsx"): check(z.read(n), f"{name}:{n}", errs)
    return len(names)

if __name__ == "__main__":
    bad = 0
    for f in sys.argv[1:]:
        errs = []; n = check(open(f, "rb").read(), f, errs)
        print(f"{f}: {n} parts, {len(errs)} problem(s)")
        for e in errs: print("  -", e)
        bad += len(errs)
    sys.exit(1 if bad else 0)
