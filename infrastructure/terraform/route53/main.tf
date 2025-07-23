terraform {
  required_version = ">= 1.0"
  
  cloud { 
    organization = "helicone" 

    workspaces { 
      name = "helicone-route53" 
    } 
  }

  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.0"
    }
  }
}

# AWS Provider Configuration
provider "aws" {
  region = var.region
  
  default_tags {
    tags = var.tags
  }
}

# Data source to get ACM state outputs from Terraform Cloud
data "terraform_remote_state" "acm" {
  backend = "remote"
  
  config = {
    organization = var.terraform_organization
    workspaces = {
      name = var.acm_terraform_workspace
    }
  }
}

# Route 53 configuration for heliconetest.com
# This assumes you have a hosted zone already created for heliconetest.com
data "aws_route53_zone" "helicone" {
  name         = var.heliconetest_domain
  private_zone = false
}

# Local value for ELB zone ID (us-west-2) - Application Load Balancer
locals {
  elb_zone_id = "Z1H1FL5HABSF5"  # This is the canonical hosted zone ID for Application Load Balancers in us-west-2
  
  # Check if we have valid EKS outputs
  has_load_balancer = try(data.terraform_remote_state.eks.outputs.load_balancer_hostname, null) != null
}

# Create A record for heliconetest.com pointing to the load balancer
resource "aws_route53_record" "helicone_main" {
  count           = local.has_load_balancer ? 1 : 0
  zone_id         = data.aws_route53_zone.helicone.zone_id
  name            = var.heliconetest_domain
  type            = "A"
  allow_overwrite = true

  alias {
    name                   = data.terraform_remote_state.eks.outputs.load_balancer_hostname
    zone_id                = local.elb_zone_id
    evaluate_target_health = true
  }
}

# Create A record for grafana.heliconetest.com pointing to the load balancer
resource "aws_route53_record" "helicone_grafana" {
  count           = local.has_load_balancer && var.enable_grafana_subdomain ? 1 : 0
  zone_id         = data.aws_route53_zone.helicone.zone_id
  name            = "grafana.${var.heliconetest_domain}"
  type            = "A"
  allow_overwrite = true

  alias {
    name                   = data.terraform_remote_state.eks.outputs.load_balancer_hostname
    zone_id                = local.elb_zone_id
    evaluate_target_health = true
  }
}

# Create A record for argocd.heliconetest.com pointing to the load balancer
resource "aws_route53_record" "helicone_argocd" {
  count           = local.has_load_balancer && var.enable_argocd_subdomain ? 1 : 0
  zone_id         = data.aws_route53_zone.helicone.zone_id
  name            = "argocd.${var.heliconetest_domain}"
  type            = "A"
  allow_overwrite = true

  alias {
    name                   = data.terraform_remote_state.eks.outputs.load_balancer_hostname
    zone_id                = local.elb_zone_id
    evaluate_target_health = true
  }
}

# Additional A records for any custom subdomains
resource "aws_route53_record" "custom_subdomains" {
  for_each = local.has_load_balancer ? toset(var.additional_subdomains) : toset([])
  
  zone_id         = data.aws_route53_zone.helicone.zone_id
  name            = "${each.value}.${var.heliconetest_domain}"
  type            = "A"
  allow_overwrite = true

  alias {
    name                   = data.terraform_remote_state.eks.outputs.load_balancer_hostname
    zone_id                = local.elb_zone_id
    evaluate_target_health = true
  }
} 