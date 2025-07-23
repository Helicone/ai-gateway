# AWS Provider Configuration
terraform {
  required_version = ">= 1.0"
  
  cloud { 
    organization = "helicone" 

    workspaces { 
      name = "helicone-acm" 
    } 
  }

  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.0"
    }
  }
} 

provider "aws" {
  region = var.region
  
  default_tags {
    tags = var.tags
  }
}

# ACM Certificate for heliconetest.com
resource "aws_acm_certificate" "helicone_cert" {
  domain_name               = var.heliconetest_domain
  subject_alternative_names = ["*.${var.heliconetest_domain}"]
  validation_method         = "EMAIL"

  lifecycle {
    create_before_destroy = true
  }

  tags = merge(var.tags, {
    Name = var.heliconetest_domain
  })
}

# ACM Certificate for helicone-test.com (Cloudflare managed)
# resource "aws_acm_certificate" "helicone_test_cert" {
#   count                     = var.enable_helicone_test_domain ? 1 : 0
#   domain_name               = var.helicone_test_domain
#   subject_alternative_names = ["*.${var.helicone_test_domain}"]
#   validation_method         = "DNS"

#   lifecycle {
#     create_before_destroy = true
#   }

#   tags = merge(var.tags, {
#     Name = var.helicone_test_domain
#   })
# }

# TODO Check if this certificate is needed.
# ACM Certificate for helicone.ai (Cloudflare managed)
# resource "aws_acm_certificate" "helicone_ai_cert" {
#   domain_name               = var.helicone_ai_domain
#   subject_alternative_names = ["*.${var.helicone_ai_domain}"]
#   validation_method         = "DNS"

#   lifecycle {
#     create_before_destroy = true
#   }

#   tags = merge(var.tags, {
#     Name = var.helicone_ai_domain
#   })
# } 