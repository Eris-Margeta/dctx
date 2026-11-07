"""Sum the quantities of every saved order: python3 tools/report.py orders/"""
import sys
from pathlib import Path


def read_orders(folder):
    for path in sorted(Path(folder).glob("*.tsv")):
        order_id, item, qty, _checksum = path.read_text().rstrip("\n").split("\t")
        yield order_id, item, int(qty)


def main(folder="orders"):
    total = sum(qty for _, _, qty in read_orders(folder))
    print(f"{total} items")


if __name__ == "__main__":
    main(*sys.argv[1:])
