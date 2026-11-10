#!/usr/bin/env python3
"""MONTI benchmark: scan pinned real-world repos and measure precision per rule against labels.

    python3 bench/run.py                  # precision table
    python3 bench/run.py --sample 15      # also print unlabeled findings to label, with context

Labels live in bench/labels.jsonl, one JSON object per line:
    {"repo": "...", "path": "...", "rule": "...", "fingerprint": "...", "label": "real"|"noise", "why": "..."}
The fingerprint hashes the rule and the finding's line text, so labels survive line moves and MONTI
changes. Precision = real / labeled. "lost real" counts labeled-real findings no longer reported:
a recall regression.
"""

import argparse
import collections
import json
import pathlib
import random
import subprocess
import sys

HERE = pathlib.Path(__file__).resolve().parent


def fetch(cache: pathlib.Path, repo: str, sha: str) -> pathlib.Path:
    dest = cache / repo.split("/")[1]
    if (dest / ".git").exists():
        return dest
    dest.mkdir(parents=True)
    git = lambda *a: subprocess.run(["git", *a], cwd=dest, check=True, capture_output=True)
    git("init", "-q")
    git("fetch", "-q", "--depth", "1", f"https://github.com/{repo}", sha)
    git("checkout", "-q", "FETCH_HEAD")
    return dest


def scan(monti: str, cache: pathlib.Path, name: str) -> list[dict]:
    out = subprocess.run([monti, "scan", "--format", "json", name], cwd=cache, capture_output=True, text=True)
    if out.returncode not in (0, 1):
        sys.exit(f"monti failed on {name}: {out.stderr}")
    findings = []
    for f in json.loads(out.stdout)["files"]:
        repo, _, path = f["path"].partition("/")
        for x in f["findings"]:
            findings.append({"repo": repo, "path": path, "rule": x["rule_id"], "fingerprint": x["fingerprint"],
                             "line": x["line"], "message": x["message"]})
    return findings


def key(d: dict) -> tuple:
    return (d["repo"], d["path"], d["rule"], d["fingerprint"])


def context(cache: pathlib.Path, f: dict, around: int = 4) -> str:
    lines = (cache / f["repo"] / f["path"]).read_text(errors="replace").splitlines()
    lo, hi = max(0, f["line"] - 1 - around), min(len(lines), f["line"] + around)
    return "\n".join(f"{'>' if i + 1 == f['line'] else ' '}{i + 1:5} {lines[i]}" for i in range(lo, hi))


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--monti", default=str(HERE.parent / "target/release/monti"))
    ap.add_argument("--cache", default=str(HERE / ".cache"))
    ap.add_argument("--sample", type=int, default=0, help="print up to N unlabeled findings per rule")
    args = ap.parse_args()
    cache = pathlib.Path(args.cache)

    findings = []
    for line in (HERE / "repos.txt").read_text().splitlines():
        if line.strip() and not line.startswith("#"):
            repo, sha = line.split()
            findings += scan(args.monti, cache, fetch(cache, repo, sha).name)

    labels = {}
    labels_file = HERE / "labels.jsonl"
    if labels_file.exists():
        labels = {key(l): l for l in map(json.loads, labels_file.read_text().splitlines()) if l}
    found = {key(f) for f in findings}

    # Precision counts labels, not findings: identical lines in one file (MATLAB's bare `catch`)
    # share a fingerprint, and one label must not count once per copy.
    per_rule = collections.defaultdict(lambda: collections.Counter())
    for f in findings:
        per_rule[f["rule"]]["findings"] += 1
    lost, kept_noise = [], []
    for k, l in labels.items():
        if k in found:
            per_rule[l["rule"]][l["label"]] += 1
            if l["label"] == "noise":
                kept_noise.append(l)
        elif l["label"] == "real":
            per_rule[l["rule"]]["lost real"] += 1
            lost.append(l)

    print(f"{'rule':24} {'findings':>8} {'labeled':>8} {'real':>5} {'noise':>6} {'precision':>9} {'lost real':>9}")
    for rule, c in sorted(per_rule.items(), key=lambda kv: -kv[1]["findings"]):
        labeled = c["real"] + c["noise"]
        precision = f"{c['real'] / labeled:.0%}" if labeled else "-"
        print(f"{rule:24} {c['findings']:8} {labeled:8} {c['real']:5} {c['noise']:6} {precision:>9} {c['lost real']:9}")

    for title, items in (("lost real (no longer reported)", lost), ("noise still reported", kept_noise)):
        if items:
            print(f"\n{title}:")
            for l in items:
                print(f"  {l['rule']:22} {l['repo']}/{l['path']}  ({l['why']})")

    if args.sample:
        rng = random.Random(0)
        by_rule = collections.defaultdict(list)
        for f in findings:
            if key(f) not in labels:
                by_rule[f["rule"]].append(f)
        for rule, fs in sorted(by_rule.items()):
            for f in rng.sample(fs, min(args.sample, len(fs))):
                print(f"\n### {json.dumps({k: f[k] for k in ('repo', 'path', 'rule', 'fingerprint')})}")
                print(f"# {f['message']}")
                print(context(cache, f))


if __name__ == "__main__":
    main()
