![Helicone AI Gateway](https://marketing-assets-helicone.s3.us-west-2.amazonaws.com/github-w%3Alogo.png)

# Helicone AI Gateway

[![GitHub stars](https://img.shields.io/github/stars/Helicone/ai-gateway?style=for-the-badge)](https://github.com/helicone/ai-gateway/)
[![Downloads](https://img.shields.io/github/downloads/Helicone/ai-gateway/total?style=for-the-badge)](https://github.com/helicone/aia-gateway/releases)
[![Docker pulls](https://img.shields.io/docker/pulls/helicone/ai-gateway?style=for-the-badge)](https://hub.docker.com/r/helicone/ai-gateway)
[![License](https://img.shields.io/badge/license-APACHE-green?style=for-the-badge)](LICENSE)
[![Public Beta](https://img.shields.io/badge/status-Public%20Beta-orange?style=for-the-badge)](https://github.com/helicone/ai-gateway)

**The fastest, lightest, and easiest-to-integrate AI Gateway on the market.**

_Built by the team at [Helicone](https://helicone.ai), open-sourced for the community._

[🚀 Quick Start](https://docs.helicone.ai/ai-gateway/quickstart) • [📖 Docs](https://docs.helicone.ai/ai-gateway/introduction) • [💬 Discord](https://discord.gg/7aSCGCGUeu) • [🌐 Website](https://helicone.ai)

_Talk to our up-to-date documentation in DeepWiki:_ [![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/Helicone/ai-gateway)

---

### 🚆 1 API. 100+ models.

**Open-source, lightweight, and built on Rust.**

Handle hundreds of models and millions of LLM requests with minimal latency and maximum reliability.

The NGINX of LLMs.

---

## 👩🏻‍💻 Set up in seconds

1. Set up your `.env` file with your `PROVIDER_API_KEY`s

```bash
OPENAI_API_KEY=your_openai_key
ANTHROPIC_API_KEY=your_anthropic_key
```

2. Run locally in your terminal

```bash
npx @helicone/ai-gateway@latest
```

3. Make your requests using any OpenAI SDK:

```python
from openai import OpenAI

client = OpenAI(
    base_url="http://localhost:8080/ai",
    api_key="placeholder-api-key" # Gateway handles API keys
)

# Route to any LLM provider through the same interface, we handle the rest.
response = client.chat.completions.create(
    model="anthropic/claude-3-5-sonnet",  # Or other 100+ models..
    messages=[{"role": "user", "content": "Hello from Helicone AI Gateway!"}]
)
```

**That's it.** No new SDKs to learn, no integrations to maintain. Fully-featured and open-sourced.

_-- For advanced config, check out our [configuration guide](https://docs.helicone.ai/ai-gateway/config) and the [providers we support](https://github.com/Helicone/ai-gateway/blob/main/ai-gateway/config/embedded/providers.yaml)._

---

## Why Helicone AI Gateway?

#### 🌐 **Unified interface**

Request **any LLM provider** using familiar OpenAI syntax. Stop rewriting integrations—use one API for OpenAI, Anthropic, Google, AWS Bedrock, and [20+ more providers](https://docs.helicone.ai/ai-gateway/providers).

#### ⚡ **Smart provider selection**

**Load balance** to always hit the fastest, cheapest, or most reliable option. Built-in strategies include latency-based P2C + PeakEWMA, weighted distribution, and cost optimization. Always aware of provider uptime and rate limits.

#### 💰 **Control your spending**

**Rate limit** to prevent runaway costs and usage abuse. Set limits per user, team, or globally with support for request counts, token usage, and dollar amounts.

#### 🚀 **Improve performance**

**Cache responses** to reduce costs and latency by up to 95%. Supports Redis and S3 backends with intelligent cache invalidation.

#### 📊 **Simplified tracing**

Monitor performance and debug issues with built-in Helicone integration, plus OpenTelemetry support for **logs, metrics, and traces**.

#### ☁️ **One-click deployment**

Deploy in seconds to your own infrastructure by using our **Docker** or **binary** download following our [deployment guides](https://docs.helicone.ai/ai-gateway/deployment/overview).

https://github.com/user-attachments/assets/ed3a9bbe-1c4a-47c8-98ec-2bb4ff16be1f

---

## ⚡ Scalable for production

| Metric           | Helicone AI Gateway | Typical Setup |
| ---------------- | ------------------- | ------------- |
| **P95 Latency**  | <10ms               | ~60-100ms     |
| **Memory Usage** | ~64MB               | ~512MB        |
| **Requests/sec** | ~2,000              | ~500          |
| **Binary Size**  | ~15MB               | ~200MB        |
| **Cold Start**   | ~100ms              | ~2s           |

_Note: These are preliminary performance metrics. See [benchmarks/README.md](benchmarks/README.md) for detailed benchmarking methodology and results._

---

## 🎥 Demo

https://github.com/user-attachments/assets/dd6b6df1-0f5c-43d4-93b6-3cc751efb5e1

---

## 🏗️ How it works

```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   Your App      │───▶│ Helicone AI     │───▶│  LLM Providers  │
│                 │    │ Gateway         │    │                 │
│ OpenAI SDK      │    │                 │    │ • OpenAI        │
│ (any language)  │    │ • Load Balance  │    │ • Anthropic     │
│                 │    │ • Rate Limit    │    │ • AWS Bedrock   │
│                 │    │ • Cache         │    │ • Google Vertex │
│                 │    │ • Trace         │    │ • 20+ more      │
│                 │    │ • Fallbacks     │    │                 │
└─────────────────┘    └─────────────────┘    └─────────────────┘
                               │
                               ▼
                      ┌─────────────────┐
                      │ Helicone        │
                      │ Observability   │
                      │                 │
                      │ • Dashboard     │
                      │ • Observability │
                      │ • Monitoring    │
                      │ • Debugging     │
                      └─────────────────┘
```

---

## ⚙️ Custom configuration

### 1. Set up your environment variables

Include your `PROVIDER_API_KEY`s in your `.env` file.

```bash
OPENAI_API_KEY=sk-...
ANTHROPIC_API_KEY=sk-ant-...
HELICONE_API_KEY=sk-...
```

### 2. Customize your config file

_Note: This is a sample `config.yaml` file. Please refer to our [configuration guide](https://docs.helicone.ai/ai-gateway/config) for the full list of options, examples, and defaults._
_See our [full provider list here.](https://github.com/Helicone/ai-gateway/blob/main/ai-gateway/config/embedded/providers.yaml)_

```yaml
helicone: # Include your HELICONE_API_KEY in your .env file
  features: all

cache-store:
  in-memory: {}

global: # Global settings for all routers
  cache:
    directive: "max-age=3600, max-stale=1800"

routers:
  your-router-name: # Single router configuration
    load-balance:
      chat:
        strategy: latency
        targets:
          - openai
          - anthropic
    rate-limit:
      per-api-key:
        capacity: 1000
        refill-frequency: 1m # 1000 requests per minute
```

### 3. Run with your custom configuration

```bash
npx @helicone/ai-gateway@latest --config config.yaml
```

### 4. Make your requests

```python
from openai import OpenAI

client = OpenAI(
    base_url="http://localhost:8080/router/your-router-name",
    api_key="placeholder-api-key" # Gateway handles API keys
)

# Route to any LLM provider through the same interface, we handle the rest.
response = client.chat.completions.create(
    model="anthropic/claude-3-5-sonnet",  # Or other 100+ models..
    messages=[{"role": "user", "content": "Hello from Helicone AI Gateway!"}]
)
```

---

## 📚 Migration guide

### From OpenAI (Python)

```diff
from openai import OpenAI

client = OpenAI(
-   api_key=os.getenv("OPENAI_API_KEY")
+   api_key="placeholder-api-key" # Gateway handles API keys
+   base_url="http://localhost:8080/router/your-router-name"
)

# No other changes needed!
response = client.chat.completions.create(
    model="openai/gpt-4o",
    messages=[{"role": "user", "content": "Hello!"}]
)
```

### From OpenAI (TypeScript)

```diff typescript
import { OpenAI } from "openai";

const client = new OpenAI({
-   apiKey: os.getenv("OPENAI_API_KEY")
+   apiKey: "placeholder-api-key", // Gateway handles API keys
+   baseURL: "http://localhost:8080/router/your-router-name",
});

const response = await client.chat.completions.create({
  model: "openai/gpt-4o",
  messages: [{ role: "user", content: "Hello from Helicone AI Gateway!" }],
});
```


---

## 📚 Resources

### Documentation

- 📖 **[Full Documentation](https://docs.helicone.ai/ai-gateway/introduction)** - Complete guides and API reference
- 🚀 **[Quickstart Guide](https://docs.helicone.ai/ai-gateway/quickstart)** - Get up and running in 1 minute
- 🔬 **[Advanced Configurations](https://docs.helicone.ai/ai-gateway/config)** - Configuration reference & examples
- 🧠 **[Ask DeepWiki](https://deepwiki.com/Helicone/ai-gateway)** - Talk to the repository. Ask any question.

### Community

- 💬 **[Discord Server](https://discord.gg/7aSCGCGUeu)** - Our community of passionate AI engineers
- 🐙 **[GitHub Discussions](https://github.com/helicone/ai-gateway/discussions)** - Q&A and feature requests
- 🐦 **[Twitter](https://twitter.com/helicone_ai)** - Latest updates and announcements
- 📧 **[Newsletter](https://helicone.ai/email-signup)** - Tips and tricks to deploying AI applications

### Support

- 🎫 **[Report bugs](https://github.com/helicone/ai-gateway/issues)**: Github issues
- 💼 **[Enterprise Support](https://cal.com/team/helicone/helicone-discovery)**: Book a discovery call with our team

---

## 📄 License

The Helicone AI Gateway is licensed under the [Apache License](LICENSE) - see the file for details.

---

**Made with ❤️ by [Helicone](https://helicone.ai).**

[Website](https://helicone.ai) • [Docs](https://docs.helicone.ai/ai-gateway/introduction) • [Twitter](https://twitter.com/helicone_ai) • [Discord](https://discord.gg/7aSCGCGUeu)
