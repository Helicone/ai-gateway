output "load_balancer_dns_name" {
  description = "DNS name of the load balancer"
  value       = aws_lb.fargate_lb.dns_name
}

output "load_balancer_zone_id" {
  description = "Zone ID of the load balancer"
  value       = aws_lb.fargate_lb.zone_id
}

output "ecs_cluster_name" {
  description = "Name of the ECS cluster"
  value       = aws_ecs_cluster.ai-gateway_service_cluster.name
}

output "ecs_service_name" {
  description = "Name of the ECS service"
  value       = aws_ecs_service.ai-gateway_service.name
}

output "ecs_cluster_arn" {
  description = "ARN of the ECS cluster"
  value       = aws_ecs_cluster.ai-gateway_service_cluster.arn
}

output "ecs_service_arn" {
  description = "ARN of the ECS service"
  value       = aws_ecs_service.ai-gateway_service.id
}

output "target_group_arn" {
  description = "ARN of the target group"
  value       = aws_lb_target_group.fargate_tg.arn
}

output "load_balancer_security_group_id" {
  description = "Security group ID for the load balancer"
  value       = aws_security_group.load_balancer_sg.id
}

output "ecs_tasks_security_group_id" {
  description = "Security group ID for ECS tasks"
  value       = aws_security_group.ecs_tasks_sg.id
}

output "endpoint_url" {
  description = "Full HTTPS endpoint URL"
  value       = "https://${aws_lb.fargate_lb.dns_name}"
}

output "health_check_url" {
  description = "Health check endpoint URL"
  value       = "https://${aws_lb.fargate_lb.dns_name}/health"
}

output "http_redirect_info" {
  description = "HTTP requests are automatically redirected to HTTPS"
  value       = "HTTP requests to http://${aws_lb.fargate_lb.dns_name} will be redirected to https://${aws_lb.fargate_lb.dns_name}"
}

output "ecr_repository_url" {
  description = "URL of the ECR repository"
  value       = aws_ecr_repository.ai_gateway.repository_url
}

output "ecr_repository_arn" {
  description = "ARN of the ECR repository"
  value       = aws_ecr_repository.ai_gateway.arn
}

output "secrets_manager_secret_arn" {
  description = "ARN of the AWS Secrets Manager secret being used"
  value       = data.aws_secretsmanager_secret.cloud_secrets.arn
}

output "secrets_manager_secret_name" {
  description = "Name of the AWS Secrets Manager secret being used"
  value       = var.secrets_manager_secret_name
}

output "secrets_manager_region" {
  description = "AWS region where the secrets manager secret is stored"
  value       = var.secrets_region
}

output "vpc_id" {
  description = "VPC ID where ECS is deployed"
  value       = var.vpc_id
}

output "subnet_ids" {
  description = "Subnet IDs where ECS tasks run"
  value       = data.aws_subnets.default.ids
}

output "autoscaling_target_resource_id" {
  description = "Resource ID of the autoscaling target"
  value       = aws_appautoscaling_target.ecs_target.resource_id
}

output "autoscaling_policy_name" {
  description = "Name of the autoscaling policy"
  value       = aws_appautoscaling_policy.ecs_cpu_policy.name
}

output "autoscaling_min_capacity" {
  description = "Minimum number of tasks for autoscaling"
  value       = aws_appautoscaling_target.ecs_target.min_capacity
}

output "autoscaling_max_capacity" {
  description = "Maximum number of tasks for autoscaling"
  value       = aws_appautoscaling_target.ecs_target.max_capacity
}

output "autoscaling_target_cpu_utilization" {
  description = "Target CPU utilization percentage for autoscaling"
  value       = aws_appautoscaling_policy.ecs_cpu_policy.target_tracking_scaling_policy_configuration[0].target_value
} 