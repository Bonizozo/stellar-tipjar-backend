#!/bin/bash

# Encrypted Database Backup Script
# This script creates a PostgreSQL dump and encrypts it using AES-256-GCM

set -euo pipefail

# Configuration
BACKUP_DIR="${BACKUP_DIR:-/tmp/backups}"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
BACKUP_NAME="stellar_tipjar_backup_${TIMESTAMP}.sql.enc"
BACKUP_PATH="${BACKUP_DIR}/${BACKUP_NAME}"

# Database connection
if [ -n "$DATABASE_URL" ]; then
    # Parse DATABASE_URL if provided
    # Expected format: postgresql://user:password@host:port/dbname
    DB_USER=$(echo "$DATABASE_URL" | sed -n 's|.*://\([^:]*\):.*|\1|p')
    DB_PASS=$(echo "$DATABASE_URL" | sed -n 's|.*://[^:]*:\([^@]*\)@.*|\1|p')
    DB_HOST=$(echo "$DATABASE_URL" | sed -n 's|.*@\([^:]*\):.*|\1|p')
    DB_PORT=$(echo "$DATABASE_URL" | sed -n 's|.*:\([0-9]*\)/.*|\1|p')
    DB_NAME=$(echo "$DATABASE_URL" | sed -n 's|.*/\([^?]*\).*|\1|p')
    export PGPASSWORD="$DB_PASS"
else
    DB_HOST="${DB_HOST:-localhost}"
    DB_PORT="${DB_PORT:-5432}"
    DB_NAME="${DB_NAME:-stellar_tipjar}"
    DB_USER="${DB_USER:-postgres}"
fi

# Encryption key (should be set via environment)
ENCRYPTION_KEY="${ENCRYPTION_KEY_CURRENT:-}"
if [ -z "$ENCRYPTION_KEY" ]; then
    echo "Error: ENCRYPTION_KEY_CURRENT environment variable not set"
    exit 1
fi

# Create backup directory
mkdir -p "$BACKUP_DIR"

echo "Starting database backup at $(date)"

# Create PostgreSQL dump
pg_dump \
    --host="$DB_HOST" \
    --port="$DB_PORT" \
    --username="$DB_USER" \
    --dbname="$DB_NAME" \
    --no-password \
    --format=custom \
    --compress=9 \
    --verbose \
    --file="${BACKUP_PATH}.dump"

echo "Database dump created: ${BACKUP_PATH}.dump"

# Encrypt the dump file
# Using openssl for encryption with the provided key
openssl enc -aes-256-gcm \
    -in "${BACKUP_PATH}.dump" \
    -out "$BACKUP_PATH" \
    -k "$ENCRYPTION_KEY" \
    -pbkdf2 \
    -iter 10000

echo "Backup encrypted: $BACKUP_PATH"

# Calculate checksum
CHECKSUM=$(sha256sum "$BACKUP_PATH" | awk '{print $1}')

# Get file size
SIZE=$(stat -c%s "$BACKUP_PATH")

# Clean up unencrypted dump
rm "${BACKUP_PATH}.dump"

echo "Backup completed successfully:"
echo "  File: $BACKUP_PATH"
echo "  Size: $SIZE bytes"
echo "  Checksum: $CHECKSUM"

# Output for caller (can be parsed)
echo "{\"file\":\"$BACKUP_PATH\",\"size\":$SIZE,\"checksum\":\"$CHECKSUM\"}"