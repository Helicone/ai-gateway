output "load_balancer_dns_name" {
  description = "DNS name of the load balancer"
  value       = aws_lb.fargate_lb.dns_name
}

output "load_balancer_zone_id" {
  description = "Hosted zone ID of the load balancer"
  value       = aws_lb.fargate_lb.zone_id
}

output "ecs_cluster_name" {
  description = "Name of the ECS cluster"
  value       = aws_ecs_cluster.ai-gateway-service-cluster.name
}

output "ecs_service_name" {
  description = "Name of the ECS service"
  value       = aws_ecs_service.ai-gateway-service.name
}

output "ecs_cluster_arn" {
  description = "ARN of the ECS cluster"
  value       = aws_ecs_cluster.ai-gateway-service-cluster.arn
}

output "ecs_service_arn" {
  description = "ARN of the ECS service"
  value       = aws_ecs_service.ai-gateway-service.id
}

output "target_group_arn" {
  description = "ARN of the target group"
  value       = aws_lb_target_group.fargate_tg.arn
}

output "security_group_id" {
  description = "ID of the security group"
  value       = aws_security_group.load_balancer_sg.id
}

output "endpoint_url" {
  description = "Full HTTP endpoint URL"
  value       = "http://${aws_lb.fargate_lb.dns_name}"
}

output "health_check_url" {
  description = "Health check endpoint URL"
  value       = "http://${aws_lb.fargate_lb.dns_name}/health"
} 