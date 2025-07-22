terraform {
  cloud { 
    organization = "helicone" 
    workspaces { 
      name = "helicone-valkey" 
    } 
  }
  required_version = ">= 1.0"
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.80"
    }
  }
} 