# General Configuration
variable "environment" {
  description = "Environment name (e.g., dev, staging, prod)"
  type        = string
}

variable "tags" {
  description = "Common tags to apply to all resources"
  type        = map(string)
  default     = {}
}

# Multi-region ALB Configuration
variable "alb_origins" {
  description = "Map of region to ALB DNS names"
  type        = map(string)
  # Example:
  # {
  #   "us-west-2" = "alb-prod-123.us-west-2.elb.amazonaws.com"
  #   "us-east-1" = "alb-prod-456.us-east-1.elb.amazonaws.com"
  # }
}

# DNS Configuration
variable "route53_zone_name" {
  description = "The Route53 hosted zone name to create and delegate from Cloudflare (e.g., 'ai-gateway.helicone.ai')"
  type        = string
}

variable "origin_subdomain" {
  description = "The subdomain prefix for origin records within the zone (e.g., 'origin' for origin.ai-gateway.helicone.ai)"
  type        = string
  default     = "origin"
}

# CloudFront Configuration
variable "acm_certificate_arn" {
  description = "ARN of the ACM certificate for CloudFront (must be in us-east-1)"
  type        = string
}

variable "cloudfront_price_class" {
  description = "CloudFront distribution price class"
  type        = string
  default     = "PriceClass_200" # US, Canada, Europe, Asia, Middle East, Africa
}

variable "forwarded_headers" {
  description = "Headers to forward to the origin"
  type        = list(string)
  default = [
    "Authorization",
    "CloudFront-Forwarded-Proto",
    "CloudFront-Viewer-Country",
    "Host",
    "Accept",
    "Accept-Encoding",
    "Accept-Language",
    "Content-Type",
    "Origin",
    "Referer",
    "User-Agent",
    "X-Forwarded-For",
    "X-Forwarded-Host",
    "X-Forwarded-Port",
    "X-Forwarded-Proto"
  ]
}

variable "origin_keepalive_timeout" {
  description = "The amount of time (in seconds) that CloudFront maintains an idle connection with your origin"
  type        = number
  default     = 10
}

variable "origin_read_timeout" {
  description = "The amount of time (in seconds) that CloudFront waits for a response from your origin"
  type        = number
  default     = 30
}

variable "geo_restriction_type" {
  description = "The method to restrict distribution of your content by geographic location"
  type        = string
  default     = "none"
}

variable "geo_restriction_locations" {
  description = "List of country codes for geo restriction"
  type        = list(string)
  default     = []
}

variable "web_acl_id" {
  description = "AWS WAF Web ACL ID to associate with the distribution"
  type        = string
  default     = null
}