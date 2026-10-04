#!/usr/bin/env python3
"""Split the Ghidra export into one file per function, plus small indexes.

Input:  decomp/src/fnv_XXXX0000.c (ExportFNV.java's 64 KB chunks) and
        decomp/functions.tsv.
Output: decomp/fn/<hi4>/<address>.c        one function per file
        decomp/index/sources.tsv           original source file -> functions
        decomp/index/by_source/<path>.tsv  functions of one source file
        decomp/index/by_class/<Class>.tsv  functions in one class namespace
        decomp/functions.tsv               gains `source` and `evidence`

The original source file of a function is recovered from the debug paths
the game keeps for its asserts ("D:\\_Fallout3\\Platforms\\Common\\Code\\
...\\Actor.cpp"):

  string   the function references the path itself
  class    the function is in class namespace N and a path ends in N.cpp
  between  the function lies between two functions of the same source file
           (MSVC links each object file's code contiguously), at most
           GAP bytes apart

Everything else is left unassigned. `between` is an inference; `string`
and `class` are direct.

Usage: python3 tools/re/split_decomp.py <decomp-dir>   (your export; never commit it)
"""

import csv
import os
import re
import shutil
import sys
from collections import Counter, defaultdict

GAP = 0x2000
HEADER = re.compile(r"^/\* ([0-9a-f]{8})  (.+) \*/$")
PATH = re.compile(r'"D:\\\\_Fallout3\\\\Platforms\\\\Common\\\\Code\\\\([^"]+?\.cpp)"', re.I)
UNSAFE = re.compile(r'[<>:"/\\|?*\s,`\']+')


def safe(name):
    return UNSAFE.sub("_", name).strip("_")[:120] or "_"


def source_path(raw):
    # Normalise the separators and the case of the directories, which the
    # paths aren't consistent about ("Fallout/AI", "Fallout/ai", "Fallout/Ai").
    parts = raw.replace("\\\\", "/").split("/")
    dirs = [p.lower().replace(" ", "_") for p in parts[:-1]]
    return "/".join(dirs + [parts[-1][:-4]])


def read_functions(src):
    """address -> (name, text) from the chunk files."""
    out = {}
    for chunk in sorted(os.listdir(src)):
        if not chunk.endswith(".c"):
            continue
        with open(os.path.join(src, chunk), encoding="utf-8", errors="replace") as f:
            lines = f.read().split("\n")
        cur, name, body = None, None, []
        for line in lines:
            m = HEADER.match(line)
            if m:
                if cur:
                    out[cur] = (name, "\n".join(body).strip("\n") + "\n")
                cur, name, body = m.group(1), m.group(2), [line]
            elif cur:
                body.append(line)
        if cur:
            out[cur] = (name, "\n".join(body).strip("\n") + "\n")
    return out


def main():
    root = sys.argv[1] if len(sys.argv) > 1 else "decomp"
    src = os.path.join(root, "src")
    funcs = read_functions(src)

    with open(os.path.join(root, "functions.tsv"), encoding="utf-8") as f:
        rows = list(csv.DictReader(f, delimiter="\t", quoting=csv.QUOTE_NONE))
    rows.sort(key=lambda r: int(r["address"], 16))

    # Direct evidence: the function names the path.
    direct = {}
    for addr, (_, text) in funcs.items():
        paths = Counter(source_path(p) for p in PATH.findall(text))
        if paths:
            direct[addr] = paths.most_common(1)[0][0]
    by_base = defaultdict(set)
    for p in set(direct.values()):
        by_base[p.rsplit("/", 1)[-1].lower()].add(p)

    source, evidence = {}, {}
    for r in rows:
        a, ns = r["address"], r["namespace"]
        if a in direct:
            source[a], evidence[a] = direct[a], "string"
        elif ns != "Global" and len(by_base.get(ns.lower(), ())) == 1:
            source[a], evidence[a] = next(iter(by_base[ns.lower()])), "class"

    # Fill the runs between two functions of the same source file.
    known = [r["address"] for r in rows if r["address"] in source]
    order = {r["address"]: i for i, r in enumerate(rows)}
    for a, b in zip(known, known[1:]):
        if source[a] != source[b] or int(b, 16) - int(a, 16) > GAP:
            continue
        for r in rows[order[a] + 1 : order[b]]:
            source[r["address"]], evidence[r["address"]] = source[a], "between"

    # One file per function.
    fn = os.path.join(root, "fn")
    shutil.rmtree(fn, ignore_errors=True)
    missing = 0
    for r in rows:
        a = r["address"]
        if a not in funcs:
            missing += 1
            continue
        d = os.path.join(fn, a[:4])
        os.makedirs(d, exist_ok=True)
        with open(os.path.join(d, a + ".c"), "w", encoding="utf-8") as f:
            f.write(funcs[a][1])

    # Indexes.
    index = os.path.join(root, "index")
    shutil.rmtree(index, ignore_errors=True)
    cols = ["address", "size", "name", "evidence"]
    groups = defaultdict(list)
    classes = defaultdict(list)
    for r in rows:
        a = r["address"]
        r["source"], r["evidence"] = source.get(a, ""), evidence.get(a, "")
        if r["source"]:
            groups[r["source"]].append(r)
        if r["namespace"] != "Global":
            classes[r["namespace"]].append(r)

    def write(path, items, columns):
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "w", encoding="utf-8", newline="") as f:
            w = csv.writer(f, delimiter="\t", lineterminator="\n", quoting=csv.QUOTE_NONE, escapechar="\\")
            w.writerow(columns)
            for it in items:
                w.writerow([it.get(c, "") for c in columns])

    summary = []
    for path, items in sorted(groups.items()):
        write(os.path.join(index, "by_source", path + ".tsv"), items, cols)
        n = Counter(i["evidence"] for i in items)
        summary.append({
            "source": path, "functions": len(items),
            "string": n["string"], "class": n["class"], "between": n["between"],
            "first": items[0]["address"], "last": items[-1]["address"],
        })
    write(os.path.join(index, "sources.tsv"), summary,
          ["source", "functions", "string", "class", "between", "first", "last"])
    for ns, items in sorted(classes.items()):
        write(os.path.join(index, "by_class", safe(ns) + ".tsv"), items, cols + ["source"])

    head = list(rows[0].keys())
    write(os.path.join(root, "functions.tsv"), rows, head)

    assigned = sum(1 for r in rows if r["source"])
    print(f"{len(funcs)} functions split ({missing} listed without a body); "
          f"{len(groups)} source files, {assigned} functions assigned; "
          f"{len(classes)} classes")


if __name__ == "__main__":
    main()
