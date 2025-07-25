# CloudFront with Route53 Multi-Region Module

This Terraform module creates a multi-region setup with CloudFront for DDoS protection and Route53 latency-based routing for optimal performance. It supports subdomain delegation from Cloudflare to Route53.

## Architecture

```
User → ai-gateway.helicone.ai (Route53) → CloudFront → origin.ai-gateway.helicone.ai (Route53 latency) → Regional ALBs
```

### DNS Structure

This module creates a single Route53 hosted zone (`ai-gateway.helicone.ai`) that contains:
- **API endpoint**: `ai-gateway.helicone.ai` → CloudFront
- **Origin endpoint**: `origin.ai-gateway.helicone.ai` → Regional ALBs with latency routing

All DNS records are managed within the same delegated zone, simplifying the architecture.

## Features

- **DDoS Protection**: CloudFront provides automatic DDoS protection
    - we can also setup things to blot bots/spam request
- **Low Latency**: Route53 automatically routes to the nearest healthy ALB using latency-based routing
- **High Availability**: Health checks ensure traffic only goes to healthy origins
- **SSL/TLS**: Full encryption from client to origin
- **IPv6 Support**: Fully IPv6 enabled
- **Multi-Region**: Always uses latency-based routing for optimal performance

## Prerequisites

1. **ALBs deployed in multiple regions** (using the ECS module)
2. **Main domain in Cloudflare** (e.g., helicone.ai)
3. **ACM certificate** for CloudFront (must include the subdomain)

## Usage

```hcl
module "cloudfront_route53" {
  source = "./cloudfront-route53"

  environment = "prod"
  
  # DNS zone configuration
  route53_zone_name = "ai-gateway.helicone.ai"  # Will be created and delegated from Cloudflare
  origin_subdomain  = "origin"                  # Creates origin.ai-gateway.helicone.ai

  # ALB endpoints from your regional deployments
  alb_origins = {
    "us-west-2" = module.ecs_us_west_2.load_balancer_dns_name
    "us-east-1" = module.ecs_us_east_1.load_balancer_dns_name
    "eu-west-1" = module.ecs_eu_west_1.load_balancer_dns_name
  }

  # ACM certificate (must be in us-east-1)
  acm_certificate_arn = aws_acm_certificate_validation.cert.certificate_arn

  tags = {
    Environment = "production"
    Project     = "helicone"
  }
}
```


## How It Works

1. **Route53 A/AAAA records** for `ai-gateway.helicone.ai` point to CloudFront
2. **CloudFront** forwards requests to `origin.ai-gateway.helicone.ai`
3. **Route53 latency-based routing** on the origin automatically selects the nearest ALB
4. **Health checks** ensure only healthy ALBs receive traffic

This module always uses latency-based routing to ensure optimal performance across regions.

## Multi-Region Setup

This module is designed for multi-region deployments:
- Provide multiple entries in `alb_origins` for all your regions
- Creates `gw-origin.helicone.ai` with latency-based routing
- Route53 automatically directs traffic to the nearest healthy ALB
- Ideal for global applications requiring low latency

## DNS Configuration: Subdomain Delegation

This module creates a Route53 hosted zone for your subdomain (e.g., `ai-gateway.helicone.ai`) while keeping your main domain (e.g., `helicone.ai`) in Cloudflare.

### Setup Steps

1. **Apply Terraform** to create the Route53 hosted zone:
   ```bash
   terraform apply
   ```

2. **Get the nameservers** for the new Route53 zone:
   ```bash
   terraform output cloudflare_ns_records
   ```

3. **Add NS records in Cloudflare** for your main domain:
   - Go to Cloudflare DNS for `helicone.ai`
   - Add 4 NS records (one for each nameserver):
     ```
     ai-gateway    NS    ns-123.awsdns-12.com
     ai-gateway    NS    ns-456.awsdns-34.net
     ai-gateway    NS    ns-789.awsdns-56.org
     ai-gateway    NS    ns-012.awsdns-78.co.uk
     ```

4. **Verify delegation** (may take a few minutes):
   ```bash
   dig ai-gateway.helicone.ai NS
   ```

### DNS Architecture

| Domain/Subdomain | DNS Provider | Purpose |
|-----------------|--------------|----------|
| helicone.ai | Cloudflare | Main domain (registration & DNS) |
| ai-gateway.helicone.ai | Route53 | API endpoint (delegated subdomain) |
| origin.ai-gateway.helicone.ai | Route53 | ALB origins with latency routing |

### Benefits

- Keep domain registration and main site with Cloudflare
- Use Route53 for AWS-integrated features (ALB aliases, latency routing)
- Single delegated zone for all API infrastructure
- Clean separation between main site and API
