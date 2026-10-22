//! Inline Python fixture corpus for the integration tests.
//! Covers the Python structural extractor contract (decision 077/decision 078).
//! Shared by focused examples and deterministic prefix checks via `common::fixtures`.
#![forbid(unsafe_code)]

pub const SAMPLE: &str = concat!(
    "\"\"\"Package entry. More text.\"\"\"\n",
    "import os\n",
    "import os.path as osp\n",
    "from . import sibling\n",
    "from ..pkg.mod import name as n, other\n",
    "from typing import *\n",
    "import a.b, c\n",
    "\n",
    "MAX = 3\n",
    "NAME: str = \"py\"\n",
    "\n",
    "@decorator\n",
    "@wrap(helper())\n",
    "async def run(n: int = default()) -> int:\n",
    "    \"\"\"Runs the job. Returns the count.\"\"\"\n",
    "    return os.getcwd()\n",
    "\n",
    "class Box(Base, factory()):\n",
    "    \"\"\"A box.\"\"\"\n",
    "\n",
    "    def __init__(self, size=seed()):\n",
    "        self.n = size\n",
    "\n",
    "    def _hidden(self):\n",
    "        return self.n\n",
    "\n",
    "    @staticmethod\n",
    "    def make():\n",
    "        return run()\n",
    "\n",
    "def nested():\n",
    "    def inner():\n",
    "        helper()\n",
);

pub const MASKING: &str = concat!(
    "def real():\n",
    "    return f\"x{compute(1)}y\"\n",
    "t\"t{compute(2)}\"\n",
    "s = b\"compute()\"\n",
    "s2 = \"compute()\"\n",
    "# compute()\n",
    "f\"{1 # compute()\n",
    "}\"\n",
    "f\"{x:#compute()}\"\n",
    "f\"{name:{fmt()}}\"\n",
);

pub const CONSTS: &str = concat!(
    "(A, B) = pair()\n",
    "A = B = 1\n",
    "C += 1\n",
    "NAME: int\n",
    "if True:\n",
    "    D = 1\n",
    "type Alias = int\n",
    "_MAX = 1\n",
);

pub const CONT: &str = "VALUE = helper(\\\n    1)\n";
pub const SEMI: &str = "MAX = 1; MIN = 2\n";

pub const MATCH_SRC: &str = concat!(
    "def outer():\n",
    "    match subject():\n",
    "        case Point(1, 2):\n",
    "            inside()\n",
    "        case other if guard():\n",
    "            taken()\n",
);

pub const PARTIAL_SRC: &str = concat!(
    "def kept():\n",
    "    return 1\n",
    "x = \"unterminated\n",
    "def also():\n",
    "    return call()\n",
);

pub const TRIPLE_OPEN: &str = concat!(
    "def kept():\n",
    "    return 1\n",
    "x = \"\"\"still\n",
    "def swallowed():\n",
    "    pass\n",
);

pub const TRIPLE_CLOSED: &str = concat!(
    "s = \"\"\"\n",
    "def fake():\n",
    "    pass\n",
    "\"\"\"\n",
    "real()\n",
);

pub const PAREN_SRC: &str = concat!(
    "def kept():\n",
    "    return 1\n",
    "x = (\n",
    "def later():\n",
    "    pass\n",
);

pub const IMPORTS_SRC: &str = concat!(
    "from ... import *\n",
    "def f():\n",
    "    from os import path as p, other\n",
    "    import a.b\n",
    "from x import (\n",
    "    a,\n",
    "    b as c,\n",
    ")\n",
    "from import missing\n",
);

pub const CALLS_SRC: &str = concat!(
    "def f():\n",
    "    (lambda: 1)()\n",
    "    (fn)()\n",
    "    obj[k]()\n",
    "    obj.method()\n",
    "    with open(path) as h:\n",
    "        h.read()\n",
    "    try:\n",
    "        pass\n",
    "    except Error():\n",
    "        recover()\n",
    "    return items()\n",
);

pub const ASYNC_FOR: &str = concat!(
    "async def f():\n",
    "    async for x in items():\n",
    "        use(x)\n",
);

pub const DOC_SRC: &str = concat!(
    "def raw():\n",
    "    r\"\"\"Raw doc. More.\"\"\"\n",
    "    return 1\n",
    "\n",
    "def binary():\n",
    "    b\"\"\"not a doc\"\"\"\n",
    "    return 1\n",
    "\n",
    "def escaped():\n",
    "    \"Line\\nTwo. More\"\n",
    "    return 1\n",
    "\n",
    "def joined():\n",
    "    \"Hello. \" \"World.\"\n",
    "    return 1\n",
    "\n",
    "def wrapped():\n",
    "    (\"Only one.\")\n",
    "    return 1\n",
    "\n",
    "def formatted():\n",
    "    f\"not {1} a doc\"\n",
    "    return 1\n",
);

pub const SHEBANG: &str = "#!/usr/bin/env python3\n# First sentence. Second.\ndef f():\n    pass\n";
pub const ORDINALS: &str = "def f():\n    pass\ndef f():\n    pass\n";
pub const BOM_SRC: &str = "\u{feff}def f():\n    pass\n";
pub const CRLF_SRC: &str = "def f():\r\n    return g()\r\n";
pub const TABS_SRC: &str = "def f():\n\treturn g()\n";
pub const MIXED_SRC: &str = "def f():\n    return 1\n \treturn g()\n";
pub const DEDENT_SRC: &str = "def f():\n    return 1\n  return 2\n";
pub const UNICODE_SRC: &str = "def café():\n    return \"数\"\n";
pub const ONELINER: &str = "def a(): def b(): def c(): pass\n";
pub const FNEST: &str = "x = f\"{f'{f\"{1}\"}'}\"\n";

pub const FIXTURES: &[&str] = &[
    SAMPLE,
    MASKING,
    CONSTS,
    CONT,
    SEMI,
    MATCH_SRC,
    PARTIAL_SRC,
    TRIPLE_OPEN,
    TRIPLE_CLOSED,
    PAREN_SRC,
    IMPORTS_SRC,
    CALLS_SRC,
    ASYNC_FOR,
    DOC_SRC,
    SHEBANG,
    UNICODE_SRC,
    CRLF_SRC,
    TABS_SRC,
    MIXED_SRC,
    DEDENT_SRC,
    ORDINALS,
    BOM_SRC,
    ONELINER,
    FNEST,
];
