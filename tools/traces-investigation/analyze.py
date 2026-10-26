"""Разбор прогона расследования следов (задача 0.6).

Вход — папка, которую заполнил run.ps1:
  markers.json        — границы фаз (FILETIME UTC);
  snap/<фаза>-pre|post/{files.tsv,hkcu.reg,creds.json};
  results.jsonl       — ответы зонда по фазам;
  cap*.pml            — запись Process Monitor.
Выход — summary.md (Markdown) в той же папке; он же печатается в журнал.

Использование: python analyze.py <папка> <LOCALAPPDATA> <APPDATA>
"""

import glob
import json
import os
import re
import sys
from collections import Counter, defaultdict

OUT = sys.argv[1]
LOCAL = sys.argv[2].rstrip("\\").lower()
ROAMING = sys.argv[3].rstrip("\\").lower()

lines = []


def emit(text=""):
    lines.append(text)


# --- категории путей ----------------------------------------------------------

CATEGORIES = [
    ("Credentials", re.compile(r"\\microsoft\\credentials", re.I)),
    ("DPAPI Protect", re.compile(r"\\microsoft\\protect", re.I)),
    ("Vault", re.compile(r"\\microsoft\\vault", re.I)),
    ("TokenBroker", re.compile(r"tokenbroker", re.I)),
    ("Identity/WAM", re.compile(r"identitycrl|identitycache|oneauth|aad\.brokerplugin|identitystore", re.I)),
    ("ComDlg32", re.compile(r"comdlg32", re.I)),
    ("ShellBags", re.compile(r"bagmru|\\shell\\bags|shellnoroam", re.I)),
    ("Recent/JumpLists", re.compile(r"\\recent\\|automaticdestinations|customdestinations|recentdocs|typedpaths", re.I)),
    ("WebView2 temp (expected)", re.compile(r"rhizomind-webview-|t06-webview-", re.I)),
    ("WAM client load (Load Image)", re.compile(r"usermgrproxy|authentication\.web\.core|microsoftaccounttokenprovider|userprofile", re.I)),
    ("Rhizomind profile", re.compile(r"\\rhizomind\\vaults|\\rhizomind\\\.vaults", re.I)),
]

NOISE = re.compile(
    r"\\microsoft\\windows\\powershell\\|\\microsoft\\powershell\\|\\sysinternals\\|\\t06\\snap\\|\\_temp\\t06\\",
    re.I,
)


def category(path):
    for name, rx in CATEGORIES:
        if rx.search(path):
            return name
    return "other"


def roams_file(path):
    p = path.lower()
    if p.startswith(ROAMING + "\\"):
        return "ДА (Roaming)"
    if p.startswith(LOCAL + "\\"):
        return "нет (Local)"
    return "-"


def roams_key(key):
    k = key.upper()
    for prefix in ("HKEY_CURRENT_USER\\SOFTWARE\\CLASSES", "HKCU\\SOFTWARE\\CLASSES"):
        if k.startswith(prefix):
            return "нет (UsrClass.dat, Local)"
    if k.startswith("HKEY_CURRENT_USER") or k.startswith("HKCU"):
        return "ДА (NTUSER.DAT)"
    if "\\REGISTRY\\USER\\" in k and "_CLASSES" in k:
        return "нет (UsrClass.dat, Local)"
    if "\\REGISTRY\\USER\\" in k:
        return "ДА (NTUSER.DAT)"
    return "-"


# --- фазы ---------------------------------------------------------------------

with open(os.path.join(OUT, "markers.json"), encoding="utf-8-sig") as f:
    markers = json.load(f)
starts = {m["phase"]: m["filetime"] for m in markers if m["edge"] == "start"}
ends = {m["phase"]: m["filetime"] for m in markers if m["edge"] == "end"}
phases = sorted(((p, starts[p], ends.get(p, starts[p])) for p in starts), key=lambda x: x[1])
phase_names = [p for p, _, _ in phases]


def phase_of(ft):
    for name, s, e in phases:
        if s <= ft <= e:
            return name
    return "(вне фаз)"


def kind(phase):
    return phase.split("#")[0]


results = defaultdict(list)
if os.path.exists(os.path.join(OUT, "results.jsonl")):
    with open(os.path.join(OUT, "results.jsonl"), encoding="utf-8-sig") as f:
        for line in f:
            line = line.strip()
            if line:
                r = json.loads(line)
                results[r.get("phase", "?")].append(r)

# --- снимки -------------------------------------------------------------------


def load_files(path):
    result = {}
    if not os.path.exists(path):
        return result
    with open(path, encoding="utf-8-sig", errors="replace") as f:
        for line in f:
            parts = line.rstrip("\r\n").split("\t")
            if len(parts) == 3:
                result[parts[0]] = (parts[1], parts[2])
    return result


def load_reg(path):
    keys = {}
    if not os.path.exists(path):
        return keys
    with open(path, encoding="utf-16", errors="replace") as f:
        text = f.read()
    text = text.replace("\\\r\n  ", "").replace("\\\n  ", "")
    current = None
    for line in text.splitlines():
        if line.startswith("[") and line.endswith("]"):
            current = line[1:-1]
            keys[current] = {}
        elif current is not None and "=" in line:
            name, _, data = line.partition("=")
            keys[current][name] = data
    return keys


def decode_strings(data):
    """Строки UTF-16 и ASCII из hex:… — чтобы видеть пути внутри PIDL."""
    m = re.match(r"hex(\([0-9a-f]+\))?:(.*)", data)
    if not m:
        return data[:200]
    try:
        raw = bytes(int(x, 16) for x in m.group(2).split(",") if x)
    except ValueError:
        return data[:200]
    found = []
    for s in re.findall(rb"(?:[\x20-\x7e]\x00){4,}", raw):
        found.append(s.decode("utf-16-le"))
    for s in re.findall(rb"[\x20-\x7e]{6,}", raw):
        found.append(s.decode("ascii"))
    uniq = []
    for s in found:
        if s not in uniq:
            uniq.append(s)
    return f"{len(raw)} байт; строки: " + " | ".join(uniq[:12])


def load_creds(path):
    if not os.path.exists(path):
        return {}
    try:
        with open(path, encoding="utf-8-sig") as f:
            data = json.load(f)
    except (ValueError, OSError) as e:
        return {"(ошибка чтения)": str(e)}
    return {c.get("target", "?"): c for c in data.get("credentials", [])}


def snapshot_diff(phase):
    pre = os.path.join(OUT, "snap", phase.replace("#", "-") + "-pre")
    post = os.path.join(OUT, "snap", phase.replace("#", "-") + "-post")
    if not os.path.isdir(pre) or not os.path.isdir(post):
        return None
    fa, fb = load_files(os.path.join(pre, "files.tsv")), load_files(os.path.join(post, "files.tsv"))
    files = []
    for p, v in fb.items():
        old = fa.get(p)
        if old is None:
            files.append(("новый", p))
        elif old != v and v[0] != "dir":
            files.append(("изменён", p))
    for p in fa:
        if p not in fb:
            files.append(("удалён", p))
    ra, rb = load_reg(os.path.join(pre, "hkcu.reg")), load_reg(os.path.join(post, "hkcu.reg"))
    reg = []
    for k, vals in rb.items():
        if k not in ra:
            reg.append(("новый ключ", k, "", ""))
            for n, d in vals.items():
                reg.append(("новое значение", k, n, decode_strings(d)))
            continue
        for n, d in vals.items():
            if n not in ra[k]:
                reg.append(("новое значение", k, n, decode_strings(d)))
            elif ra[k][n] != d:
                reg.append(("изменено значение", k, n, decode_strings(d)))
        for n in ra[k]:
            if n not in vals:
                reg.append(("удалено значение", k, n, ""))
    for k in ra:
        if k not in rb:
            reg.append(("удалён ключ", k, "", ""))
    ca, cb = load_creds(os.path.join(pre, "creds.json")), load_creds(os.path.join(post, "creds.json"))
    creds = []
    for t, c in cb.items():
        if t not in ca:
            creds.append(("новая", c))
        elif ca[t] != c:
            creds.append(("изменена", c))
    for t, c in ca.items():
        if t not in cb:
            creds.append(("удалена", c))
    return files, reg, creds


# --- Process Monitor ----------------------------------------------------------

WRITE_OPS = {
    "WriteFile",
    "SetRenameInformationFile",
    "SetRenameInformationEx",
    "SetDispositionInformationFile",
    "SetDispositionInformationEx",
    "SetBasicInformationFile",
    "SetEndOfFileInformationFile",
    "SetAllocationInformationFile",
    "RegSetValue",
    "RegDeleteValue",
    "RegDeleteKey",
    "RegSetInfoKey",
    "RegRenameKey",
}

pml_files = sorted(glob.glob(os.path.join(OUT, "cap*.pml")))
writes = defaultdict(lambda: {"count": 0, "stacks": [], "detail": "", "result": ""})
network = Counter()
proc_info = {}
total_events = 0
outside_filter = 0
filter_rx = re.compile(
    r"credentials|protect|vault|tokenbroker|identitycrl|identitycache|oneauth|aad\.brokerplugin|identitystore|usermgrproxy|authentication\.web\.core|microsoftaccounttokenprovider|userprofile|comdlg32|bagmru|shell\\bags|shellnoroam|\\recent\\|destinations|recentdocs|typedpaths|rhizomind|t06-|t06probe| -> ",
    re.I,
)


def stack_summary(event):
    frames = []
    modules = event.process.modules or []
    for addr in event.stacktrace or []:
        name = None
        for m in modules:
            if m.base_address <= addr < m.base_address + m.size:
                name = f"{os.path.basename(m.path)}+0x{addr - m.base_address:x}"
                break
        if name is None:
            continue  # ядро ОС: адреса без модуля процесса
        frames.append(name)
    # схлопываем повторы одного модуля подряд
    compact = []
    for fr in frames:
        mod = fr.split("+")[0]
        if compact and compact[-1].split("+")[0] == mod:
            continue
        compact.append(fr)
    return " < ".join(compact[:14])


if pml_files:
    from procmon_parser import ProcmonLogsReader

    for pml in pml_files:
        with open(pml, "rb") as f:
            reader = ProcmonLogsReader(f, should_get_stacktrace=True, should_get_details=True)
            for p in reader.processes():
                proc_info[p.pid] = p
            for event in reader:
                total_events += 1
                path = event.path or ""
                if not filter_rx.search(path):
                    outside_filter += 1
                    continue
                phase = phase_of(event.date_filetime)
                proc = event.process.process_name
                op = event._get_compatible_csv_operation_name()
                cls = str(event.event_class)
                if "Network" in cls or " -> " in path:
                    network[(phase, proc, event.process.pid, op, path)] += 1
                    continue
                try:
                    detail = event._get_compatible_csv_detail_column()
                except Exception:  # noqa: BLE001 — разбор деталей не критичен
                    detail = ""
                if op == "Load Image" and re.search(r"usermgrproxy|authentication\.web\.core|microsoftaccounttokenprovider|userprofile", path, re.I):
                    key = (phase, proc, event.process.pid, op, path)
                    w = writes[key]
                    w["count"] += 1
                    if not w["stacks"]:
                        w["stacks"].append(stack_summary(event))
                    continue
                is_write = op in WRITE_OPS or (op == "CreateFile" and "OpenResult: Created" in detail) or (
                    op == "RegCreateKey" and "REG_CREATED_NEW_KEY" in detail
                )
                if not is_write:
                    continue
                key = (phase, proc, event.process.pid, op, path)
                w = writes[key]
                w["count"] += 1
                if not w["detail"]:
                    w["detail"] = detail[:300]
                    try:
                        from procmon_parser.consts import get_error_message

                        w["result"] = get_error_message(event.result)
                    except Exception:  # noqa: BLE001
                        w["result"] = str(event.result)
                if len(w["stacks"]) < 1:
                    w["stacks"].append(stack_summary(event))

# --- отчёт --------------------------------------------------------------------

emit(f"# Расследование следов: {os.environ.get('T06_JOB', '?')}")
emit()
emit(f"Фазы: {', '.join(phase_names)}")
emit(f"Process Monitor: файлов PML {len(pml_files)}, событий {total_events}, вне фильтра {outside_filter}")
emit()

for phase in phase_names:
    emit(f"## Фаза {phase}")
    for r in results.get(phase, []):
        emit(f"- ответ зонда: `{json.dumps(r, ensure_ascii=False)[:1500]}`")
    d = snapshot_diff(phase)
    if d is None:
        emit("- снимков нет")
    else:
        files, reg, creds = d
        shown = [(k, p) for k, p in files if not NOISE.search(p)]
        temp_count = sum(1 for _, p in shown if category(p) == "WebView2 temp (expected)")
        shown = [(k, p) for k, p in shown if category(p) != "WebView2 temp (expected)"]
        emit(f"- файлы %LOCALAPPDATA%/%APPDATA% (без шума PowerShell; во временной папке WebView2: {temp_count}):")
        for k, p in shown[:60]:
            emit(f"  - {k}: `{p}` — категория {category(p)}, Roaming: {roams_file(p)}")
        if len(shown) > 60:
            emit(f"  - … ещё {len(shown) - 60}")
        reg_shown = [r for r in reg if not NOISE.search(r[1])]
        emit(f"- реестр HKCU (без Sysinternals): {len(reg_shown)} изменений")
        for kind_, k, n, d_ in reg_shown[:80]:
            emit(f"  - {kind_}: `{k}` {n} {('= ' + d_) if d_ else ''} — Roaming: {roams_key(k)}")
        if len(reg_shown) > 80:
            emit(f"  - … ещё {len(reg_shown) - 80}")
        emit(f"- диспетчер учётных данных: {len(creds)} изменений")
        for kind_, c in creds:
            emit(f"  - {kind_}: `{json.dumps(c, ensure_ascii=False)}`")
    pw = [(k, v) for k, v in writes.items() if k[0] == phase]
    by_cat = defaultdict(list)
    for k, v in pw:
        by_cat[category(k[4])].append((k, v))
    emit("- Process Monitor, записи по категориям:")
    if not pw:
        emit("  - нет")
    for cat in sorted(by_cat):
        items = by_cat[cat]
        if cat in ("WebView2 temp (expected)",):
            emit(f"  - {cat}: {sum(v['count'] for _, v in items)} событий записи")
            continue
        emit(f"  - **{cat}**:")
        for (ph, proc, pid, op, path), v in items[:25]:
            cmd = (proc_info[pid].command_line if pid in proc_info else "")[:160]
            emit(f"    - {proc} ({pid}) {op} `{path}` ×{v['count']} {v['result']} — {v['detail'][:160]}")
            emit(f"      - командная строка: `{cmd}`")
            if v["stacks"] and v["stacks"][0]:
                emit(f"      - стек: `{v['stacks'][0]}`")
        if len(items) > 25:
            emit(f"    - … ещё {len(items) - 25}")
    net = [(k, c) for k, c in network.items() if k[0] == phase]
    emit(f"- сеть (TCP/UDP), {len(net)} разных потоков:")
    by_proc = defaultdict(list)
    for (ph, proc, pid, op, path), c in net:
        by_proc[(proc, pid)].append((op, path, c))
    for (proc, pid), items in sorted(by_proc.items()):
        cmd = (proc_info[pid].command_line if pid in proc_info else "")[:120]
        emit(f"  - {proc} ({pid}) `{cmd}`")
        for op, path, c in items[:8]:
            emit(f"    - {op} {path} ×{c}")
        if len(items) > 8:
            emit(f"    - … ещё {len(items) - 8}")
    emit()

# сводная таблица «категория × вид фазы»
emit("## Сводка: записи Process Monitor по категориям и видам фаз (число фаз с записью / всего фаз)")
kinds = []
for p in phase_names:
    if kind(p) not in kinds:
        kinds.append(kind(p))
cats = sorted({category(k[4]) for k in writes})
emit("| категория | " + " | ".join(kinds) + " | вне фаз |")
emit("|---" * (len(kinds) + 2) + "|")
for cat in cats:
    row = []
    for kd in kinds:
        ph = [p for p in phase_names if kind(p) == kd]
        hit = {k[0] for k in writes if category(k[4]) == cat and kind(k[0]) == kd}
        row.append(f"{len(hit)}/{len(ph)}")
    outside = sum(v["count"] for k, v in writes.items() if category(k[4]) == cat and k[0] == "(вне фаз)")
    emit(f"| {cat} | " + " | ".join(row) + f" | {outside} |")
emit()
emit("## Сводка: сетевые процессы по видам фаз (число фаз с сетью / всего фаз)")
procs = sorted({k[1] for k in network})
emit("| процесс | " + " | ".join(kinds) + " |")
emit("|---" * (len(kinds) + 1) + "|")
for proc in procs:
    row = []
    for kd in kinds:
        ph = [p for p in phase_names if kind(p) == kd]
        hit = {k[0] for k in network if k[1] == proc and kind(k[0]) == kd}
        row.append(f"{len(hit)}/{len(ph)}")
    emit(f"| {proc} | " + " | ".join(row) + " |")
emit()
emit("## Записи вне фаз (Credentials, TokenBroker, ComDlg32, ShellBags, Recent, Identity)")
for (ph, proc, pid, op, path), v in writes.items():
    if ph == "(вне фаз)" and category(path) in (
        "Credentials", "TokenBroker", "ComDlg32", "ShellBags", "Recent/JumpLists", "Identity/WAM", "DPAPI Protect", "Vault"
    ):
        emit(f"- {proc} ({pid}) {op} `{path}` ×{v['count']} — стек: `{v['stacks'][0] if v['stacks'] else ''}`")

text = "\n".join(lines) + "\n"
with open(os.path.join(OUT, "summary.md"), "w", encoding="utf-8") as f:
    f.write(text)
print(text)
