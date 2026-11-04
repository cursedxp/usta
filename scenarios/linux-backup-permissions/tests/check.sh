#!/bin/sh
# Runs inside the container only at check time. Checks system state, not commands typed.
# Exit codes: 0 = solved, 1 = not solved. Nothing else: container.rs treats
# 125-127 and 137 as infrastructure failures, so every path below exits 0 or 1.
rm -f /srv/backups/app-*.tgz
if ! runuser -u deniz -- /opt/app/backup.sh >/dev/null 2>&1; then
  echo "the backup script still fails when deniz runs it"
  exit 1
fi
if ! tar -tzf /srv/backups/app-*.tgz 2>/dev/null | grep -q 'customers.txt'; then
  echo "no archive containing customers.txt in /srv/backups"
  exit 1
fi
mode=$(stat -c %a /srv/backups 2>/dev/null) || mode=""
if [ -z "$mode" ]; then
  echo "cannot read /srv/backups"
  exit 1
fi
if [ $(( 0$mode & 2 )) -ne 0 ]; then
  echo "/srv/backups is writable for everyone"
  exit 1
fi
echo "ok"
exit 0
