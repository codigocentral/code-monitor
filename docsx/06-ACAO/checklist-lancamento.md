# Checklist de Lancamento

## Pre-Launch (2 Semanas Antes)

### Produto

```
[ ] TLS/SSL funcionando
[ ] Sistema de alertas básico (5 tipos)
[ ] Histórico SQLite (7 dias)
[ ] Health check endpoints
[ ] CI/CD pipeline completo
[ ] Docker Compose funcional
[ ] Binários para Linux e Windows
[ ] Testes automatizados (>70% coverage)
[ ] Bug críticos resolvidos (0)
```

### Documentacao

```
[ ] README.md profissional
[ ] Getting Started guide
[ ] Installation guide (Linux, Windows, Docker)
[ ] Configuration reference
[ ] Troubleshooting guide
[ ] API documentation (se aplicável)
[ ] CHANGELOG.md atualizado
[ ] CONTRIBUTING.md
```

### Website

```
[ ] Landing page completa
    [ ] Hero section com value prop
    [ ] Features section
    [ ] Pricing section (se aplicável)
    [ ] CTA (download/signup)
    [ ] Footer com links
[ ] Página de documentação
[ ] Página de pricing (se tier pago)
[ ] Blog com post de lançamento
[ ] Legal (Terms, Privacy)
```

### Marketing

```
[ ] Blog post de lançamento pronto
[ ] Thread do Twitter/X preparada (10 tweets)
[ ] Post do Reddit preparado
    [ ] r/selfhosted
    [ ] r/homelab
    [ ] r/devops
    [ ] r/rust
[ ] Assets visuais
    [ ] Screenshots
    [ ] GIFs/Videos
    [ ] Open Graph image
[ ] Demo video (2 min)
```

### Infraestrutura

```
[ ] Domínio configurado
[ ] SSL/TLS no website
[ ] CDN configurado (Cloudflare)
[ ] Analytics instalado
[ ] Error tracking (Sentry)
[ ] Uptime monitoring
[ ] Email configurado (transacional)
```

### Comunidade

```
[ ] Discord server criado
    [ ] Canais configurados
    [ ] Bots de moderação
    [ ] Welcome message
[ ] GitHub Issues templates
[ ] GitHub Discussions habilitado
[ ] Twitter/X conta ativa
```

---

## Launch Day

### Timeline

```
06:00 - Verificar tudo funcionando
        [ ] Website online
        [ ] Downloads funcionando
        [ ] Docs acessíveis

07:00 - Publicar blog post
        [ ] Blog post ao ar
        [ ] Links verificados
        [ ] SEO meta tags

08:00 - Twitter/X thread
        [ ] Thread publicada
        [ ] Pinned tweet

09:00 - Hacker News
        [ ] "Show HN" submetido
        [ ] Preparado para responder

10:00 - Reddit posts
        [ ] r/selfhosted
        [ ] r/homelab
        [ ] Responder comentários

11:00 - LinkedIn
        [ ] Post publicado
        [ ] Tag pessoas relevantes

12:00 - Product Hunt (se aplicável)
        [ ] Lançamento ativo
        [ ] Responder reviews

14:00 - Monitorar e responder
        [ ] HN comments
        [ ] Reddit comments
        [ ] Twitter mentions
        [ ] GitHub issues

18:00 - Email para lista (se tiver)
        [ ] Newsletter enviada

20:00 - Review do dia
        [ ] Métricas coletadas
        [ ] Issues críticos?
        [ ] Próximos passos
```

### Posts Templates

**Hacker News (Show HN):**
```
Show HN: Code Monitor – Lightweight multi-server monitoring in Rust

Hey HN! I built Code Monitor because I was frustrated with existing
monitoring tools:
- Glances uses 15% CPU on idle
- Netdata requires their cloud for basic features
- Datadog costs a fortune

Code Monitor is:
- Written in Rust (1.5% CPU, 50MB RAM)
- Terminal-native (works via SSH)
- 100% on-prem (your data stays yours)
- Single binary, zero dependencies

Current features:
- Multi-server monitoring
- Real-time metrics (CPU, memory, disk, network)
- Process and service monitoring
- Vim-style keyboard navigation

Still early, but looking for feedback!

GitHub: [link]
Demo: [gif/video]
```

**Reddit (r/selfhosted):**
```
[Project] Code Monitor - Lightweight server monitoring in Rust

I've been working on Code Monitor, a monitoring tool for those of us
who want visibility into our servers without the overhead.

**The problem:**
- Glances: Python, uses 15% CPU
- Netdata: Wants your data in their cloud
- Datadog: $$$$

**My solution:**
- Rust-based, uses 1.5% CPU
- TUI that works via SSH
- Multi-server from one dashboard
- 100% on-prem, zero telemetry

Currently supports:
- CPU, Memory, Disk, Network metrics
- Process and service monitoring
- Token-based authentication

Looking for beta testers and feedback!

[Screenshots/GIF]

GitHub: [link]
```

---

## Post-Launch (Semana 1)

### Diário

```
[ ] Responder TODOS os comentários
[ ] Monitorar GitHub issues
[ ] Hotfix para bugs críticos
[ ] Coletar feedback
[ ] Agradecer contributors
```

### Métricas para Coletar

```
[ ] Website
    [ ] Unique visitors
    [ ] Page views
    [ ] Bounce rate
    [ ] Time on site

[ ] Produto
    [ ] Downloads
    [ ] GitHub stars
    [ ] GitHub issues abertos
    [ ] GitHub PRs

[ ] Comunidade
    [ ] Discord members
    [ ] Twitter followers
    [ ] Newsletter signups

[ ] Conversão (se tier pago)
    [ ] Signups
    [ ] Trial starts
    [ ] Conversions
```

### Conteúdo Follow-up

```
[ ] Blog: "What we learned from launch"
[ ] Blog: "Top feedback and what's next"
[ ] Twitter: Milestone celebration
[ ] Email: Thank you to early adopters
```

---

## Post-Launch (Semana 2-4)

### Produto

```
[ ] Implementar top 5 feature requests
[ ] Bug fixes da comunidade
[ ] Performance improvements
[ ] Docs improvements based on questions
```

### Conteudo

```
[ ] Tutorial: "Getting started with Code Monitor"
[ ] Tutorial: "Setting up alerts"
[ ] Comparison: "Code Monitor vs Glances"
[ ] Case study (se tiver early adopter)
```

### Comunidade

```
[ ] Weekly office hours (Discord)
[ ] First AMA
[ ] Contributor recognition
[ ] Feature voting poll
```

---

## Launch Metrics Targets

### Primeiro Dia

| Métrica | Minimum | Good | Great |
|---------|---------|------|-------|
| HN Points | 50 | 150 | 300+ |
| Website visits | 1,000 | 5,000 | 10,000+ |
| Downloads | 100 | 500 | 1,000+ |
| GitHub stars | 50 | 200 | 500+ |
| Discord joins | 20 | 50 | 100+ |

### Primeira Semana

| Métrica | Minimum | Good | Great |
|---------|---------|------|-------|
| Total downloads | 500 | 2,000 | 5,000+ |
| GitHub stars | 200 | 500 | 1,000+ |
| Issues opened | 10 | 30 | 50+ |
| PRs received | 1 | 5 | 10+ |
| Discord members | 50 | 150 | 300+ |

### Primeiro Mês

| Métrica | Minimum | Good | Great |
|---------|---------|------|-------|
| Active installs | 200 | 1,000 | 3,000+ |
| GitHub stars | 500 | 1,000 | 2,000+ |
| Contributors | 5 | 15 | 30+ |
| Discord members | 100 | 300 | 500+ |
| NPS | 30 | 45 | 60+ |

---

## Plano de Contingência

### Se o launch "flopar"

```
1. Não desanimar (normal no primeiro launch)
2. Analisar:
   - Qual canal performou pior?
   - O que o feedback disse?
   - Onde foi o drop-off?
3. Ajustar:
   - Messaging
   - Targeting
   - Produto (se necessário)
4. Tentar novamente:
   - Novo ângulo
   - Novo canal
   - Nova feature como hook
```

### Se houver problemas técnicos

```
1. Comunicar imediatamente
   - Twitter: "Aware of issues, working on fix"
   - Discord: Status channel
2. Priorizar:
   - Critical (blocking usage) → Fix now
   - High (degraded experience) → Fix today
   - Medium (annoying) → Fix this week
3. Post-mortem:
   - O que aconteceu
   - Por que aconteceu
   - O que fizemos
   - Como preveniremos
```

### Se houver feedback negativo

```
1. Responder profissionalmente
2. Agradecer pelo feedback
3. Explicar contexto (se necessário)
4. Comprometer com melhoria (se válido)
5. NÃO entrar em flame war
```

---

## Próximos Passos

- [Métricas de Sucesso](./metricas-sucesso.md) - KPIs detalhados
- [Quick Wins](./quick-wins.md) - O que fazer primeiro
