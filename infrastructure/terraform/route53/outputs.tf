# Route53 outputs
output "route53_zone_id" {
  description = "ID of the Route53 hosted zone for heliconetest.com"
  value       = data.aws_route53_zone.helicone.zone_id
}

output "route53_zone_name" {
  description = "Name of the Route53 hosted zone"
  value       = data.aws_route53_zone.helicone.name
}

# Certificate validation output (from ACM module)
output "certificate_validation_arn" {
  description = "ARN of the validated ACM certificate for heliconetest.com (from ACM module)"
  value       = data.terraform_remote_state.acm.outputs.certificate_validation_arn
}

# DNS records info
output "dns_records_created" {
  description = "Information about created DNS records"
  value = {
    main_domain    = local.has_load_balancer ? aws_route53_record.helicone_main[0].name : "Not created - load balancer not available"
    grafana_domain = local.has_load_balancer && var.enable_grafana_subdomain ? aws_route53_record.helicone_grafana[0].name : "Not created - load balancer not available or subdomain disabled"
    argocd_domain  = local.has_load_balancer && var.enable_argocd_subdomain ? aws_route53_record.helicone_argocd[0].name : "Not created - load balancer not available or subdomain disabled"
    custom_domains = local.has_load_balancer ? [for record in aws_route53_record.custom_subdomains : record.name] : []
  }
}

# Load balancer info (passed through from EKS)
output "load_balancer_hostname" {
  description = "Hostname of the load balancer (from EKS)"
  value       = local.has_load_balancer ? data.terraform_remote_state.eks.outputs.load_balancer_hostname : null
}

# Zone ID for other modules that might need it
output "elb_zone_id" {
  description = "Canonical hosted zone ID for Application Load Balancers in the region"
  value       = local.elb_zone_id
} 