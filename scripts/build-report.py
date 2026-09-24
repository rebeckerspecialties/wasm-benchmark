#!/usr/bin/env python3
"""Fill a report's table markers from scripts/summarize-pass.py output.

Usage: build-report.py REPORT.md TABLES_DIR
TABLES_DIR is summarize-pass.py's <tables-dir> (by default
<pass-root>/report-tables). Every line `<!-- T:name -->` in REPORT.md is
replaced (in place, between the marker and a matching `<!-- /T:name -->`)
by TABLES_DIR/tables-name.md,
so the report can be rebuilt after a data refresh without touching the
prose around the tables.
"""
import os
import re
import sys


def main():
    report, data = sys.argv[1], sys.argv[2]
    text = open(report).read()

    def fill(m):
        name = m.group(1)
        path = os.path.join(data, f"tables-{name}.md")
        body = open(path).read().rstrip() + "\n" if os.path.exists(path) else f"(no data: {path})\n"
        return f"<!-- T:{name} -->\n{body}<!-- /T:{name} -->"

    new = re.sub(r"<!-- T:([\w-]+) -->(?:.*?<!-- /T:\1 -->)?", fill, text, flags=re.S)
    open(report, "w").write(new)
    print(f"filled {len(re.findall(r'<!-- T:', new))} table markers in {report}")


if __name__ == "__main__":
    main()
