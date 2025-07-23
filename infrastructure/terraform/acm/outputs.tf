# ACM Certificate outputs for heliconetest.com
output "certificate_arn" {
  description = "ARN of the ACM certificate for heliconetest.com"
  value       = aws_acm_certificate.helicone_cert.arn
}

output "certificate_domain_name" {
  description = "Domain name of the ACM certificate"
  value       = aws_acm_certificate.helicone_cert.domain_name
}

# ACM Certificate outputs for helicone-test.com
# output "certificate_helicone_test_arn" {
#   description = "ARN of the ACM certificate for helicone-test.com"
#   value       = var.enable_helicone_test_domain ? aws_acm_certificate.helicone_test_cert[0].arn : null
# }

# output "certificate_helicone_test_domain_name" {
#   description = "Domain name of the ACM certificate for helicone-test.com"
#   value       = var.enable_helicone_test_domain ? aws_acm_certificate.helicone_test_cert[0].domain_name : null
# }

# output "certificate_helicone_test_validation_options" {
#   description = "Certificate validation options for helicone-test.com (to be added to Cloudflare)"
#   value       = var.enable_helicone_test_domain ? aws_acm_certificate.helicone_test_cert[0].domain_validation_options : null
#   sensitive   = false
# }

# ACM Certificate outputs for helicone.ai
# output "certificate_helicone_ai_arn" {
#   description = "ARN of the ACM certificate for helicone.ai"
#   value       = aws_acm_certificate.helicone_ai_cert.arn
# }

# output "certificate_helicone_ai_domain_name" {
#   description = "Domain name of the ACM certificate for helicone.ai"
#   value       = aws_acm_certificate.helicone_ai_cert.domain_name
# }

# output "certificate_helicone_ai_validation_options" {
#   description = "Certificate validation options for helicone.ai (to be added to Cloudflare)"
#   value       = aws_acm_certificate.helicone_ai_cert.domain_validation_options
#   sensitive   = false
# } 