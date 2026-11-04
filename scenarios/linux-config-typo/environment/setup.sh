#!/bin/sh
set -e
mkdir -p /opt/shopd /etc/shopd/conf.d /var/log/shopd
cat > /opt/shopd/shopd <<'EOT'
#!/bin/sh
conf=/etc/shopd/conf.d
file=$(grep -l '^listen_port' $conf/*.conf | tail -1)
port=$(grep -h '^listen_port' $conf/*.conf | tail -1 | cut -d= -f2 | tr -d ' ')
case "$port" in
  ''|*[!0-9]*)
    echo "shopd: invalid configuration, see /var/log/shopd/shopd.log" >&2
    echo "$(date '+%F %T') ERROR invalid listen_port '$port' in $file" >> /var/log/shopd/shopd.log
    exit 1 ;;
esac
echo "shopd: configuration ok (port $port)"
EOT
chmod 755 /opt/shopd/shopd
printf 'listen_port = 8080\nworkers = 4\n' > /etc/shopd/conf.d/10-base.conf
printf 'name = shop\nlisten_port = 80g0\n' > /etc/shopd/conf.d/20-site.conf
