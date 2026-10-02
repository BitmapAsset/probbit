#!/usr/bin/env python3
"""Serve a directory on 127.0.0.1 for the installer tests (stdlib, Python >= 3.7).

    python3 scripts/bench/serve.py <directory> <port>

Same handler as `python3 -m http.server`, minus its startup reverse-DNS lookup: HTTPServer.server_bind calls
socket.getfqdn(), which stalled ~35 s per start on the GitHub macOS runners (bench run 36969399069).
"""
import functools, http.server, socketserver, sys

directory, port = sys.argv[1], int(sys.argv[2])
socketserver.TCPServer.allow_reuse_address = True
handler = functools.partial(http.server.SimpleHTTPRequestHandler, directory=directory)
with socketserver.TCPServer(("127.0.0.1", port), handler) as server:
    server.serve_forever()
