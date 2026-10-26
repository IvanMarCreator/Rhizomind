"""Сводка поиска OneAuth (задача 0.6б): по каждой фазе — загрузил ли процесс
браузера WebView2 oneauth.dll и клиент WAM, писал ли кэш брокера токенов,
выходила ли служба wlidsvc в сеть.

Использование: python oneauth_sweep.py <папка с cap*.pml и markers.json>
Пишет oneauth.md в ту же папку и печатает его.
"""

import glob
import json
import os
import re
import sys

from procmon_parser import ProcmonLogsReader

OUT = sys.argv[1]
with open(os.path.join(OUT, "markers.json"), encoding="utf-8-sig") as f:
    markers = json.load(f)
starts = {m["phase"]: m["filetime"] for m in markers if m["edge"] == "start"}
ends = {m["phase"]: m["filetime"] for m in markers if m["edge"] == "end"}
phases = sorted(starts, key=lambda p: starts[p])
info = {}
results = {}
if os.path.exists(os.path.join(OUT, "results.jsonl")):
    with open(os.path.join(OUT, "results.jsonl"), encoding="utf-8-sig") as f:
        for line in f:
            if line.strip():
                r = json.loads(line)
                results.setdefault(r.get("phase"), {}).update(r)


def phase_of(ft):
    for p in phases:
        if starts[p] <= ft <= ends.get(p, starts[p]):
            return p
    return None


rows = {p: {"oneauth": False, "webcore": False, "tbres": 0, "wlid": set(), "browser": set()} for p in phases}
for pml in sorted(glob.glob(os.path.join(OUT, "cap*.pml"))):
    with open(pml, "rb") as f:
        for ev in ProcmonLogsReader(f, should_get_stacktrace=False, should_get_details=False):
            p = phase_of(ev.date_filetime)
            if p is None:
                continue
            proc = ev.process
            name = proc.process_name.lower()
            path = ev.path or ""
            op = ev._get_compatible_csv_operation_name()
            cmd = proc.command_line or ""
            row = rows[p]
            if name == "msedgewebview2.exe":
                if "--type=" not in cmd:
                    row["browser"].add(proc.pid)
                if op == "Load Image" and re.search(r"\\oneauth\.dll$", path, re.I):
                    row["oneauth"] = True
                if op == "Load Image" and "authentication.web.core" in path.lower():
                    row["webcore"] = True
                if "tokenbroker\\cache" in path.lower() and op in ("WriteFile", "SetDispositionInformationFile"):
                    row["tbres"] += 1
            if "wlidsvc" in cmd and " -> " in path and "Connect" in op:
                row["wlid"].add(path.split("-> ")[-1])

lines = ["# OneAuth: загрузка по фазам", "", "| фаза | oneauth.dll | клиент WAM | запись .tbres | wlidsvc в сеть | exe | pfhostedapp | параметры |", "|---|---|---|---|---|---|---|---|"]
for p in phases:
    r = rows[p]
    res = results.get(p, {})
    params = res.get("policy") or res.get("args") or ""
    cmd = res.get("browserCmd") or ""
    hosted = re.search(r"/pfhostedapp:([0-9a-f]+)", cmd)
    exe = re.search(r"--webview-exe-name=(\S+)", cmd)
    lines.append(
        f"| {p} | {'да' if r['oneauth'] else 'нет'} | {'да' if r['webcore'] else 'нет'} | {r['tbres']} | {', '.join(sorted(r['wlid'])) or '—'} "
        f"| {exe.group(1) if exe else ''} | {hosted.group(1)[:12] if hosted else ''} | {params} |"
    )
text = "\n".join(lines) + "\n"
with open(os.path.join(OUT, "oneauth.md"), "w", encoding="utf-8") as f:
    f.write(text)
print(text)
