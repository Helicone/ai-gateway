terraform {
  required_version = ">= 1.0"
  
  cloud { 
    organization = "helicone" 

    workspaces { 
      name = "helicone-cloudflare" 
    } 
  }

  required_providers {
    cloudflare = {
      source  = "cloudflare/cloudflare"
      version = "~> 5.0"
    }
  }
}

# Cloudflare Provider Configuration
provider "cloudflare" {
  api_token = var.cloudflare_api_token
}

# Data source to get EKS state outputs from Terraform Cloud
data "terraform_remote_state" "eks" {
  backend = "remote"
  
  config = {
    organization = "helicone"
    workspaces = {
      name = "helicone"  # This matches the EKS workspace name
    }
  }
}

# Data source to get ACM state outputs from Terraform Cloud
data "terraform_remote_state" "acm" {
  backend = "remote"
  
  config = {
    organization = "helicone"
    workspaces = {
      name = "helicone-acm"  # This matches the acm workspace name
    }
  }
}

# Locals for better error handling
locals {
  # Check if we have valid EKS outputs
  has_load_balancer = try(data.terraform_remote_state.eks.outputs.load_balancer_hostname, null) != null
  
  # Zone IDs with better error handling - don't use empty string fallback
  # This will cause resources to not be created rather than fail with empty zone_id
  helicone_ai_zone_id = var.enable_helicone_ai_domain && length(data.cloudflare_zone.helicone_ai) > 0 ? data.cloudflare_zone.helicone_ai[0].id : null
  
  # Check if zones were found
  helicone_ai_zone_found = local.helicone_ai_zone_id != null
}

# Data source for the helicone.ai Cloudflare zone
# Using zone_id directly to avoid syntax issues with filters
data "cloudflare_zone" "helicone_ai" {
  count   = var.enable_helicone_ai_domain ? 1 : 0
  zone_id = "391fdcbd3e8173410d3353d4e78f82a4"  # helicone.ai zone ID
}

resource "cloudflare_dns_record" "helicone_ai_app" {
  count   = local.helicone_ai_zone_found && local.has_load_balancer ? 1 : 0
  zone_id = local.helicone_ai_zone_id
  name    = var.cloudflare_helicone_ai_subdomain
  content = data.terraform_remote_state.eks.outputs.load_balancer_hostname
  type    = "CNAME"
  ttl     = 1  # TTL=1 when proxied
  proxied = true  # Enable Cloudflare proxy for HTTPS termination

  comment = "Managed by Terraform - Points to AWS EKS load balancer with Cloudflare proxy for helicone.ai"
}

# Certificate validation records for helicone.ai ACM certificate
resource "cloudflare_dns_record" "helicone_ai_cert_validation" {
  for_each = local.helicone_ai_zone_found && data.terraform_remote_state.acm.outputs.certificate_helicone_ai_validation_options != null ? {
    for dvo in data.terraform_remote_state.acm.outputs.certificate_helicone_ai_validation_options : dvo.domain_name => {
      name   = dvo.resource_record_name
      record = dvo.resource_record_value
      type   = dvo.resource_record_type
    }
  } : {}

  zone_id = local.helicone_ai_zone_id
  name    = each.value.name
  content = each.value.record
  type    = each.value.type
  ttl     = 60

  comment = "Managed by Terraform - ACM certificate validation for helicone.ai"
  
  lifecycle {
    ignore_changes = [content, name, type]  # Ignore changes if record already exists
  }
}
