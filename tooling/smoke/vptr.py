#!/usr/bin/env python3
"""A mouse for the headless compositor: one click, or the wheel, at X Y (output pixels).

    vptr.py X Y [BUTTON]        BUTTON: left (default), middle, right
    vptr.py X Y wheel-up [N]    N notches of the wheel (default 1); also wheel-down

Speaks the Wayland wire protocol itself (no library in the image has it): binds
zwlr_virtual_pointer_manager_v1, makes a pointer, moves it, presses and releases.
The seat has no pointer until this connects, and the app binds wl_pointer only then, so
there is a pause before the first motion (the same reason `wtype -s 250` is used).
"""
import os
import socket
import struct
import sys
import time

BUTTONS = {"left": 0x110, "right": 0x111, "middle": 0x112}
# Wayland's vertical axis grows downwards. One notch is sent as a smooth scroll of this many
# pixels (protocol version 1 has no discrete steps): three cells of 17 px, the app's 3 rows.
WHEELS = {"wheel-up": -1, "wheel-down": 1}
NOTCH_PX = 51
EXTENT = (1280, 800)


def message(obj, opcode, payload=b""):
    return struct.pack("=IHH", obj, opcode, 8 + len(payload)) + payload


def string(text):
    data = text.encode() + b"\0"
    return struct.pack("=I", len(data)) + data + b"\0" * (-len(data) % 4)


class Conn:
    def __init__(self):
        path = os.path.join(os.environ["XDG_RUNTIME_DIR"], os.environ["WAYLAND_DISPLAY"])
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.settimeout(5)
        self.sock.connect(path)
        self.buf = b""
        self.next_id = 2

    def new_id(self):
        self.next_id += 1
        return self.next_id - 1

    def send(self, data):
        self.sock.sendall(data)

    def events(self):
        """Yields (object, opcode, payload) as they arrive."""
        while True:
            while len(self.buf) >= 8:
                obj, opcode, size = struct.unpack_from("=IHH", self.buf)
                if len(self.buf) < size:
                    break
                payload, self.buf = self.buf[8:size], self.buf[size:]
                yield obj, opcode, payload
            chunk = self.sock.recv(65536)
            if not chunk:
                raise SystemExit("vptr: the compositor closed the connection")
            self.buf += chunk

    def roundtrip(self, on_event=None):
        callback = self.new_id()
        self.send(message(1, 0, struct.pack("=I", callback)))  # wl_display.sync
        for obj, opcode, payload in self.events():
            if obj == callback:
                return
            if obj == 1 and opcode == 0:  # wl_display.error
                raise SystemExit("vptr: protocol error: %r" % payload[8:])
            if on_event:
                on_event(obj, opcode, payload)


def main():
    x, y = int(sys.argv[1]), int(sys.argv[2])
    action = sys.argv[3] if len(sys.argv) > 3 else "left"
    if action not in BUTTONS and action not in WHEELS:
        raise SystemExit("vptr: unknown action %r" % action)
    notches = int(sys.argv[4]) if len(sys.argv) > 4 else 1
    conn = Conn()
    registry = conn.new_id()
    conn.send(message(1, 1, struct.pack("=I", registry)))  # wl_display.get_registry
    found = {}

    def on_global(obj, opcode, payload):
        if obj == registry and opcode == 0:
            name, length = struct.unpack_from("=II", payload)
            interface = payload[8 : 8 + length - 1].decode()
            found[interface] = name

    conn.roundtrip(on_global)
    manager_name = found.get("zwlr_virtual_pointer_manager_v1")
    if manager_name is None:
        raise SystemExit("vptr: the compositor has no zwlr_virtual_pointer_manager_v1")
    manager = conn.new_id()
    bind = struct.pack("=I", manager_name) + string("zwlr_virtual_pointer_manager_v1")
    conn.send(message(registry, 0, bind + struct.pack("=II", 1, manager)))
    pointer = conn.new_id()
    # create_virtual_pointer(seat = none, id)
    conn.send(message(manager, 0, struct.pack("=II", 0, pointer)))
    conn.roundtrip()
    time.sleep(0.3)

    def now():
        return int(time.monotonic() * 1000) & 0xFFFFFFFF

    def frame():
        conn.send(message(pointer, 4))
        conn.roundtrip()

    # motion_absolute(time, x, y, x_extent, y_extent); twice, so the app sees a motion after
    # the enter.
    for dx in (1, 0):
        conn.send(message(pointer, 1, struct.pack("=IIIII", now(), x + dx, y, *EXTENT)))
        frame()
        time.sleep(0.1)
    if action in WHEELS:
        for _ in range(notches):
            conn.send(message(pointer, 5, struct.pack("=I", 0)))  # axis_source(wheel)
            # axis(time, vertical, value as wl_fixed)
            value = WHEELS[action] * NOTCH_PX * 256
            conn.send(message(pointer, 3, struct.pack("=IIi", now(), 0, value)))
            frame()
            time.sleep(0.08)
    else:
        for state in (1, 0):
            conn.send(message(pointer, 2, struct.pack("=III", now(), BUTTONS[action], state)))
            frame()
            time.sleep(0.08)
    time.sleep(0.2)


if __name__ == "__main__":
    main()
