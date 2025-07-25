output "cloudfront_distribution_id" {
  description = "ID of the CloudFront distribution"
  value       = aws_cloudfront_distribution.alb_distribution.id
}

output "cloudfront_distribution_arn" {
  description = "ARN of the CloudFront distribution"
  value       = aws_cloudfront_distribution.alb_distribution.arn
}

output "cloudfront_domain_name" {
  description = "Domain name of the CloudFront distribution"
  value       = aws_cloudfront_distribution.alb_distribution.domain_name
}

output "cloudfront_hosted_zone_id" {
  description = "CloudFront Route 53 zone ID for alias records"
  value       = aws_cloudfront_distribution.alb_distribution.hosted_zone_id
}

output "cloudfront_etag" {
  description = "Current version of the distribution's information"
  value       = aws_cloudfront_distribution.alb_distribution.etag
}

output "cloudfront_status" {
  description = "Current status of the distribution"
  value       = aws_cloudfront_distribution.alb_distribution.status
}

output "custom_domain_urls" {
  description = "List of custom domain URLs for the CloudFront distribution"
  value       = [for domain in var.domain_names : "https://${domain}"]
}

output "cloudfront_url" {
  description = "CloudFront distribution URL"
  value       = "https://${aws_cloudfront_distribution.alb_distribution.domain_name}"
}

output "origin_configurations" {
  description = "Map of origin configurations used in the distribution"
  value       = var.origins
}

output "origin_group_enabled" {
  description = "Whether origin failover is enabled"
  value       = var.enable_origin_failover && length(var.origins) > 1
}