# Data sources to find existing VPC and subnets
data "aws_vpc" "selected" {
  id = var.vpc_id
}

# Get subnets in the specified VPC
data "aws_subnets" "default" {
  filter {
    name   = "vpc-id"
    values = [var.vpc_id]
  }
}

# Get subnet details to filter by AZ
data "aws_subnet" "all" {
  for_each = toset(length(var.public_subnet_ids) > 0 ? var.public_subnet_ids : data.aws_subnets.default.ids)
  id       = each.value
}

# Select one subnet per AZ for the load balancer
locals {
  # Group subnets by AZ and pick the first one from each AZ
  subnet_by_az = {
    for s in data.aws_subnet.all : s.availability_zone => s.id...
  }

  # Get one subnet per AZ
  lb_subnet_ids = [for az, subnets in local.subnet_by_az : subnets[0]]
}

# Data source for CloudFront managed prefix list
data "aws_ec2_managed_prefix_list" "cloudfront" {
  name = "com.amazonaws.global.cloudfront.origin-facing"
}

# Security group for the load balancer with inbound rules for HTTP and HTTPS
resource "aws_security_group" "load_balancer_sg" {
  name        = "ai-gateway-load-balancer-sg-${var.environment}"
  description = "Security group for ALB in ${var.environment} environment"
  vpc_id      = var.vpc_id

  # Allow HTTP from CloudFront only
  ingress {
    from_port       = 80
    to_port         = 80
    protocol        = "tcp"
    prefix_list_ids = [data.aws_ec2_managed_prefix_list.cloudfront.id]
  }

  # Allow HTTPS from CloudFront only
  ingress {
    from_port       = 443
    to_port         = 443
    protocol        = "tcp"
    prefix_list_ids = [data.aws_ec2_managed_prefix_list.cloudfront.id]
  }


  tags = {
    Name = "lb-sg-${var.environment}"
  }
}

# Separate egress rule to avoid circular dependency
resource "aws_security_group_rule" "lb_to_ecs_egress" {
  type                     = "egress"
  from_port                = var.container_port
  to_port                  = var.container_port
  protocol                 = "tcp"
  source_security_group_id = aws_security_group.ecs_tasks_sg.id
  security_group_id        = aws_security_group.load_balancer_sg.id
  description              = "Allow traffic to ECS tasks"
}

resource "aws_lb" "fargate_lb" {
  name               = "ai-gateway-lb-${var.environment}"
  internal           = false
  load_balancer_type = "application"
  security_groups    = [aws_security_group.load_balancer_sg.id]
  subnets            = local.lb_subnet_ids
}

resource "aws_lb_target_group" "fargate_tg" {
  name     = "ai-gateway-tg-${var.environment}"
  port     = var.container_port
  protocol = "HTTP"
  vpc_id   = var.vpc_id

  health_check {
    enabled             = true
    healthy_threshold   = 2
    unhealthy_threshold = 3
    timeout             = 5
    path                = "/health"
    protocol            = "HTTP"
    interval            = 30
    matcher             = "200"
  }

  target_type = "ip"

  lifecycle {
    create_before_destroy = true
  }
}



# HTTP Listener - redirects to HTTPS
resource "aws_lb_listener" "http_listener" {
  load_balancer_arn = aws_lb.fargate_lb.arn
  port              = 80
  protocol          = "HTTP"

  default_action {
    type = "redirect"

    redirect {
      port        = "443"
      protocol    = "HTTPS"
      status_code = "HTTP_301"
    }
  }

  lifecycle {
    create_before_destroy = true
  }
}

# HTTPS Listener - forwards to target group
resource "aws_lb_listener" "https_listener" {
  load_balancer_arn = aws_lb.fargate_lb.arn
  port              = 443
  protocol          = "HTTPS"
  ssl_policy        = "ELBSecurityPolicy-TLS13-1-2-2021-06"
  certificate_arn   = var.certificate_arn

  default_action {
    type             = "forward"
    target_group_arn = aws_lb_target_group.fargate_tg.arn
  }

  depends_on = [aws_lb_target_group.fargate_tg]

  lifecycle {
    create_before_destroy = true
  }
}