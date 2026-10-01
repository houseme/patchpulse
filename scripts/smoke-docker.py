#!/usr/bin/env python3
"""Build-independent smoke verification of the real Docker image and its API."""
import argparse
import json
from pathlib import Path
import subprocess
import time
import urllib.error
import urllib.request
import uuid


def docker(*args):
    return subprocess.check_output(["docker", *args], text=True).strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--image", default="patchpulse:0.1.0")
    parser.add_argument("--report", type=Path, default=Path("target/docker-smoke.json"))
    args = parser.parse_args()
    name = "patchpulse-smoke-" + uuid.uuid4().hex[:12]
    container = docker("run", "-d", "--name", name, "--read-only", "--cap-drop", "ALL",
                       "--security-opt", "no-new-privileges", "-p", "127.0.0.1::9100", args.image)
    checks = {}
    try:
        port = docker("port", container, "9100/tcp").splitlines()[0].rsplit(":", 1)[1]
        base = "http://127.0.0.1:" + port

        def request(path, method="GET"):
            try:
                response = urllib.request.urlopen(urllib.request.Request(base + path, method=method), timeout=3)
            except urllib.error.HTTPError as error:
                response = error
            with response:
                return response.status, response.read(), {key.lower(): value for key, value in response.headers.items()}

        deadline = time.monotonic() + 30
        while True:
            try:
                if request("/health")[0] == 200:
                    break
            except urllib.error.URLError:
                pass
            if time.monotonic() >= deadline:
                raise AssertionError("Container HTTP startup timed out")
            time.sleep(0.2)
        for path, expected in [("/health", 200), ("/ready", 503), ("/version", 200),
                               ("/patches", 200), ("/patches/pending", 200),
                               ("/patches/summary", 200), ("/metrics", 200),
                               ("/patches?since=invalid", 400), ("/missing", 404)]:
            status, body, headers = request(path)
            if status != expected:
                raise AssertionError(f"{path}: expected {expected}, got {status}")
            checks[path] = status
            if path == "/metrics":
                if "text/plain; version=0.0.4" not in headers["content-type"]:
                    raise AssertionError("Incorrect metrics content type")
                if b"patchpulse_stale 1" not in body:
                    raise AssertionError("Unsupported host must be stale")
            if path == "/patches/summary":
                summary = json.loads(body)
                if not summary["is_stale"] or summary["total_installed"] != 0:
                    raise AssertionError("Linux must not fabricate Windows inventory")
        if request("/patches", "POST")[0] != 405:
            raise AssertionError("Write methods must be rejected")
        if request("/health", "HEAD")[0] != 200:
            raise AssertionError("HEAD liveness probe failed")
        docker("exec", container, "patchpulse", "--config", "/etc/patchpulse/patchpulse.toml", "--healthcheck")
        image = json.loads(docker("image", "inspect", args.image))[0]
        if image["Config"]["User"] != "65532:65532":
            raise AssertionError("Runtime must be non-root")
        logs = docker("logs", container)
        if "requires Windows" not in logs:
            raise AssertionError("Unsupported collectors must log failures")
        docker("stop", "--time", "15", container)
        state = json.loads(docker("inspect", container))[0]
        if not state["HostConfig"]["ReadonlyRootfs"] or state["HostConfig"]["CapDrop"] != ["ALL"]:
            raise AssertionError("Runtime hardening options were not applied")
        if state["State"]["ExitCode"] != 0:
            raise AssertionError("SIGTERM shutdown failed")
        report = {"image": args.image, "image_id": image["Id"], "architecture": image["Architecture"],
                  "size_bytes": image["Size"], "non_root": True, "read_only": True,
                  "healthcheck": "passed", "sigterm_exit_code": state["State"]["ExitCode"],
                  "endpoints": checks}
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(report, indent=2) + "\n")
        print(json.dumps(report, indent=2))
    finally:
        subprocess.run(["docker", "rm", "-f", container], check=False, stdout=subprocess.DEVNULL)


if __name__ == "__main__":
    main()
