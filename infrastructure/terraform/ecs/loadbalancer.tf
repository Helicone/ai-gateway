# Data sources to find existing VPC and subnets
data "aws_vpc" "default" {
  default = true
}

# Get default subnets
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

# Security group for the load balancer with inbound rules for HTTP and HTTPS
resource "aws_security_group" "load_balancer_sg" {
  name        = "ai-gateway-load-balancer-sg-${var.environment}"
  description = "Security group for ALB in ${var.environment} environment"
  vpc_id      = data.aws_vpc.default.id

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

resource "aws_lb" "fargate_lb" {
  name               = "ai-gateway-lb-${var.environment}"
  internal           = false
  load_balancer_type = "application"
  security_groups    = [aws_security_group.load_balancer_sg.id]
  subnets            = data.aws_subnets.default.ids
}

resource "aws_lb_target_group" "fargate_tg" {
  name     = "ai-gateway-tg-${var.environment}"
  port     = var.container_port
  protocol = "HTTP"
  vpc_id   = data.aws_vpc.default.id

  health_check {
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