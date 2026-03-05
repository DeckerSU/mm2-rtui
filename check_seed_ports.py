#!/usr/bin/env python3
"""
Reads seed-nodes.json, computes the second port (other_ports + 20) via lp_ports formula,
and checks whether that port is open on each node's host.
"""

import json
import socket
import sys
from pathlib import Path

LP_RPCPORT = 7783
MAX_NETID = (65535 - 40 - LP_RPCPORT) // 4


def lp_ports(netid: int) -> tuple[int, int, int]:
    """Ports per Rust formula: (other_ports+10, other_ports+20, other_ports+30)."""
    if netid > MAX_NETID:
        raise ValueError(f"netid {netid} > MAX_NETID {MAX_NETID}")
    if netid == 0:
        other_ports = LP_RPCPORT
    else:
        net_mod = netid % 10
        net_div = netid // 10
        other_ports = (net_div * 40) + LP_RPCPORT + net_mod
    return (other_ports + 10, other_ports + 20, other_ports + 30)


def is_port_open(host: str, port: int, timeout: float = 3.0) -> bool:
    """Check if port is open on host (TCP connect)."""
    try:
        with socket.create_connection((host, port), timeout=timeout):
            return True
    except (socket.timeout, socket.error, OSError):
        return False


def main() -> None:
    script_dir = Path(__file__).resolve().parent
    json_path = script_dir / "seed-nodes.json"

    if not json_path.exists():
        print(f"File not found: {json_path}", file=sys.stderr)
        sys.exit(1)

    with open(json_path, encoding="utf-8") as f:
        nodes = json.load(f)

    if not nodes:
        print("No entries in seed-nodes.json")
        return

    timeout = 3.0
    working = 0
    dead = 0

    for node in nodes:
        name = node.get("name", "?")
        host = node.get("host", "")
        netid = node.get("netid")
        if netid is None:
            print(f"{name} ({host}): no netid — skip")
            continue
        try:
            _, port, _ = lp_ports(netid)
        except ValueError as e:
            print(f"{name} ({host}): {e}")
            dead += 1
            continue
        open_ = is_port_open(host, port, timeout=timeout)
        status = "OK" if open_ else "closed"
        print(f"{name} ({host}): netid={netid} port={port} — {status}")
        if open_:
            working += 1
        else:
            dead += 1

    print()
    print(f"Working nodes: {working}, dead nodes: {dead}")
    sys.exit(0 if dead == 0 else 1)


if __name__ == "__main__":
    main()
