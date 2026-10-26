"""Закрывает окна процесса так, как это делает пользователь кнопкой «×» в X11:
сообщение WM_DELETE_WINDOW (протокол WM_PROTOCOLS) каждому показанному окну
верхнего уровня с _NET_WM_PID = <pid>. Нужен сценарному тесту закрытия окна
(tests/e2e/close.test.ts) на Linux: в Xvfb нет оконного менеджера, а
WebDriver закрытие окна пользователем не воспроизводит.

Только стандартная библиотека (ctypes + libX11), без сети.

Запуск: python3 -I x11-close.py <pid>; код 0 — сообщение отправлено хотя бы
одному окну, 1 — окон процесса нет.
"""

import ctypes
import ctypes.util
import sys

X11 = ctypes.cdll.LoadLibrary(ctypes.util.find_library("X11") or "libX11.so.6")

Window = ctypes.c_ulong
Atom = ctypes.c_ulong


class XClientMessageEvent(ctypes.Structure):
    _fields_ = [
        ("type", ctypes.c_int),
        ("serial", ctypes.c_ulong),
        ("send_event", ctypes.c_int),
        ("display", ctypes.c_void_p),
        ("window", Window),
        ("message_type", Atom),
        ("format", ctypes.c_int),
        ("data", ctypes.c_long * 5),
    ]


class XEvent(ctypes.Union):
    _fields_ = [("xclient", XClientMessageEvent), ("pad", ctypes.c_long * 24)]


class XWindowAttributes(ctypes.Structure):
    _fields_ = [
        ("x", ctypes.c_int),
        ("y", ctypes.c_int),
        ("width", ctypes.c_int),
        ("height", ctypes.c_int),
        ("border_width", ctypes.c_int),
        ("depth", ctypes.c_int),
        ("visual", ctypes.c_void_p),
        ("root", Window),
        ("c_class", ctypes.c_int),
        ("bit_gravity", ctypes.c_int),
        ("win_gravity", ctypes.c_int),
        ("backing_store", ctypes.c_int),
        ("backing_planes", ctypes.c_ulong),
        ("backing_pixel", ctypes.c_ulong),
        ("save_under", ctypes.c_int),
        ("colormap", ctypes.c_ulong),
        ("map_installed", ctypes.c_int),
        ("map_state", ctypes.c_int),
        ("all_event_masks", ctypes.c_long),
        ("your_event_mask", ctypes.c_long),
        ("do_not_propagate_mask", ctypes.c_long),
        ("override_redirect", ctypes.c_int),
        ("screen", ctypes.c_void_p),
    ]


X11.XOpenDisplay.restype = ctypes.c_void_p
X11.XOpenDisplay.argtypes = [ctypes.c_char_p]
X11.XDefaultRootWindow.restype = Window
X11.XDefaultRootWindow.argtypes = [ctypes.c_void_p]
X11.XInternAtom.restype = Atom
X11.XInternAtom.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_int]
X11.XQueryTree.argtypes = [
    ctypes.c_void_p,
    Window,
    ctypes.POINTER(Window),
    ctypes.POINTER(Window),
    ctypes.POINTER(ctypes.POINTER(Window)),
    ctypes.POINTER(ctypes.c_uint),
]
X11.XGetWindowProperty.argtypes = [
    ctypes.c_void_p,
    Window,
    Atom,
    ctypes.c_long,
    ctypes.c_long,
    ctypes.c_int,
    Atom,
    ctypes.POINTER(Atom),
    ctypes.POINTER(ctypes.c_int),
    ctypes.POINTER(ctypes.c_ulong),
    ctypes.POINTER(ctypes.c_ulong),
    ctypes.POINTER(ctypes.c_void_p),
]
X11.XGetWindowAttributes.argtypes = [ctypes.c_void_p, Window, ctypes.POINTER(XWindowAttributes)]
X11.XSendEvent.argtypes = [ctypes.c_void_p, Window, ctypes.c_int, ctypes.c_long, ctypes.POINTER(XEvent)]
X11.XFree.argtypes = [ctypes.c_void_p]
X11.XFlush.argtypes = [ctypes.c_void_p]
X11.XCloseDisplay.argtypes = [ctypes.c_void_p]

XA_CARDINAL = 6
CLIENT_MESSAGE = 33
IS_VIEWABLE = 2


def window_pid(display, window, pid_atom):
    actual_type = Atom()
    actual_format = ctypes.c_int()
    count = ctypes.c_ulong()
    after = ctypes.c_ulong()
    data = ctypes.c_void_p()
    status = X11.XGetWindowProperty(
        display, window, pid_atom, 0, 1, 0, XA_CARDINAL,
        ctypes.byref(actual_type), ctypes.byref(actual_format),
        ctypes.byref(count), ctypes.byref(after), ctypes.byref(data),
    )
    if status != 0 or not data.value:
        return None
    try:
        if count.value < 1 or actual_format.value != 32:
            return None
        return ctypes.cast(data, ctypes.POINTER(ctypes.c_ulong))[0]
    finally:
        X11.XFree(data)


def children(display, window):
    root = Window()
    parent = Window()
    items = ctypes.POINTER(Window)()
    count = ctypes.c_uint()
    if not X11.XQueryTree(display, window, ctypes.byref(root), ctypes.byref(parent), ctypes.byref(items), ctypes.byref(count)):
        return []
    try:
        return [items[i] for i in range(count.value)]
    finally:
        if items:
            X11.XFree(items)


def main():
    pid = int(sys.argv[1])
    display = X11.XOpenDisplay(None)
    if not display:
        print("нет дисплея X11", file=sys.stderr)
        return 2
    try:
        pid_atom = X11.XInternAtom(display, b"_NET_WM_PID", 1)
        protocols = X11.XInternAtom(display, b"WM_PROTOCOLS", 0)
        delete = X11.XInternAtom(display, b"WM_DELETE_WINDOW", 0)
        sent = 0
        for window in children(display, X11.XDefaultRootWindow(display)):
            if window_pid(display, window, pid_atom) != pid:
                continue
            attributes = XWindowAttributes()
            if not X11.XGetWindowAttributes(display, window, ctypes.byref(attributes)):
                continue
            if attributes.map_state != IS_VIEWABLE:
                continue
            event = XEvent()
            event.xclient.type = CLIENT_MESSAGE
            event.xclient.window = window
            event.xclient.message_type = protocols
            event.xclient.format = 32
            event.xclient.data[0] = delete
            event.xclient.data[1] = 0  # CurrentTime
            X11.XSendEvent(display, window, 0, 0, ctypes.byref(event))
            sent += 1
        X11.XFlush(display)
        return 0 if sent else 1
    finally:
        X11.XCloseDisplay(display)


if __name__ == "__main__":
    sys.exit(main())
