"""Печатает заголовки (_NET_WM_NAME) показанных окон верхнего уровня процесса
<pid> в X11, по одному в строке (UTF-8). Нужен сценарному тесту заголовков
окон (tests/e2e/window-titles.ts, находка QA Н2): WebDriver отдаёт заголовок
страницы, а не заголовок окна ОС.

Только стандартная библиотека (ctypes + libX11), без сети. Привязки libX11
и обход окон — из x11-close.py (загружается по пути).

Запуск: python3 -I x11-titles.py <pid>.
"""

import ctypes
import importlib.util
import os
import sys

_SPEC = importlib.util.spec_from_file_location(
    "x11_close", os.path.join(os.path.dirname(os.path.abspath(__file__)), "x11-close.py")
)
x11 = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(x11)
X11 = x11.X11
Atom = x11.Atom


def window_name(display, window, name_atom, utf8_atom):
    actual_type = Atom()
    actual_format = ctypes.c_int()
    count = ctypes.c_ulong()
    after = ctypes.c_ulong()
    data = ctypes.c_void_p()
    status = X11.XGetWindowProperty(
        display, window, name_atom, 0, 4096, 0, utf8_atom,
        ctypes.byref(actual_type), ctypes.byref(actual_format),
        ctypes.byref(count), ctypes.byref(after), ctypes.byref(data),
    )
    if status != 0 or not data.value:
        return None
    try:
        if actual_format.value != 8:
            return None
        return ctypes.string_at(data, count.value).decode("utf-8", "replace")
    finally:
        X11.XFree(data)


def main():
    pid = int(sys.argv[1])
    display = X11.XOpenDisplay(None)
    if not display:
        print("нет дисплея X11", file=sys.stderr)
        return 2
    try:
        pid_atom = X11.XInternAtom(display, b"_NET_WM_PID", 1)
        name_atom = X11.XInternAtom(display, b"_NET_WM_NAME", 0)
        utf8_atom = X11.XInternAtom(display, b"UTF8_STRING", 0)
        out = []
        for window in x11.children(display, X11.XDefaultRootWindow(display)):
            if x11.window_pid(display, window, pid_atom) != pid:
                continue
            attributes = x11.XWindowAttributes()
            if not X11.XGetWindowAttributes(display, window, ctypes.byref(attributes)):
                continue
            if attributes.map_state != x11.IS_VIEWABLE:
                continue
            name = window_name(display, window, name_atom, utf8_atom)
            if name is not None:
                out.append(name)
        sys.stdout.buffer.write("".join(f"{name}\n" for name in out).encode("utf-8"))
        return 0
    finally:
        X11.XCloseDisplay(display)


if __name__ == "__main__":
    sys.exit(main())
