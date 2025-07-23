# Helicone Route53 Module

This Terraform module manages Route53 DNS records and certificate validation for Helicone.

## Overview

This module:

- References the existing Route53 hosted zone for `heliconetest.com`
- Creates DNS validation records for ACM certificates
- Validates the ACM certificate for `heliconetest.com`
- Creates A records pointing to the EKS load balancer for various subdomains

## Dependencies

This module depends on:

- **EKS Module**: Provides the load balancer hostname
- **ACM Module**: Provides the certificate ARN and validation options

## Usage

```hcl
module "route53" {
  source = "./terraform/route53"

  region = "us-west-2"

  heliconetest_domain = "heliconetest.com"

  # Remote state configuration
  terraform_organization   = "helicone"
  eks_terraform_workspace = "helicone"
  acm_terraform_workspace = "helicone-acm"

  # Subdomain configuration
  enable_grafana_subdomain = true
  enable_argocd_subdomain  = true
  additional_subdomains    = ["api", "staging"]

  tags = {
    Environment = "production"
    Project     = "helicone"
    ManagedBy   = "terraform"
  }
}
```

## Requirements

- Terraform >= 1.0
- AWS Provider ~> 5.0
- Existing Route53 hosted zone for the domain
- Completed EKS module deployment (for load balancer)
- Completed ACM module deployment (for certificate)

## DNS Records Created

The module creates the following DNS records:

1. **Certificate Validation Records**: CNAME records for ACM certificate validation
2. **Main Domain A Record**: Points `heliconetest.com` to the load balancer
3. **Grafana Subdomain**: Points `grafana.heliconetest.com` to the load balancer (optional)
4. **ArgoCD Subdomain**: Points `argocd.heliconetest.com` to the load balancer (optional)
5. **Custom Subdomains**: Any additional subdomains specified in `additional_subdomains`

## Inputs

| Name                     | Description                                           | Type           | Default              | Required |
| ------------------------ | ----------------------------------------------------- | -------------- | -------------------- | :------: |
| region                   | AWS region for resources                              | `string`       | `"us-west-2"`        |    no    |
| tags                     | Common tags to apply to all resources                 | `map(string)`  | See variables.tf     |    no    |
| heliconetest_domain      | The primary domain for Route53 hosted zone            | `string`       | `"heliconetest.com"` |    no    |
| terraform_organization   | Terraform Cloud organization name                     | `string`       | `"helicone"`         |    no    |
| eks_terraform_workspace  | Name of the EKS Terraform workspace                   | `string`       | `"helicone"`         |    no    |
| acm_terraform_workspace  | Name of the ACM Terraform workspace                   | `string`       | `"helicone-acm"`     |    no    |
| enable_grafana_subdomain | Whether to create A record for grafana subdomain      | `bool`         | `true`               |    no    |
| enable_argocd_subdomain  | Whether to create A record for argocd subdomain       | `bool`         | `true`               |    no    |
| additional_subdomains    | List of additional subdomains to create A records for | `list(string)` | `[]`                 |    no    |

## Outputs

| Name                       | Description                                                           |
| -------------------------- | --------------------------------------------------------------------- |
| route53_zone_id            | ID of the Route53 hosted zone for heliconetest.com                    |
| route53_zone_name          | Name of the Route53 hosted zone                                       |
| certificate_validation_arn | ARN of the validated ACM certificate for heliconetest.com             |
| dns_records_created        | Information about created DNS records                                 |
| load_balancer_hostname     | Hostname of the load balancer (from EKS)                              |
| elb_zone_id                | Canonical hosted zone ID for Application Load Balancers in the region |

## Notes

- The module assumes a Route53 hosted zone already exists for `heliconetest.com`
- The ELB zone ID is hardcoded for `us-west-2`. Update this if deploying to a different region
- DNS records will only be created if a valid load balancer hostname is available from the EKS
  module
- Certificate validation must complete before the validated certificate ARN is available
- All A records use alias configuration to point to the Application Load Balancer
