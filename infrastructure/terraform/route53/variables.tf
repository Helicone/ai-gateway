variable "region" {
  description = "AWS region for resources"
  type        = string
  default     = "us-west-2"
}

variable "tags" {
  description = "Common tags to apply to all resources"
  type        = map(string)
  default = {
    Environment = "production"
    Project     = "helicone"
    ManagedBy   = "terraform"
  }
}

# Domain Configuration
variable "heliconetest_domain" {
  description = "The primary domain for Route53 hosted zone"
  type        = string
  default     = "heliconetest.com"
}

# Remote state configuration
variable "terraform_organization" {
  description = "Terraform Cloud organization name"
  type        = string
  default     = "helicone"
}

variable "eks_terraform_workspace" {
  description = "Name of the EKS Terraform workspace"
  type        = string
  default     = "helicone"
}

variable "acm_terraform_workspace" {
  description = "Name of the ACM Terraform workspace"
  type        = string
  default     = "helicone-acm"
}

# Subdomain configuration
variable "enable_grafana_subdomain" {
  description = "Whether to create A record for grafana subdomain"
  type        = bool
  default     = true
}

variable "enable_argocd_subdomain" {
  description = "Whether to create A record for argocd subdomain"
  type        = bool
  default     = true
}

variable "additional_subdomains" {
  description = "List of additional subdomains to create A records for"
  type        = list(string)
  default     = []
} 