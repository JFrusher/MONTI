#!/bin/bash
set -euo pipefail
deploy() {
  rsync -a build/ "server:/srv/$1"
}
