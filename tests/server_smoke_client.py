"""Synthetic localhost-only smoke client for a running t9-server.

Usage: python tests/server_smoke_client.py
Does not contact the game, Steam, or any external service; sends no credentials.
Prints only response metadata, never tickets or response bodies. A FAIL is an
observed protocol limitation, not a claim that the real game uses that path.
"""

import base64
import json
import socket
import struct
import time

HOST = "127.0.0.1"
# This is a response deadline for deliberately non-answering protocol probes,
# not a limit on how long the emulator process is allowed to run.
TIMEOUT = 5.0
failures = 0


def result(name, ok, detail):
    global failures
    print(f"{'PASS' if ok else 'FAIL'} {name}: {detail}")
    if not ok:
        failures += 1


def connection(port):
    return socket.create_connection((HOST, port), timeout=TIMEOUT)


def http_response(stream):
    data = b""
    while b"\r\n\r\n" not in data:
        part = stream.recv(8192)
        if not part:
            raise EOFError("connection closed before HTTP headers")
        data += part
    head, body = data.split(b"\r\n\r\n", 1)
    fields = {}
    for line in head.split(b"\r\n")[1:]:
        key, _, val = line.partition(b":")
        fields[key.lower()] = val.strip()
    length = int(fields[b"content-length"])
    while len(body) < length:
        part = stream.recv(8192)
        if not part:
            raise EOFError("connection closed before HTTP body")
        body += part
    return head.split(b"\r\n", 1)[0].decode("ascii"), body[:length]


def get(port, path):
    with connection(port) as stream:
        stream.sendall(f"GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n".encode())
        return http_response(stream)


try:
    status, body = get(3100, "/v1/login/steam/?titleID=27551&client=pc")
    payload = json.loads(body)
    result(
        "rest_login",
        status == "HTTP/1.1 200 OK"
        and payload["lsg"]["host"] == "127.0.0.1:3200"
        and len(base64.b64decode(payload["token"])) == 128,
        f"{status}; LSG={payload['lsg']['host']}; ticket_bytes={len(base64.b64decode(payload['token']))}",
    )
except (OSError, ValueError, KeyError, EOFError) as exc:
    result("rest_login", False, f"{type(exc).__name__}: {exc}")

try:
    status, body = get(3300, "/synthetic")
    result("umbrella_http", status == "HTTP/1.1 200 OK" and body == b"{}", f"{status}; body_bytes={len(body)}")
except (OSError, ValueError, KeyError, EOFError) as exc:
    result("umbrella_http", False, f"{type(exc).__name__}: {exc}")

for question, answer, length, name in [(30, 31, 9, "stun_ip"), (20, 21, 15, "stun_nat")]:
    try:
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
            sock.settimeout(TIMEOUT)
            sock.sendto(bytes([question, 2, 0]), (HOST, 3074))
            reply, peer = sock.recvfrom(1024)
        good = peer[0] == HOST and len(reply) == length and reply[:7] == bytes([answer, 2, 0, 127, 0, 0, 1])
        result(name, good, f"reply_type={reply[0]}; reply_bytes={len(reply)}; source={peer[0]}")
    except OSError as exc:
        result(name, False, f"{type(exc).__name__}: {exc}")

header_ack = struct.pack("<II", 0xC8, 0xC8) + bytes([0x42]) * 16
auth = struct.pack("<I", 0x2A) + b"\xab\x82" + bytes([0x55]) * 32 + bytes([0x99]) * 8
try:
    with connection(3200) as stream:
        stream.settimeout(TIMEOUT)
        stream.sendall(header_ack)
        ack = stream.recv(26)
        result("lobby_header", len(ack) == 26 and ack[:6] == b"\x16\x00\x00\x00\xab\x81", f"reply_bytes={len(ack)}; message_type={ack[5] if len(ack) > 5 else 'none'}")
        stream.sendall(auth)
        done = stream.recv(14)
        result("lobby_auth_synthetic", len(done) == 14 and done[:6] == b"\x0a\x00\x00\x00\xab\x83", f"reply_bytes={len(done)}; message_type={done[5] if len(done) > 5 else 'none'}")
except OSError as exc:
    result("lobby_handshake", False, f"{type(exc).__name__}: {exc}")

# TCP is a byte stream; a valid packet must not depend on one write == one read.
try:
    with connection(3200) as stream:
        stream.sendall(header_ack[:2])
        time.sleep(0.12)
        stream.sendall(header_ack[2:])
        reply = stream.recv(26)
        result("lobby_fragmented_header", reply[:6] == b"\x16\x00\x00\x00\xab\x81", f"expected server header ACK; got_prefix={reply[:6].hex()}")
except OSError as exc:
    result("lobby_fragmented_header", False, f"{type(exc).__name__}: {exc}")

# The legacy endpoint must reply to a complete, ordinary HTTP request.
try:
    with connection(3100) as stream:
        stream.sendall(b"GET /auth/ HTTP/1.1\r\nHost: localhost\r\n\r\n")
        status, body = http_response(stream)
        result("legacy_auth_http", status.startswith("HTTP/1.1 "), f"{status}; body_bytes={len(body)}")
except (OSError, ValueError, KeyError, EOFError) as exc:
    result("legacy_auth_http", False, f"{type(exc).__name__}: {exc}")

synthetic_body = b'{"title_id":"27551","iv_seed":"1","identity":"synthetic"}'
auth_header = (
    b"POST /auth/ HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\n"
    + f"Content-Length: {len(synthetic_body)}\r\n\r\n".encode()
)
for split, name in [(False, "auth_post_single_write"), (True, "auth_post_split_write")]:
    try:
        with connection(3100) as stream:
            if split:
                stream.sendall(auth_header)
                time.sleep(0.12)
                stream.sendall(synthetic_body)
            else:
                stream.sendall(auth_header + synthetic_body)
            status, body = http_response(stream)
            payload = json.loads(body)
            result(name, status == "HTTP/1.1 200 OK" and payload.get("code") == "700", f"{status}; response_code={payload.get('code')}")
    except (OSError, ValueError, KeyError, EOFError) as exc:
        result(name, False, f"{type(exc).__name__}: {exc}")

# This is a deliberately incomplete TLS record, NOT a TLS client or real credentials.
# Plain HTTP sent back to it shows the listener cannot negotiate TLS as written.
try:
    with connection(3100) as stream:
        stream.sendall(b"\x16\x03\x03\x00\x00")
        head = stream.recv(64)
        result("tls_on_login_port", not head.startswith(b"HTTP/1.1"), f"reply_prefix={head[:15]!r}; plain_HTTP={head.startswith(b'HTTP/1.1')}")
except OSError as exc:
    result("tls_on_login_port", False, f"{type(exc).__name__}: {exc}")

print("NOT_RUN real game/proxy: synthetic localhost client cannot establish game compatibility or egress isolation")
print(f"SUMMARY failures={failures}")
raise SystemExit(1 if failures else 0)
