terraform {
  required_version = ">= 1.0"

  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.0"
    }
  }
}

# Create Route53 hosted zone for subdomain delegation
resource "aws_route53_zone" "subdomain" {
  name = var.route53_zone_name

  tags = merge(
    var.tags,
    {
      Name        = "ai-gateway-zone-${var.environment}"
      Environment = var.environment
    }
  )
}

# Data source to look up ALB hosted zone IDs dynamically
data "aws_elb_hosted_zone_id" "alb" {
  for_each = var.alb_origins
  region   = each.key
}

locals {
  # Extract base domain and subdomain parts for Cloudflare NS records
  # e.g., "ai-gateway.helicone.ai" -> base_domain = "helicone.ai", subdomain = "ai-gateway"
  domain_parts = split(".", var.route53_zone_name)
  subdomain    = local.domain_parts[0]
  base_domain  = join(".", slice(local.domain_parts, 1, length(local.domain_parts)))
  
  # Zone ID for all Route53 records
  zone_id = aws_route53_zone.subdomain.zone_id
  
  # Origin configuration for latency routing (within the same zone)
  origin_domain = "${var.origin_subdomain}.${var.route53_zone_name}"
  origin_id     = "gw-latency-origin"
  
  # API domain for CloudFront (same as the zone name)
  api_domain = var.route53_zone_name
}

# CloudFront distribution
resource "aws_cloudfront_distribution" "main" {
  enabled             = true
  is_ipv6_enabled     = true
  comment             = "CloudFront for AI Gateway - ${var.environment}"
  default_root_object = ""
  price_class         = var.cloudfront_price_class
  aliases             = [local.api_domain]

  # Single origin configuration
  origin {
    domain_name = local.origin_domain
    origin_id   = local.origin_id

    custom_origin_config {
      https_port             = 443
      http_port              = 80
      origin_protocol_policy = "https-only"
      origin_ssl_protocols   = ["TLSv1.2"]
      
      # Adjust timeouts for your application needs
      origin_keepalive_timeout = var.origin_keepalive_timeout
      origin_read_timeout      = var.origin_read_timeout
    }
  }

  default_cache_behavior {
    target_origin_id       = local.origin_id
    viewer_protocol_policy = "redirect-to-https"
    allowed_methods        = ["DELETE", "GET", "HEAD", "OPTIONS", "PATCH", "POST", "PUT"]
    cached_methods         = ["GET", "HEAD"]
    compress               = true

    forwarded_values {
      query_string = true
      headers      = var.forwarded_headers

      cookies {
        forward = "all"
      }
    }

    min_ttl     = 0
    default_ttl = 0
    max_ttl     = 0
  }

  restrictions {
    geo_restriction {
      restriction_type = var.geo_restriction_type
      locations        = var.geo_restriction_locations
    }
  }

  viewer_certificate {
    acm_certificate_arn            = var.acm_certificate_arn
    ssl_support_method             = "sni-only"
    minimum_protocol_version       = "TLSv1.2_2021"
    cloudfront_default_certificate = false
  }

  web_acl_id = var.web_acl_id

  tags = merge(
    var.tags,
    {
      Name        = "ai-gateway-cloudfront-${var.environment}"
      Environment = var.environment
    }
  )
}

# Latency-based routing for origin subdomain pointing to regional ALBs
resource "aws_route53_record" "origin_albs" {
  for_each = var.alb_origins

  zone_id = local.zone_id
  name    = local.origin_domain
  type    = "A"

  alias {
    name                   = each.value
    zone_id                = data.aws_elb_hosted_zone_id.alb[each.key].id
    evaluate_target_health = true
  }

  set_identifier = each.key  # "us-west-2", "eu-west-1", etc.

  latency_routing_policy {
    region = each.key
  }
}

# A record for api subdomain pointing to CloudFront distribution
resource "aws_route53_record" "api_cloudfront" {
  zone_id = local.zone_id
  name    = local.api_domain
  type    = "A"

  alias {
    name                   = aws_cloudfront_distribution.main.domain_name
    zone_id                = aws_cloudfront_distribution.main.hosted_zone_id
    evaluate_target_health = false
  }
}

# AAAA record for IPv6 support
resource "aws_route53_record" "api_cloudfront_ipv6" {
  zone_id = local.zone_id
  name    = local.api_domain
  type    = "AAAA"

  alias {
    name                   = aws_cloudfront_distribution.main.domain_name
    zone_id                = aws_cloudfront_distribution.main.hosted_zone_id
    evaluate_target_health = false
  }
}