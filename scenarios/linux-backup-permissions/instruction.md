The nightly backup script `/opt/app/backup.sh` runs as the user `deniz` and fails with a permission error.

Make it work: when `deniz` runs it, an archive containing `customers.txt` must appear in `/srv/backups`.

Do not edit the script and do not make `/srv/backups` writable for everyone.
