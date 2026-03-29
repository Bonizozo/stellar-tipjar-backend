# Backup and Recovery

## Overview

This document outlines procedures to ensure data safety, backup integrity, and recovery from disasters for Stellar Tip Jar backend.

---

## Backup Strategy

- **Full Backup:** Daily at 2 AM UTC
- **Incremental Backup:** Every 6 hours
- **Retention:** 30 days
- **Storage:** Encrypted S3 bucket

---

## Backup Verification

- Weekly restore test
- Integrity checks on backup files
- Ensure backups are not corrupted

Example:

```bash
aws s3 cp s3://stellar-tipjar-backups/latest-backup.sql ./restore-test.sql
```
- Restore test to a staging database

## Recovery Procedures
- Identify the latest valid backup
- Restore database:
```bash
psql $DATABASE_URL < latest-backup.sql
```
- Restore configuration files if needed
- Restart backend service:
```bash
systemctl restart tipjar-backend
```
- Verify service and database are functional

## Disaster Recovery (DR) Plan
- Maintain a secondary environment (staging) to test recovery
- Document RTO (Recovery Time Objective) and RPO (Recovery Point Objective)
- Notify stakeholders of DR execution
- Post-incident review to improve DR processes
