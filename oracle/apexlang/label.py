#!/usr/bin/env python3
"""Label APEXlang reference edges from Oracle's own legacy SQL exports (estate#313 precision).

The APEXlang resolver binds `@slug` references by a curated property -> kind table plus a scope.
This oracle never reads that table. For each resolved edge it finds the SAME reference in Oracle's
legacy SQL export of the same application at the same commit (`<app>/sql/<app>.sql`, written by
APEX itself), where references are numeric component ids (`p_item_plug_id=>wwv_flow_imp.id(N)`),
and checks that the component Oracle points at is the component the resolver chose:

  owner   = the SQL record of the edge's source (same page, same static id / item name / page id)
  param   = the SQL parameter that stores the property (MAPPING below; written from the APEX
            export API, not from the resolver's table)
  truth   = the SQL record whose p_id is that parameter's id (or, for p_named_lov, whose name)
  correct = truth's component kind and static id equal the edge target's kind and key, a
            page-level truth is on the edge target's page, and a list entry's truth is in the
            edge target's list (the export file is per application, and the test asserts every
            edge stays inside its application)

An edge whose property has no SQL parameter here, or whose owner/target cannot be found in the
export, is `unlabelled` with the reason; it is counted, never dropped.

  label.py <corpus dir> <edges.tsv> <labels.tsv>

`<edges.tsv>` is the resolved-edge export of the `full_pinned_corpus_resolution` test
(`APEXLANG_EDGES=<file>`). Only `oracle-apex/` applications have SQL exports.
"""
import glob
import os
import re
import sys

# SQL create procedure -> APEXlang component kind, and the parameter holding the APEXlang key.
PROCS = {
    "create_page": ("page", "p_id"),
    "create_page_plug": ("region", "p_static_id"),
    "create_report_region": ("region", "p_static_id"),
    "create_page_item": ("pageItem", "p_name"),
    "create_page_button": ("button", "p_static_id"),
    "create_page_process": ("process", "p_static_id"),
    "create_page_validation": ("validation", "p_static_id"),
    "create_page_da_event": ("dynamicAction", "p_static_id"),
    "create_page_da_action": ("action", "p_static_id"),
    "create_page_computation": ("computation", "p_static_id"),
    "create_security_scheme": ("authorization", "p_static_id"),
    "create_list_of_values": ("lov", "p_static_id"),
    "create_list": ("list", "p_static_id"),
    "create_list_item": ("entry", "p_static_id"),
    "create_menu": ("breadcrumb", "p_static_id"),
    "create_menu_option": ("entry", "p_static_id"),
    "create_page_group": ("pageGroup", "p_static_id"),
    "create_build_option": ("buildOption", "p_static_id"),
}
PAGE_KINDS = {
    "region", "pageItem", "button", "process", "validation", "dynamicAction", "computation", "action",
}
# Kinds whose static ids repeat across parents, and how the export names the parent: a parameter
# holding the parent's id (a DA action's p_event_id), or the enclosing call the record follows (a
# list's items follow its create_list, a breadcrumb's options its create_menu).
PARENT_PARAMS = {"action": ("p_event_id",)}
PARENT_CALLS = {"create_list_item": "create_list", "create_menu_option": "create_menu"}
PARENT_KINDS = {"entry", "action"}

# (owner kind, APEXlang property) -> SQL parameter of the owner's record holding the reference.
MAPPING = {
    ("pageItem", "layout.region"): "p_item_plug_id",
    ("button", "layout.region"): "p_button_plug_id",
    ("region", "layout.parentRegion"): "p_parent_plug_id",
    ("pageItem", "source.formRegion"): "p_item_source_plug_id",
    ("process", "formRegion"): "p_region_id",
    ("page", "security.authorizationScheme"): "p_required_role",
    ("region", "security.authorizationScheme"): ("p_plug_required_role", "p_required_role"),
    ("button", "security.authorizationScheme"): "p_security_scheme",
    ("process", "security.authorizationScheme"): "p_security_scheme",
    ("pageItem", "security.authorizationScheme"): "p_security_scheme",
    ("entry", "security.authorizationScheme"): "p_security_scheme",
    ("page", "pageGroup"): "p_group_id",
    ("region", "source.breadcrumb"): "p_menu_id",
    ("region", "source.list"): "p_list_id",
    ("pageItem", "lov.lov"): "p_named_lov",
    ("page", "config.buildOption"): "p_required_patch",
    ("region", "config.buildOption"): "p_required_patch",
    ("button", "config.buildOption"): "p_required_patch",
    ("pageItem", "config.buildOption"): "p_required_patch",
    ("process", "config.buildOption"): "p_required_patch",
    ("entry", "config.buildOption"): "p_required_patch",
    ("process", "serverSideCondition.whenButtonPressed"): "p_process_when_button_id",
    ("validation", "serverSideCondition.whenButtonPressed"): "p_when_button_pressed",
    ("dynamicAction", "when.button"): "p_triggering_button_id",
    ("dynamicAction", "when.region"): "p_triggering_region_id",
    ("action", "affectedElements.region"): "p_affected_region_id",
    ("action", "affectedElements.button"): "p_affected_button_id",
    ("validation", "error.associatedItem"): "p_associated_item",
    ("entry", "layout.parentEntry"): ("p_parent_list_item_id", "p_parent_id"),
    ("entry", "appearance.parentEntry"): ("p_parent_list_item_id", "p_parent_id"),
}

CALL = re.compile(r"^wwv_flow_imp(?:_page|_shared)?\.(create_[a-z_]+)\($")
PARAM = re.compile(r"^[ ,](p_[a-z0-9_]+)=>(.*)$")
ID = re.compile(r"^wwv_flow_imp\.id\((-?\d+)\)$")


def sql_value(raw):
    raw = raw.strip()
    m = ID.match(raw)
    if m:
        return ("id", m.group(1))
    if raw.startswith("'") and raw.endswith("'") and len(raw) >= 2:
        return ("str", raw[1:-1].replace("''", "'"))
    if re.fullmatch(r"-?\d+", raw):
        return ("num", raw)
    return ("expr", raw)


def parse_export(path):
    """Every create_* record: (proc, params, page)."""
    records, cur, page = [], None, None
    with open(path, encoding="utf-8", errors="replace") as fh:
        for line in fh:
            line = line.rstrip("\n")
            m = CALL.match(line)
            if m:
                cur = (m.group(1), {})
                continue
            if cur is None:
                continue
            if line == ");":
                proc, params = cur
                if proc == "create_page" and "p_id" in params:
                    page = params["p_id"][1]
                records.append((proc, params, page if proc != "create_page" else params.get("p_id", (None, None))[1]))
                cur = None
                continue
            pm = PARAM.match(line)
            if pm and pm.group(1) not in cur[1]:
                cur[1][pm.group(1)] = sql_value(pm.group(2))
    return records


class Export:
    def __init__(self, path):
        self.by_id = {}
        self.by_key = {}
        self.lov_by_name = {}
        self.parent = {}  # id(rec) -> parent static id
        enclosing = {}
        records = []
        for proc, params, page in parse_export(path):
            if proc not in PROCS:
                continue
            kind, keyp = PROCS[proc]
            key = params.get(keyp, (None, None))[1]
            rec = (kind, key, page if kind in PAGE_KINDS else None, params)
            records.append((proc, rec))
            if proc in PARENT_CALLS.values():
                enclosing[proc] = key
            if proc in PARENT_CALLS:
                self.parent[id(rec)] = enclosing.get(PARENT_CALLS[proc])
            pid = params.get("p_id")
            if pid and pid[0] in ("id", "num"):
                self.by_id[pid[1]] = rec
            if key is not None:
                self.by_key.setdefault((kind, rec[2], key), []).append(rec)
            if proc == "create_list_of_values" and "p_lov_name" in params:
                self.lov_by_name.setdefault(params["p_lov_name"][1], []).append(rec)
        for proc, rec in records:
            for p in PARENT_PARAMS.get(rec[0], ()):
                v = rec[3].get(p)
                if v and v[0] in ("id", "num") and v[1] in self.by_id:
                    self.parent[id(rec)] = self.by_id[v[1]][1]

    def parent_of(self, rec):
        return self.parent.get(id(rec))


def page_of(symbol):
    m = re.search(r"#page:([^#~]+)#", symbol)
    return m.group(1) if m else None


def parent_key(symbol):
    """Key of the source's parent component (the descriptor before the last), decoded."""
    parts = [p for p in symbol.split("#") if p]
    if len(parts) < 2:
        return None
    last = parts[-2]
    return decode_key(last.split(":", 1)[1]) if ":" in last else None


def decode_key(k):
    return re.sub(r"%([0-9A-F]{2})", lambda m: chr(int(m.group(1), 16)), k)


def label(edge, export):
    sk, skey, tk, tkey = edge["source_kind"], edge["source_key"], edge["target_kind"], edge["target_key"]
    param = MAPPING.get((sk, edge["property"]))
    if param is None:
        return "unlabelled", f"no SQL parameter mapped for {sk}.{edge['property']}"
    spage = page_of(edge["source"])
    owners = export.by_key.get((sk, spage if sk in PAGE_KINDS else None, skey), [])
    if sk == "page":
        owners = export.by_key.get(("page", None, skey), [])
    if sk in PARENT_KINDS and len(owners) > 1:
        want = parent_key(edge["source"])
        owners = [o for o in owners if export.parent_of(o) == want]
    if len(owners) != 1:
        return "unlabelled", f"owner {sk} {skey} on page {spage}: {len(owners)} SQL records"
    params = owners[0][3]
    names = param if isinstance(param, tuple) else (param,)
    val = next((params[p] for p in names if p in params), None)
    if val is None:
        return "wrong", f"SQL owner has no {'/'.join(names)}: Oracle records no such reference"
    if val[0] == "str" and names[0] == "p_named_lov":
        truths = export.lov_by_name.get(val[1], [])
        if len(truths) != 1:
            return "unlabelled", f"p_named_lov {val[1]!r}: {len(truths)} LOVs"
        truth = truths[0]
    elif val[0] in ("id", "num"):
        truth = export.by_id.get(val[1])
        if truth is None:
            return "unlabelled", f"{names[0]} id {val[1]} not in export (external/built-in)"
    else:
        return "unlabelled", f"{names[0]} is an expression: {val[1][:40]}"
    tkind, tkey_sql, tpage, _ = truth
    if tkind != tk:
        return "wrong", f"Oracle target kind {tkind}, resolver {tk}"
    if tkey_sql != decode_key(tkey):
        return "wrong", f"Oracle target {tkey_sql!r}, resolver {tkey!r}"
    if tpage is not None and tpage != page_of(edge["target"]):
        return "wrong", f"Oracle target on page {tpage}, resolver page {page_of(edge['target'])}"
    # A target whose static id repeats across parents (a list entry) must sit under the parent
    # Oracle names: `list:a#entry:x#` is not `list:b#entry:x#`.
    if tkind in PARENT_KINDS:
        sql_parent = export.parent_of(truth)
        if sql_parent is None or sql_parent != parent_key(edge["target"]):
            return "wrong", f"Oracle target parent {sql_parent!r}, resolver {parent_key(edge['target'])!r}"
    return "correct", f"{names[0]} -> {tkind} {tkey_sql}"


def main():
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    corpus, edges_tsv, out = sys.argv[1:]
    exports = {}
    rows = []
    with open(edges_tsv, encoding="utf-8") as fh:
        header = fh.readline().rstrip("\n").split("\t")
        for line in fh:
            rows.append(dict(zip(header, line.rstrip("\n").split("\t"))))
    with open(out, "w", encoding="utf-8", newline="\n") as fh:
        fh.write("file\tline\tproperty\twritten\tsource\ttarget\tlabel\treason\n")
        for e in rows:
            if not e["file"].startswith("oracle-apex/"):
                verdict, why = "unlabelled", "no SQL export (oracle/skills example)"
            else:
                app_dir = e["file"].split("/apexlang/")[0]
                if app_dir not in exports:
                    sqls = glob.glob(os.path.join(corpus, app_dir, "sql", "*.sql"))
                    exports[app_dir] = Export(sqls[0]) if len(sqls) == 1 else None
                ex = exports[app_dir]
                if ex is None:
                    verdict, why = "unlabelled", "no single SQL export for the application"
                else:
                    verdict, why = label(e, ex)
            fh.write("\t".join([e["file"], e["line"], e["property"], e["written"], e["source"],
                                e["target"], verdict, why]) + "\n")
    print(f"labelled {len(rows)} edges -> {out}")


if __name__ == "__main__":
    main()
