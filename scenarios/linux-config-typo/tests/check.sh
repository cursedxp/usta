#!/bin/sh
# Runs inside the container only at check time. Exit 0 = solved, 1 = not solved.
# Re-reads the configuration itself; does not trust the learner-writable program.
port=$(grep -h '^listen_port' /etc/shopd/conf.d/*.conf 2>/dev/null | tail -1 | cut -d= -f2 | tr -d ' ')
case "$port" in
  ''|*[!0-9]*)
    echo "listen_port is not a number: '$port'"
    exit 1 ;;
esac
if [ "$port" -lt 1024 ] || [ "$port" -gt 65535 ]; then
  echo "listen_port is out of range"
  exit 1
fi
echo "ok"
exit 0
