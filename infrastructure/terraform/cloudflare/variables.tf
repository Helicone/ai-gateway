variable "cloudflare_api_token" {
  description = "Cloudflare API token for DNS management"
  type        = string
  sensitive   = true
}

variable "cloudflare_subdomain" {
  description = "Subdomain for the application"
  type        = string
  default     = "filevine"
}

# Cloudflare Configuration for helicone.ai
variable "enable_helicone_ai_domain" {
  description = "Whether to create DNS records for helicone.ai domain"
  type        = bool
  default     = true
}

variable "cloudflare_helicone_ai_zone_name" {
  description = "Cloudflare zone name for helicone.ai domain"
  type        = string
  default     = "helicone.ai"
}

variable "cloudflare_helicone_ai_subdomain" {
  description = "Subdomain for the helicone.ai application"
  type        = string
  default     = "filevine"
}

variable "create_helicone_ai_root_domain_record" {
  description = "Whether to create a DNS record for the root domain (helicone.ai)"
  type        = bool
  default     = false
}

variable "cloudflare_helicone_test_zone_id" {
  description = "Cloudflare zone ID for helicone-test.com domain (required if enable_helicone_test_domain is true)"
  type        = string
  default     = ""
}

# EKS configuration (for remote state path)
variable "eks_terraform_state_path" {
  description = "Path to the EKS Terraform state file"
  type        = string
  default     = "../eks/terraform.tfstate"
}

variable "remote_state_backend" {
  description = "Backend type for remote state (local, s3, remote, etc.)"
  type        = string
  default     = "local"
}

variable "remote_state_config" {
  description = "Configuration for remote state backend"
  type        = map(string)
  default     = {}
} 