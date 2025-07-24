terraform {
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "5.4.0"
    }
  }

  cloud {
    organization = "helicone"

    workspaces {
      name = "ai-gateway-ecs"
    }
  }
}

provider "aws" {
  region = var.region
}

# Provider for accessing secrets in us-west-2
provider "aws" {
  alias  = "secrets_region"
  region = var.secrets_region
}

# Data source to get current AWS account ID
data "aws_caller_identity" "current" {}

# Data sources for secrets using variable names
data "aws_secretsmanager_secret" "cloud_secrets" {
  provider = aws.secrets_region
  name     = var.secrets_manager_secret_name
}

# Data source to get route53-acm state outputs
data "terraform_remote_state" "route53_acm" {
  backend = "remote"

  config = {
    organization = "helicone"
    workspaces = {
      name = "helicone-route53-acm"
    }
  }
}
