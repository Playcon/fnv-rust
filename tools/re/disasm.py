#!/usr/bin/env python3
"""Disassemble functions of the decrypted executable, annotated.

Calls and jumps are labelled with the target's name from
decomp/functions.tsv, addresses that hold a string with the string, and
game/INI setting objects with the setting's name (decomp/settings.tsv,
made by tools/re/settings_map.py --all).
Much cheaper than a Ghidra round trip when the decompiler lost `this`
(ECX) or a register argument.

Usage: python3 tools/re/disasm.py <address> [<address> ...]

Environment:
  FNV_EXE     the decrypted executable (Steamless output; docs/RESEARCH_TOOLS.md)
              default: ../work/FalloutNV.unpacked.exe next to this repository
  FNV_DECOMP  a directory with functions.tsv / strings.tsv / settings.tsv from
              your own Ghidra export (default: ../Decompiling-FNV/decomp next to
              this repository, if any). Never commit it here.

Without the export it still works: strings and setting names are read from
the executable itself, functions are unnamed and 0x200 bytes are shown.
Strings Ghidra did not define are read from the executable too.
Needs: pip install capstone
"""

import csv
import os
import re
import struct
import sys

import capstone

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))  # the repository (tools/re/..)
EXE = os.environ.get("FNV_EXE", os.path.join(os.path.dirname(ROOT), "work", "FalloutNV.unpacked.exe"))
DECOMP = os.environ.get("FNV_DECOMP", os.path.join(os.path.dirname(ROOT), "Decompiling-FNV", "decomp"))


def load_pe(path):
    if not os.path.exists(path):
        sys.exit(f"{path}: not found. Set FNV_EXE to the Steamless-decrypted FalloutNV.exe.")
    data = open(path, "rb").read()
    pe = struct.unpack_from("<I", data, 0x3C)[0]
    nsec = struct.unpack_from("<H", data, pe + 6)[0]
    optsize = struct.unpack_from("<H", data, pe + 20)[0]
    base = struct.unpack_from("<I", data, pe + 24 + 28)[0]
    sections = []
    off = pe + 24 + optsize
    for i in range(nsec):
        _, vsize, va, rsize, raw = struct.unpack_from("<8sIIII", data, off + i * 40)
        sections.append((base + va, max(vsize, rsize), raw, rsize))
    return data, sections


def read(data, sections, addr, n):
    for va, size, raw, rsize in sections:
        if va <= addr < va + size:
            o = addr - va
            return data[raw + o : raw + min(o + n, rsize)]
    return b""


def cstring(data, sections, addr, limit=60):
    """The printable ASCII string at addr, if there is one of 4+ chars."""
    raw = read(data, sections, addr, 256)
    end = raw.find(b"\0")
    if end < 4:
        return None
    raw = raw[:end]
    if not all(32 <= b < 127 or b in (9, 10, 13) for b in raw):
        return None
    return raw.decode("ascii")[:limit]


def _rows(name):
    path = os.path.join(DECOMP, name)
    if not os.path.exists(path):
        return None
    with open(path, encoding="utf-8") as f:
        return list(csv.DictReader(f, delimiter="\t", quoting=csv.QUOTE_NONE))


def tables(data=None, sections=None):
    """(names, sizes, strings, settings). Missing export files give empty
    names/sizes; strings are then looked up in the executable on demand
    (see annotate), and settings are found by tools/re/settings_map.py."""
    names, sizes = {}, {}
    for r in _rows("functions.tsv") or []:
        a = int(r["address"], 16)
        ns = r["namespace"]
        names[a] = r["name"] if ns == "Global" else f"{ns}::{r['name']}"
        sizes[a] = int(r["size"])
    strings = {}
    for r in _rows("strings.tsv") or []:
        strings[int(r["address"], 16)] = r["value"][:60]
    settings = {}
    rows = _rows("settings.tsv")
    if rows is None and data is not None:
        import importlib.util

        spec = importlib.util.spec_from_file_location("settings_map", os.path.join(HERE, "settings_map.py"))
        sm = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(sm)
        rows = [{"object": f"{o:08x}", "value": f"{o + 4:08x}", "name": n}
                for o, n in sm.all_settings(data, sections).items()]
    for r in rows or []:
        settings[int(r["object"], 16)] = r["name"]
        settings[int(r["value"], 16)] = r["name"] + " (value)"
    return names, sizes, strings, settings


HEX = re.compile(r"0x([0-9a-f]{6,8})")


def main():
    data, sections = load_pe(EXE)
    names, sizes, strings, settings = tables(data, sections)
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    for arg in sys.argv[1:]:
        start = int(arg, 16)
        size = sizes.get(start, 0x200)
        print(f"; {start:08x} {names.get(start, '?')} ({size} bytes)")
        for ins in md.disasm(read(data, sections, start, size), start):
            notes = []
            for m in HEX.finditer(ins.op_str):
                v = int(m.group(1), 16)
                if v in names and v != start:
                    notes.append(names[v])
                elif v in strings:
                    notes.append(repr(strings[v]))
                elif v in settings:
                    notes.append(settings[v])
                elif text := cstring(data, sections, v):
                    notes.append(repr(text))
            note = ("  ; " + ", ".join(notes)) if notes else ""
            print(f"{ins.address:08x}  {ins.mnemonic:6} {ins.op_str}{note}")
        print()


if __name__ == "__main__":
    try:
        main()
    except BrokenPipeError:  # piped into head
        sys.stderr.close()
