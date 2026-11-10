#!/bin/bash
deploy() {
  rsync -a build/ server:/srv || true
}
