# Operations Manual

## Overview

This manual provides guidance for operating, deploying, monitoring, and maintaining the Stellar Tip Jar backend.

The backend is a Rust-based API service that enables tip creation and management using the Stellar network.

---

## System Requirements

- **CPU**: 2+ cores
- **RAM**: 4GB minimum (8GB recommended)
- **Storage**: 20GB+ SSD
- **Network**: Stable internet connection

---

## Service Dependencies

- **PostgreSQL** (required)
- **Stellar Horizon API** (testnet or mainnet)
- **Environment Variables (.env)**

Optional (recommended for production):
- Reverse proxy (NGINX)
- Monitoring tools (Prometheus/Grafana)

---

## Architecture Overview

The system consists of:

- **Rust Backend API**
  - Handles HTTP requests
  - Processes tip logic
  - Interacts with Stellar network

- **Database Layer (PostgreSQL)**
  - Stores creators, tips, transactions

- **External Services**
  - Stellar Horizon API for blockchain interactions

---

## Access and Credentials

Ensure secure handling of:

- Database connection string (`DATABASE_URL`)
- Stellar network configuration
- Secret keys (if applicable)
- Environment variables

---

## Environment Variables

Typical required variables:

```env
DATABASE_URL=postgres://user:password@localhost:5432/tipjar
PORT=8000
STELLAR_NETWORK=testnet
HORIZON_URL=https://horizon-testnet.stellar.org
```

## On-Call Procedures

### Escalation Path
1. Backend engineer
2. DevOps/infra engineer
3. Project maintainer

### Response Time SLA
| Severity | Response Time |
| -------- | ------------- |
| Critical | Immediate     |
| High     | < 15 mins     |
| Medium   | < 1 hour      |


### Incident Severity Levels
- **SEV1** — Full outage or data loss
- **SEV2** — Major feature broken
- **SEV3** — Minor issues

## Communication Channels
- GitHub Issues
- Team Slack/Discord (if available)
- Email (for critical alerts)

## Logging
- Logs are output via stdout/stderr
- Recommended:
   - Store logs using journald or log files
   - Rotate logs regularly

## Health Checks
Ensure the API exposes or supports:
- Basic health endpoint (e.g., `/health`)
- Database connectivity checks

## Security Notes
-nNever commit `.env` files
- Use secure credentials storage
- Restrict database access
- Validate all inputs



