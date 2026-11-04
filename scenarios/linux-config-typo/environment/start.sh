#!/bin/sh
mkdir -p /var/log/shopd
echo "$(date '+%F %T') INFO services up" >> /var/log/shopd/starts.log
