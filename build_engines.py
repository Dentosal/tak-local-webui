#!/usr/bin/env python3

import subprocess
from pathlib import Path
import shlex

ENGINE_REPOS = [
    (
        "https://github.com/MortenLohne/tiltak",
        "cargo build --release --bin tei --features smol",
        [
            "target/release/tei",
            ["slatebot", "target/release/tei --slatebot"],
            ["cobblebot", "target/release/tei --cobblebot"],
        ],
    ),
    (
        "https://github.com/nelhage/taktician",
        "make build; go build -o taktician cmd/taktician/main.go",
        ["taktician tei"],
    ),
    ("https://git.sr.ht/~tslil/ctak", "make native", ["cttei"]),
]

ENGINES = Path("engines")
ENGINES.mkdir(exist_ok=True)


def pull_and_build_engine(repo_url, build_cmd, run_cmds):
    name = repo_url.rsplit("/", 1)[-1]
    if name.endswith(".git"):
        name = name[:-4]
    if (ENGINES / name).exists():
        subprocess.run(["git", "pull"], check=True, cwd=ENGINES / name)
    else:
        subprocess.run(["git", "clone", repo_url, str(ENGINES / name)], check=True)

    assert (ENGINES / name).exists()

    subprocess.run(build_cmd, shell=True, check=True, cwd=ENGINES / name)

    for cmd in run_cmds:
        if isinstance(cmd, list):
            name_override, run_cmd = cmd
        else:
            name_override = ""
            run_cmd = cmd

        bin_path = shlex.split(run_cmd)[0]
        assert (
            ENGINES / name / bin_path
        ).exists(), f"Build failed for {name}, {bin_path!r} not found"

        yield (name_override + "=" if name_override else "") + str(
            ENGINES / name
        ) + "/" + run_cmd


results = [run_cmd for args in ENGINE_REPOS for run_cmd in pull_and_build_engine(*args)]
with open("enginelist.txt", "w") as f:
    f.write("\n".join(results) + "\n")

subprocess.run(["cargo", "run", "--", "--check", "-F", "enginelist.txt"], check=True)
print("All engines built and verified successfully.")
