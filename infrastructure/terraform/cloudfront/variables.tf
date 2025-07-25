variable "environment" {
  description = "Environment name (e.g., dev, staging, prod)"
  type        = string
}

variable "origins" {
  description = "Map of ALB domain names by region"
  type        = map(string)
}

variable "domain_names" {
  description = "List of domain names (aliases) for the CloudFront distribution"
  type        = list(string)
  default     = []
}

variable "acm_certificate_arn" {
  description = "ARN of the ACM certificate for the alias domain CloudFront"
  type        = string
}

variable "price_class" {
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
  default     = 900
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

variable "default_cache_behavior" {
  description = "List of cache behaviors for specific path patterns"
  type = list(object({
    path_pattern     = string
    allowed_methods  = list(string)
    cached_methods   = list(string)
    query_string     = bool
    headers          = list(string)
    cookies_forward  = string
    min_ttl          = number
    default_ttl      = number
    max_ttl          = number
  }))
  default = []
}

variable "enable_origin_failover" {
  description = "Enable origin failover using origin groups"
  type        = bool
  default     = true
}
