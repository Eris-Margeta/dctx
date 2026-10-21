#!/usr/bin/env python3
"""Replace third-party AI product and agent names in published data files with neutral placeholders.

usage: scrub_names.py DATA_DIR
The benchmark data contains verbatim tool output from corpora whose code names coding agents (Trail-Code
integrates with several). Numbers, hashes and structure are unchanged; only these words are replaced.
The generic word "cursor" (pagination cursors in dctx) is left alone except in agent lists.
"""
import os
import re
import sys

import codecs

# Product and agent names, rot13-encoded so this file does not itself spell them out.
NAMES = codecs.decode("pynhqr pbqrk trzvav tebx pbcvybg jvaqfhes xveb nagvtenivgl naguebcvp bcranv pungtcg fbaarg bepnebhgre bcraebhgre urezrf", "rot13").split()
RX = re.compile("|".join(NAMES), re.IGNORECASE)
AGENT_LIST = re.compile(r"\(agents, adal, cursor,[^)]*\)")


def main(root):
    for dp, _, fns in os.walk(root):
        for fn in fns:
            p = os.path.join(dp, fn)
            with open(p, "rb") as f:
                b = f.read()
            t = b.decode("utf-8", "surrogateescape")
            n = AGENT_LIST.sub("(<agent ids>)", t)
            n = RX.sub("agentx", n)
            if n != t:
                with open(p, "wb") as f:
                    f.write(n.encode("utf-8", "surrogateescape"))


if __name__ == "__main__":
    main(sys.argv[1])
