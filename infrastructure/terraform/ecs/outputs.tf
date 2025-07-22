output "ecs_cluster_name" {
  description = "Name of the ECS cluster"
  value       = aws_ecs_cluster.ai-gateway_service_cluster.name
}

output "ecs_cluster_arn" {
  description = "ARN of the ECS cluster"
  value       = aws_ecs_cluster.ai-gateway_service_cluster.arn
}

output "ecs_capacity_provider_name" {
  description = "Name of the ECS capacity provider"
  value       = aws_ecs_capacity_provider.ec2_capacity_provider.name
}

output "load_balancer_dns_name" {
  description = "DNS name of the load balancer"
  value       = aws_lb.ai_gateway_lb.dns_name
}

output "load_balancer_zone_id" {
  description = "Zone ID of the load balancer"
  value       = aws_lb.ai_gateway_lb.zone_id
}

output "autoscaling_group_name" {
  description = "Name of the Auto Scaling Group"
  value       = aws_autoscaling_group.ecs_ec2_asg.name
}

output "autoscaling_group_arn" {
  description = "ARN of the Auto Scaling Group"
  value       = aws_autoscaling_group.ecs_ec2_asg.arn
}

output "launch_template_id" {
  description = "ID of the launch template"
  value       = aws_launch_template.ecs_ec2_launch_template.id
}

output "security_group_ec2_id" {
  description = "ID of the security group for EC2 instances"
  value       = aws_security_group.ecs_ec2_sg.id
}

output "security_group_lb_id" {
  description = "ID of the security group for load balancer"
  value       = aws_security_group.load_balancer_sg.id
}

output "target_group_arn" {
  description = "ARN of the target group"
  value       = aws_lb_target_group.ai_gateway_tg.arn
}

output "ecs_service_name" {
  description = "Name of the ECS service"
  value       = aws_ecs_service.ai-gateway_service.name
}

output "instance_type" {
  description = "EC2 instance type being used"
  value       = var.instance_type
} 