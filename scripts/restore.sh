#!/bin/bash
set -e

BACKUP_FILE=$1

if [ -z "$BACKUP_FILE" ] || [ ! -f "$BACKUP_FILE" ]; then
    echo "Usage: ./restore.sh <path_to_compressed_sql_file>"
    exit 1
fi

echo "WARNING: This will overwrite the current database. Proceed? (y/n)"
read -r confirm
if [ "$confirm" != "y" ]; then
    echo "Restore aborted."
    exit 0
fi

echo "Restoring from $BACKUP_FILE..."
gunzip -c "$BACKUP_FILE" | psql "$DATABASE_URL"

echo "Restore completed successfully."