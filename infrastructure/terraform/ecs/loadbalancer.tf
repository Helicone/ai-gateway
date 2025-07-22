# Data sources to find existing VPC and subnets
data "aws_vpc" "default" {
  default = true
}

data "aws_subnets" "default" {
  filter {
    name   = "vpc-id"
    values = [data.aws_vpc.default.id]
  }
  
  filter {
    name   = "default-for-az"
    values = ["true"]
  }
}

locals {
  vpc_id = data.aws_vpc.default.id
  subnets = data.aws_subnets.default.ids
}

# Security group for the load balancer with inbound rules for HTTP and HTTPS
resource "aws_security_group" "load_balancer_sg" {
  name        = "load-balancer-wizard-1-${var.environment}"
  description = "Security group for ALB in ${var.environment} environment"
  vpc_id      = local.vpc_id

  # Allow HTTP from anywhere
  ingress {
    from_port        = 80
    to_port          = 80
    protocol         = "tcp"
    cidr_blocks      = ["0.0.0.0/0"]
    ipv6_cidr_blocks = ["::/0"]
  }

  # Allow HTTPS from anywhere
  ingress {
    from_port        = 443
    to_port          = 443
    protocol         = "tcp"
    cidr_blocks      = ["0.0.0.0/0"]
    ipv6_cidr_blocks = ["::/0"]
  }

  # Standard outbound rule for unrestricted egress
  egress {
    from_port        = 0
    to_port          = 0
    protocol         = "-1"
    cidr_blocks      = ["0.0.0.0/0"]
    ipv6_cidr_blocks = ["::/0"]
  }

  tags = {
    Name = "lb-sg-${var.environment}"
  }
}

resource "aws_lb" "ai_gateway_lb" {
  name               = "ai-gateway-lb-${var.environment}"
  internal           = false
  load_balancer_type = "application"
  security_groups    = [aws_security_group.load_balancer_sg.id]
  subnets            = local.subnets
}

resource "aws_lb_target_group" "ai_gateway_tg" {
  name     = "ai-gateway-tg-${var.environment}"
  port     = 8080
  protocol = "HTTP"
  vpc_id   = local.vpc_id

  health_check {
    healthy_threshold   = 2
    unhealthy_threshold = 2
    timeout             = 5
    path                = "/health"
    protocol            = "HTTP"
    interval            = 30
    matcher             = "200"
  }

  target_type = "instance"

  lifecycle {
    create_before_destroy = true
  }
}
