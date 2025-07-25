locals {
  # Primary origin for default behavior
  primary_origin_key = keys(var.origins)[0]
}

resource "aws_cloudfront_distribution" "alb_distribution" {
  is_ipv6_enabled     = true
  comment             = "CloudFront distribution for AI Gateway - ${var.environment}"
  default_root_object = ""
  price_class         = var.price_class
  aliases             = var.domain_names

  # Create an origin for each region
  dynamic "origin" {
    for_each = var.origins
    content {
      domain_name = origin.value
      origin_id   = "ALB-${var.environment}-${origin.key}"

      custom_origin_config {
        https_port             = 443
        origin_protocol_policy = "https-only"
        origin_ssl_protocols   = ["TLSv1.2"]
        
        # Adjust timeouts for your application needs
        origin_keepalive_timeout = var.origin_keepalive_timeout
        origin_read_timeout      = var.origin_read_timeout
      }
    }
  }

  # Create origin groups for failover if enabled and multiple origins exist
  dynamic "origin_group" {
    for_each = var.enable_origin_failover && length(var.origins) > 1 ? [1] : []
    content {
      origin_id = "ALB-${var.environment}-origin-group"

      failover_criteria {
        status_codes = [500, 502, 503, 504, 522]
      }

      member {
        origin_id = "ALB-${var.environment}-${local.primary_origin_key}"
      }

      # Add other origins as failover members
      dynamic "member" {
        for_each = { for k, v in var.origins : k => v if k != local.primary_origin_key }
        content {
          origin_id = "ALB-${var.environment}-${member.key}"
        }
      }
    }
  }

  default_cache_behavior = var.default_cache_behavior

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

  tags = {
    Name        = "ai-gateway-cloudfront-${var.environment}"
    Environment = var.environment
  }
}