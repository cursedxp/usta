## open
06:40. Deniz again. The backups you repaired are running, but now monitoring says the disk is nearly full: "The shop cannot take orders when it hits the top."

Your task: find the single biggest file under `/var/log`, free its space, and keep the file where it is, because the service holds it open. Do not touch the other logs.

## close
06:58. Deniz: "Alert cleared." The orders flow again, and you now have a habit worth keeping: look for the biggest file first.
