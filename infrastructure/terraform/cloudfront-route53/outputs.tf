# CloudFront Outputs
output "cloudfront_distribution_id" {
  description = "ID of the CloudFront distribution"
  value       = aws_cloudfront_distribution.main.id
}

output "cloudfront_distribution_arn" {
  description = "ARN of the CloudFront distribution"
  value       = aws_cloudfront_distribution.main.arn
}

output "cloudfront_distribution_domain_name" {
  description = "Domain name of the CloudFront distribution"
  value       = aws_cloudfront_distribution.main.domain_name
}

output "cloudfront_distribution_hosted_zone_id" {
  description = "Hosted zone ID of the CloudFront distribution"
  value       = aws_cloudfront_distribution.main.hosted_zone_id
}

output "cloudfront_etag" {
  description = "Current version of the distribution's information"
  value       = aws_cloudfront_distribution.main.etag
}

output "cloudfront_status" {
  description = "Current status of the distribution"
  value       = aws_cloudfront_distribution.main.status
}

# Route53 Outputs
output "route53_zone_id" {
  description = "ID of the Route53 hosted zone"
  value       = local.zone_id
}

output "route53_zone_name" {
  description = "Name of the Route53 hosted zone"
  value       = var.route53_zone_name
}

output "route53_nameservers" {
  description = "Nameservers for the Route53 hosted zone (add these as NS records in Cloudflare)"
  value       = aws_route53_zone.subdomain.name_servers
}

output "cloudflare_ns_records" {
  description = "Instructions for creating NS records in Cloudflare"
  value = {
    instructions = "Add these NS records in Cloudflare for ${local.base_domain}:"
    records = [
      for ns in aws_route53_zone.subdomain.name_servers : {
        name  = local.subdomain
        type  = "NS"
        value = ns
      }
    ]
  }
}

# Domain Outputs
output "api_domain" {
  description = "The API domain pointing to CloudFront"
  value       = local.api_domain
}

output "api_url" {
  description = "Full HTTPS URL for the API"
  value       = "https://${local.api_domain}"
}

output "origin_domain" {
  description = "The origin domain for CloudFront with latency routing"
  value       = local.origin_domain
}

# Configuration Summary
output "multi_region_configuration" {
  description = "Summary of the multi-region configuration"
  value = {
    api_endpoint       = "https://${local.api_domain}"
    origin_domain      = local.origin_domain
    regions_configured = keys(var.alb_origins)
    cloudfront_aliases = [local.api_domain]
  }
}

# ALB Configuration
output "alb_endpoints" {
  description = "Map of regions to ALB endpoints"
  value       = var.alb_origins
}

output "delegation_status" {
  description = "Status of subdomain delegation setup"
  value = {
    zone_name       = var.route53_zone_name
    base_domain     = local.base_domain
    subdomain       = local.subdomain
    origin_domain   = local.origin_domain
    ns_record_count = length(aws_route53_zone.subdomain.name_servers)
  }
}


