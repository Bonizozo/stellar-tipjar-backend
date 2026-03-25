# Deployment Guide

A guide for deploying the Nova Launch TipJar API to production and staging environments.

---

## Prerequisites

- **Environment**: Linux-based (Ubuntu 22.04+ recommended)
- **Database**: PostgreSQL 15+
- **Cache**: Redis 6+ (with persistence recommended)
- **Runtime**: Docker 24+ (Recommended for containerized deployments)

---

## Environment Variables

| Variable | Required | Default | Description |
| :--- | :---: | :--- | :--- |
| **`DATABASE_URL`** | Yes | - | PostgreSQL connection string. |
| **`REDIS_URL`** | No | `localhost:6379` | Redis connection URL. |
| **`PORT`** | No | `8000` | Local port to bind the API. |
| **`JWT_SECRET`** | Yes | - | Secret for signing admin tokens. |
| **`RUST_LOG`** | No | `info` | Trait-based logging level. |
| **`STELLAR_RPC_URL`** | Yes | - | Horizon provider (e.g., `https://horizon-testnet.stellar.org`). |
| **`STELLAR_NETWORK`** | No | `testnet` | Stellar network mode (`testnet`, `public`). |

---

## Docker Deployment (Manual)

### 1. Build the Production Image
```bash
docker build -t tipjar-backend .
```

### 2. Run with Environment File
```bash
docker run -d \
  --name tipjar-api \
  --env-file .env.production \
  -p 8000:8000 \
  tipjar-backend
```

---

## Docker Compose (Recommended)
Use the included `compose.yml` for unified management:

```bash
docker-compose up -d
```

---

## Production Security Checklist

- [ ] **SSL/TLS**: Ensure the API is behind a reverse proxy (e.g., Nginx, Caddy, Cloudflare) with HTTPS enabled.
- [ ] **Hardened Secrets**: Ensure `JWT_SECRET` and `DATABASE_URL` (with credentials) are not exposed.
- [ ] **Rate Limiting**: Monitor `429` errors to ensure quotas are balanced between security and usability.
- [ ] **Database Backups**: Schedule daily automated backups and verify restoration procedures.
- [ ] **Firewall**: Restrict database and Redis ports to the internal network only.
- [ ] **Monitoring**: Integrate with a monitoring system (e.g., Prometheus, Grafana, Datadog) to track 5xx errors and latency.

---

## CI/CD Pipeline
Continuous integration and deployment are handled through standard runners (e.g., GitHub Actions), which:
1. Run all tests.
2. Build the production Docker image.
3. Push to a private container registry.
4. Trigger a rollout on the orchestration platform (e.g., Kubernetes, CapRover, Render).
