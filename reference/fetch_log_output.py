#!/usr/bin/env python3
"""Rebuild a harness output folder from a workflow job log.

The workflow prints every harness-out/*.json file between "=====BEGIN <path>" and
"=====END <path>" lines. Use this when the artifact cannot be downloaded:

  gh run view <run-id> -R ArneVDA-AP/daytrace --log > run.log
  python3 reference/fetch_log_output.py run.log out/harness-out
"""

import json
import os
import re
import sys

# gh prefixes each line with "<job>\t<step>\t<timestamp> ".
PREFIX = re.compile(r"^(?:[^\t]*\t[^\t]*\t)?﻿?\d{4}-\d\d-\d\dT[\d:.]+Z ?")


def main():
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    log_path, out_dir = sys.argv[1], sys.argv[2]
    os.makedirs(out_dir, exist_ok=True)
    current, lines, written = None, [], []
    with open(log_path, encoding="utf-8", errors="replace") as f:
        for raw in f:
            line = PREFIX.sub("", raw.rstrip("\n"), count=1)
            if line.startswith("=====BEGIN "):
                current, lines = line[len("=====BEGIN "):].strip(), []
            elif line.startswith("=====END ") and current:
                name = os.path.basename(current)
                text = "\n".join(lines)
                json.loads(text)  # refuse to write a damaged file
                with open(os.path.join(out_dir, name), "w", encoding="utf-8") as out:
                    out.write(text + "\n")
                written.append(name)
                current = None
            elif current is not None:
                lines.append(line)
    if not written:
        sys.exit("no =====BEGIN/END blocks found in the log")
    print("wrote " + ", ".join(written) + f" to {out_dir}")


if __name__ == "__main__":
    main()
