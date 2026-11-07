#!/bin/sh
copy_value() {
  nested() { printf '%s' "$1"; }
  nested "$1"
}
