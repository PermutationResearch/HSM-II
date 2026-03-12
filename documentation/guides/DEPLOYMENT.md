# HSM-II Production Deployment Guide

Complete deployment instructions for HSM-II in production environments.

## Quick Start

```bash
# 1. Clone and setup
git clone <repository>
cd hyper-stigmergic-morphogenesisII

# 2. Configure environment
cp .env.example .env
# Edit .env with your API keys

# 3. Deploy with Docker Compose
docker-compose up -d

# 4. Check health
curl http://localhost:8080/health
```

## Prerequisites

- Docker 20.10+ and Docker Compose 2.0+
- At least one LLM API key (OpenAI, Anthropic, or Ollama)
- 4GB+ RAM, 2+ CPU cores

## Configuration

### Required: LLM Provider

At least one LLM provider must be configured:

**OpenAI:**
```bash
OPENAI_API_KEY=sk-...
```

**Anthropic:**
```bash
ANTHROPIC_API_KEY=sk-ant-...
```

**Ollama (local):**
```bash
# No API key needed, just ensure Ollama is running
OLLAMA_URL=http://localhost:11434
```

### Optional: Browser Automation

For web scraping and browser automation:
```bash
BROWSERBASE_API_KEY=...
BROWSERBASE_PROJECT_ID=...
```

### Optional: Cloudflare Web Search

```bash
CF_ACCOUNT_ID=...
CF_API_TOKEN=...
```

## Deployment Options

### 1. Docker Compose (Recommended)

Full stack with monitoring:

```bash
docker-compose up -d
```

Services:
- `hsm-ii`: Main application (port 8080)
- `ollama`: Local LLM inference (port 11434)
- `prometheus`: Metrics collection (port 9090)
- `grafana`: Dashboards (port 3000)

### 2. Single Container

Minimal deployment:

```bash
docker build -t hsm-ii .
docker run -d \
  -p 8080:8080 \
  -e OPENAI_API_KEY=... \
  -v $(pwd)/data:/app/data \
  hsm-ii
```

### 3. Kubernetes

See `k8s/` directory for manifests (coming soon).

### 4. Bare Metal

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Build release binary
cargo build --release

# Run
./target/release/hyper-stigmergy
```

## Health Checks

### Liveness
```bash
curl http://localhost:8080/live
# Returns: 200 OK if process is running
```

### Readiness
```bash
curl http://localhost:8080/ready
# Returns: 200 OK if all dependencies healthy
# Returns: 503 if degraded
```

### Full Health
```bash
curl http://localhost:8080/health
```

Response:
```json
{
  "status": "healthy",
  "version": "0.1.0",
  "uptime_seconds": 3600,
  "checks": [
    {
      "name": "llm",
      "status": "healthy",
      "latency_ms": 150
    },
    {
      "name": "database",
      "status": "healthy",
      "latency_ms": 5
    }
  ]
}
```

## Monitoring

### Metrics Endpoint

```bash
curl http://localhost:9000/metrics
```

Key metrics:
- `hsm_http_requests_total`: Total HTTP requests
- `hsm_llm_requests_total`: LLM API calls
- `hsm_llm_latency_milliseconds`: LLM response times
- `hsm_tool_executions_total`: Tool usage
- `hsm_failures_total`: Failed operations
- `hsm_promises_kept_total` / `hsm_promises_broken_total`: Promise tracking

### Prometheus

Access at http://localhost:9090

### Grafana

Access at http://localhost:3000 (default login: admin/admin)

Pre-configured dashboards:
- HSM-II Overview
- LLM Performance
- Tool Usage
- Error Rates

## Logging

Structured JSON logging to stdout:

```bash
# View logs
docker logs hsm-ii

# Follow logs
docker logs -f hsm-ii

# Filter by level
RUST_LOG=warn docker-compose up
```

Log levels:
- `error`: Failures requiring attention
- `warn`: Degraded performance
- `info`: Normal operations
- `debug`: Detailed debugging
- `trace`: Verbose tracing

## Scaling

### Horizontal Scaling

```bash
# Scale to 3 instances
docker-compose up -d --scale hsm-ii=3
```

Use a load balancer (nginx, traefik) in front.

### Resource Limits

Default limits in docker-compose.yml:
- CPU: 4 cores
- Memory: 8GB

Adjust based on workload:
```yaml
deploy:
  resources:
    limits:
      cpus: '8'
      memory: 16G
```

## Backup and Recovery

### Data Backup

```bash
# Backup data directory
tar -czf backup-$(date +%Y%m%d).tar.gz ./data

# Automated daily backup
0 2 * * * tar -czf /backups/hsm-$(date +\%Y\%m\%d).tar.gz /app/data
```

### Database Migration

For SQLite (default):
- Copy `.data/hsm.db` to migrate

For MySQL:
```bash
mysqldump -u user -p hsm > backup.sql
mysql -u user -p hsm < backup.sql
```

## Troubleshooting

### Container Won't Start

```bash
# Check logs
docker logs hsm-ii

# Verify environment
docker exec hsm-ii env | grep -E "(OPENAI|ANTHROPIC)"

# Test LLM connectivity
docker exec hsm-ii curl -s http://ollama:11434/api/tags
```

### High Memory Usage

```bash
# Check memory usage
docker stats hsm-ii

# Restart with more memory
docker-compose up -d --no-deps --force-recreate hsm-ii
```

### LLM Failures

```bash
# Check provider health
curl http://localhost:8080/health | jq '.checks[] | select(.name == "llm")'

# Test direct API call
curl -H "Authorization: Bearer $OPENAI_API_KEY" \
  https://api.openai.com/v1/models
```

## Security

### API Keys

- Never commit `.env` to git
- Use Docker secrets or external vault in production
- Rotate keys regularly

### Network

```bash
# Only expose necessary ports
# 8080: API (expose)
# 9000: Metrics (internal only)
# 11434: Ollama (internal only)
```

### Non-root User

Container runs as `hsm` user (UID 999) for security.

## Updating

```bash
# Pull latest
git pull

# Rebuild
docker-compose build --no-cache

# Restart
docker-compose up -d

# Verify
curl http://localhost:8080/health
```

## Support

- Issues: GitHub Issues
- Documentation: docs.hsm-ii.io
- Community: Discord

---

**Production Checklist:**

- [ ] LLM API keys configured
- [ ] `.env` file created and secured
- [ ] Health checks passing
- [ ] Monitoring enabled (Prometheus/Grafana)
- [ ] Backups configured
- [ ] Alert webhooks set (optional)
- [ ] Resource limits appropriate for workload
- [ ] Security: Non-root user, no sensitive data in images
