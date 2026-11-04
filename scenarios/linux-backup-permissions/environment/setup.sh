#!/bin/sh
set -e
mkdir -p /opt/app/data /srv/backups
echo "customer list" > /opt/app/data/customers.txt
cat > /opt/app/backup.sh <<'EOS'
#!/bin/sh
if tar -czf "/srv/backups/app-$(date +%F).tgz" -C /opt/app data 2>/tmp/backup.err; then
  echo "backup ok"
else
  echo "backup failed:"
  cat /tmp/backup.err
  exit 1
fi
EOS
chmod 755 /opt/app/backup.sh
chmod -R a+rX /opt/app
chown root:root /srv/backups
chmod 755 /srv/backups
