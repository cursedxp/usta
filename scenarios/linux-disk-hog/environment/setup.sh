#!/bin/sh
set -e
mkdir -p /var/log/shopd/worker
dd if=/dev/zero of=/var/log/shopd/access.log bs=1k count=1024 2>/dev/null
dd if=/dev/zero of=/var/log/shopd/worker/debug.log bs=1k count=20480 2>/dev/null
dd if=/dev/zero of=/var/log/shopd/error.log bs=1 count=2048 2>/dev/null
