#!/usr/bin/env python3
"""Build fix applied to the CI checkout of Dayflow (never to the analysis code).

Xcode 26.3, the newest Xcode on GitHub's macos-15 runner, cannot type-check one long string
concatenation in Core/Flow/FlowSessionTimeline.swift ("unable to type-check this expression in
reasonable time"), even with raised solver limits. That function builds prompt JSON for Dayflow's
Flow feature and is not used by the analysis path the harness runs. This script rewrites the
expression as separate statements that produce the same string. It stops with an error if the
original text is not found, so a different Dayflow commit cannot be patched silently.

Usage: python3 build_fix.py <path to Dayflow checkout>
"""

import pathlib
import sys

FILE = "Dayflow/Dayflow/Core/Flow/FlowSessionTimeline.swift"

ORIGINAL = '''    let rows = entries.map { entry in
      "{\\"start\\":\\"\\(Self.clock(entry.startedAt))\\",\\"end\\":\\"\\(Self.clock(entry.endedAt))\\","
        + "\\"kind\\":\\"\\(entry.kind.rawValue)\\",\\"title\\":\\(Self.jsonString(entry.title))"
        + (entry.category.map { ",\\"category\\":\\(Self.jsonString($0))" } ?? "")
        + (entry.site.map { ",\\"site\\":\\(Self.jsonString($0))" } ?? "")
        + (entry.completedTasks.map { tasks in
          ",\\"completed\\":[" + tasks.map(Self.jsonString).joined(separator: ",") + "]"
        } ?? "") + "}"
    }
'''

REPLACEMENT = '''    // Daytrace reference harness: split into statements so Xcode 26.3 can type-check it.
    let rows = entries.map { entry -> String in
      var row = "{\\"start\\":\\"\\(Self.clock(entry.startedAt))\\",\\"end\\":\\"\\(Self.clock(entry.endedAt))\\","
      row += "\\"kind\\":\\"\\(entry.kind.rawValue)\\",\\"title\\":\\(Self.jsonString(entry.title))"
      if let category = entry.category { row += ",\\"category\\":\\(Self.jsonString(category))" }
      if let site = entry.site { row += ",\\"site\\":\\(Self.jsonString(site))" }
      if let tasks = entry.completedTasks {
        let items: [String] = tasks.map { Self.jsonString($0) }
        row += ",\\"completed\\":[" + items.joined(separator: ",") + "]"
      }
      row += "}"
      return row
    }
'''


def main():
    path = pathlib.Path(sys.argv[1]) / FILE
    text = path.read_text(encoding="utf-8")
    if text.count(ORIGINAL) != 1:
        sys.exit(f"build_fix: expected expression not found exactly once in {path}")
    path.write_text(text.replace(ORIGINAL, REPLACEMENT), encoding="utf-8")
    print(f"build_fix: rewrote promptTimelineJSON row expression in {FILE}")


if __name__ == "__main__":
    main()
