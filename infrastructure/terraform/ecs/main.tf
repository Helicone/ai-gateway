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
  region = "us-east-1"
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
