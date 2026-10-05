#!/usr/bin/env python3
"""Map game setting names to the address of their Setting object.

Static initialisers construct each setting as
    push <default>; push <name>; mov ecx, <object>; call <ctor>
(either order of the pushes and the mov). The value lives at object + 4,
which is what the code reads: grep your decompiled code for DAT_<object+4>.

Usage: python3 tools/re/settings_map.py <regex on the setting name>
       python3 tools/re/settings_map.py --all [out.tsv]
           writes every setting to out.tsv (default: $FNV_DECOMP/settings.tsv),
           which tools/re/disasm.py reads
FNV_EXE / FNV_DECOMP as for tools/re/disasm.py.
"""
import importlib.util
import os
import re
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("disasm", os.path.join(HERE, "disasm.py"))
d = importlib.util.module_from_spec(spec)
spec.loader.exec_module(d)

# A setting's name: a type prefix, then a capital, optionally ":Section".
NAME = re.compile(r"^[sfibu][A-Z][A-Za-z0-9_]*(:[A-Za-z0-9 _]+)?$")


def cstr(data, secs, va):
    for start, size, raw, rsize in secs:
        if start <= va < start + rsize:
            o = raw + va - start
            end = data.find(b"\0", o, o + 128)
            if end < 0:
                return None
            try:
                return data[o:end].decode("ascii")
            except UnicodeDecodeError:
                return None
    return None


def all_settings(data, secs):
    """object address -> setting name, from every `push name` with a
    `mov ecx, object` next to it."""
    out = {}
    for m in re.finditer(rb"\x68(....)", data, re.S):
        a = struct.unpack("<I", m.group(1))[0]
        if not 0x1000000 <= a < 0x1200000:
            continue
        name = cstr(data, secs, a)
        if not name or not NAME.match(name):
            continue
        o = m.start()
        window = data[max(0, o - 16) : o + 24]
        for k in range(len(window) - 4):
            if window[k] == 0xB9:
                obj = struct.unpack_from("<I", window, k + 1)[0]
                if 0x1100000 <= obj < 0x1300000:
                    out.setdefault(obj, name)
                    break
    return out


def main():
    data, secs = d.load_pe(d.EXE)
    found = all_settings(data, secs)
    if sys.argv[1:2] == ["--all"]:
        path = sys.argv[2] if len(sys.argv) > 2 else os.path.join(d.DECOMP, "settings.tsv")
        os.makedirs(os.path.dirname(os.path.abspath(path)), exist_ok=True)
        with open(path, "w", encoding="utf-8") as f:
            f.write("object\tvalue\tname\n")
            for obj, name in sorted(found.items()):
                f.write(f"{obj:08x}\t{obj + 4:08x}\t{name}\n")
        print(f"{len(found)} settings -> {path}")
        return
    pattern = re.compile(sys.argv[1])
    by_name = {}
    for obj, name in found.items():
        if pattern.search(name):
            by_name.setdefault(name, []).append(obj)
    for name in sorted(by_name):
        objs = ", ".join(f"{o:08x} (value DAT_{o + 4:08x})" for o in sorted(by_name[name]))
        print(f"{name}\t{objs}")


if __name__ == "__main__":
    main()
