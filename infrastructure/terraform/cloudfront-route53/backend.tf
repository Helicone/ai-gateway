terraform {
  backend "remote" {
    organization = "helicone"
    
    workspaces {
      name = "ai-gateway-cloudfront-route53"
    }
  }
}