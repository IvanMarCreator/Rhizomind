"""Конфигурация Process Monitor для расследования следов (задача 0.6).

Пишет файл .pmc (для /LoadConfig) и те же правила в виде REG_BINARY
(HKCU\\Software\\Sysinternals\\Process Monitor\\FilterRules) — на случай,
если частичный .pmc не применится. Правила одного столбца объединяются
по «или», поэтому всё, что нужно, — подстроки столбца Path.

Использование: python make_pmc.py <out.pmc> <out-filter-rules.bin>
"""

import sys

from procmon_parser import Rule, dumps_configuration
from procmon_parser.configuration_format import RulesStruct

PATH_CONTAINS = [
    # учётные данные и защита DPAPI
    "\\Microsoft\\Credentials",
    "\\Microsoft\\Protect",
    "\\Microsoft\\Vault",
    # WAM / брокер токенов / учётные записи Microsoft
    "TokenBroker",
    "IdentityCRL",
    "IdentityCache",
    "OneAuth",
    "AAD.BrokerPlugin",
    "\\Microsoft\\IdentityStore",
    # окно выбора папки, ShellBags, недавние
    "ComDlg32",
    "BagMRU",
    "\\Shell\\Bags",
    "ShellNoRoam",
    "\\Recent\\",
    "AutomaticDestinations",
    "CustomDestinations",
    "RecentDocs",
    "TypedPaths",
    # само приложение и зонды
    "rhizomind",
    "t06-",
    "t06probe",
    # загрузка клиентских библиотек WAM/учётных записей (Load Image со стеком вызова)
    "UserMgrProxy",
    "Windows.Security.Authentication.Web.Core",
    "MicrosoftAccountTokenProvider",
    "Windows.System.UserProfile",
    # вся сеть: у сетевых событий Path имеет вид "адрес:порт -> адрес:порт"
    " -> ",
]

EXCLUDE_PROCESSES = ["Procmon.exe", "Procmon64.exe", "python.exe", "reg.exe"]


def rules():
    result = [Rule("Path", "contains", value, "include") for value in PATH_CONTAINS]
    result += [Rule("Process_Name", "is", name, "exclude") for name in EXCLUDE_PROCESSES]
    return result


def main():
    pmc, reg_bin = sys.argv[1], sys.argv[2]
    config = {"DestructiveFilter": 1, "FilterRules": rules()}
    with open(pmc, "wb") as f:
        f.write(dumps_configuration(config))
    with open(reg_bin, "wb") as f:
        f.write(RulesStruct.build(rules()))
    print(f"правил: {len(rules())}; {pmc}, {reg_bin}")


if __name__ == "__main__":
    main()
