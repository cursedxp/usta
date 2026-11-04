#!/bin/sh
# Runs inside the container only at check time. Exit 0 = solved, 1 = not solved.
d=/var/log/shopd
if [ ! -f "$d/worker/debug.log" ]; then
  echo "the largest log was deleted; it has to stay in place"
  exit 1
fi
if [ "$(stat -c %s "$d/worker/debug.log")" -ge 1048576 ]; then
  echo "the largest log still takes its space"
  exit 1
fi
if [ "$(stat -c %s "$d/access.log" 2>/dev/null)" != 1048576 ]; then
  echo "another log was changed"
  exit 1
fi
if [ "$(stat -c %s "$d/error.log" 2>/dev/null)" != 2048 ]; then
  echo "another log was changed"
  exit 1
fi
total=$(find "$d" -type f -exec stat -c %s {} + 2>/dev/null | awk '{s+=$1} END {print s+0}')
if [ "$total" -ge 5242880 ]; then
  echo "the space is still used somewhere under $d"
  exit 1
fi
echo "ok"
exit 0
