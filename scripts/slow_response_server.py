#!/usr/bin/env python3
"""Serve deliberately slow HTTP responses for testing Probe's progress UI."""

from __future__ import annotations

import argparse
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

MEBIBYTE = 1024 * 1024


class SlowResponseHandler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def do_GET(self) -> None:
        if self.path == "/empty":
            self.send_response(204)
            self.send_header("Connection", "close")
            self.end_headers()
            return

        routes = {
            "/known": (200, True),
            "/unknown": (200, False),
            "/error": (500, True),
        }
        route = routes.get(self.path)
        if route is None:
            message = b"Try /known, /unknown, /error, or /empty.\n"
            self.send_response(404)
            self.send_header("Content-Type", "text/plain; charset=utf-8")
            self.send_header("Content-Length", str(len(message)))
            self.send_header("Connection", "close")
            self.end_headers()
            self.wfile.write(message)
            return

        status, include_length = route
        total_bytes = self.server.total_bytes
        chunk_bytes = min(self.server.chunk_bytes, total_bytes)
        chunk_count = (total_bytes + chunk_bytes - 1) // chunk_bytes
        delay = self.server.duration / chunk_count

        self.send_response(status)
        self.send_header("Content-Type", "application/octet-stream")
        self.send_header("Cache-Control", "no-store")
        if include_length:
            self.send_header("Content-Length", str(total_bytes))
        else:
            self.send_header("Transfer-Encoding", "chunked")
        self.send_header("Connection", "close")
        self.end_headers()

        payload = (b"Probe slow response payload.\n" * (chunk_bytes // 29 + 1))[
            :chunk_bytes
        ]
        sent = 0
        try:
            while sent < total_bytes:
                time.sleep(delay)
                body = payload[: min(chunk_bytes, total_bytes - sent)]
                if include_length:
                    self.wfile.write(body)
                else:
                    self.wfile.write(f"{len(body):X}\r\n".encode("ascii"))
                    self.wfile.write(body)
                    self.wfile.write(b"\r\n")
                self.wfile.flush()
                sent += len(body)

            if not include_length:
                self.wfile.write(b"0\r\n\r\n")
                self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError):
            print(f"client disconnected after {sent} bytes", file=sys.stderr)

    def log_message(self, format: str, *args: object) -> None:
        print(f"{self.client_address[0]} - {format % args}", file=sys.stderr)


class SlowResponseServer(ThreadingHTTPServer):
    daemon_threads = True

    def __init__(
        self,
        address: tuple[str, int],
        duration: float,
        total_bytes: int,
        chunk_bytes: int,
    ) -> None:
        super().__init__(address, SlowResponseHandler)
        self.duration = duration
        self.total_bytes = total_bytes
        self.chunk_bytes = chunk_bytes


def positive_float(value: str) -> float:
    parsed = float(value)
    if parsed <= 0:
        raise argparse.ArgumentTypeError("must be greater than zero")
    return parsed


def positive_int(value: str) -> int:
    parsed = int(value)
    if parsed <= 0:
        raise argparse.ArgumentTypeError("must be greater than zero")
    return parsed


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=8080)
    parser.add_argument(
        "--duration",
        type=positive_float,
        default=8.0,
        help="approximate streaming duration in seconds (default: 8)",
    )
    parser.add_argument(
        "--size-mb",
        type=positive_int,
        default=24,
        help="response size in MiB (default: 24)",
    )
    parser.add_argument(
        "--chunk-kb",
        type=positive_int,
        default=256,
        help="chunk size in KiB (default: 256)",
    )
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    server = SlowResponseServer(
        (args.host, args.port),
        args.duration,
        args.size_mb * MEBIBYTE,
        args.chunk_kb * 1024,
    )
    host, port = server.server_address
    print(f"Slow response server listening on http://{host}:{port}")
    print(f"  GET /known   200 with Content-Length over ~{args.duration:g}s")
    print(f"  GET /unknown 200 chunked without Content-Length over ~{args.duration:g}s")
    print(f"  GET /error   500 with Content-Length over ~{args.duration:g}s")
    print("  GET /empty   204 without a body")
    print("Press Ctrl-C to stop.")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("\nStopping server.")
    finally:
        server.server_close()


if __name__ == "__main__":
    main()
