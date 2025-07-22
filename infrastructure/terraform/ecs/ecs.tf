# ECS Cluster with Capacity Providers
resource "aws_ecs_cluster" "ai-gateway_service_cluster" {
  name = "ai-gateway-cluster-${var.environment}"
}

# CloudWatch Log Group for ECS
resource "aws_cloudwatch_log_group" "ecs_log_group" {
  name              = "/ecs/ai-gateway-${var.environment}"
  retention_in_days = 30

  tags = {
    Name        = "ai-gateway-${var.environment}"
    Environment = var.environment
  }
}

# ECS Task Definition
# NOTE: ECR repository is in us-east-2, but ECS is in us-east-1
# Cross-region ECR access is allowed but may have performance implications
resource "aws_ecs_task_definition" "ai-gateway_task" {
  family                   = "ai-gateway-${var.environment}"
  network_mode             = "host"
  requires_compatibilities = ["EC2"]
  execution_role_arn       = aws_iam_role.ecs_execution_role.arn
  cpu                      = "1024"
  memory                   = "2048"

  container_definitions = jsonencode([
    {
      name  = "ai-gateway-${var.environment}"
      image = "849596434884.dkr.ecr.us-east-2.amazonaws.com/helicone/ai-gateway:latest"
      portMappings = [
        {
          containerPort = 8080
          protocol      = "tcp"
        }
      ]

      logConfiguration = {
        logDriver = "awslogs"
        options = {
          "awslogs-group"         = "/ecs/ai-gateway-${var.environment}"
          "awslogs-region"        = var.region
          "awslogs-stream-prefix" = "ecs"
        }
      }
    }
  ])
}

# Security group for EC2 instances
resource "aws_security_group" "ecs_ec2_sg" {
  name        = "ecs-ec2-sg-${var.environment}"
  description = "Security group for ECS EC2 instances"
  vpc_id      = local.vpc_id

  # Allow traffic from load balancer on port 8080 (host networking)
  ingress {
    from_port       = 8080
    to_port         = 8080
    protocol        = "tcp"
    security_groups = [aws_security_group.load_balancer_sg.id]
  }

  # Allow SSH access
  ingress {
    from_port   = 22
    to_port     = 22
    protocol    = "tcp"
    cidr_blocks = ["0.0.0.0/0"]
  }

  # Standard outbound rule
  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }

  tags = {
    Name = "ecs-ec2-sg-${var.environment}"
  }
}

# IAM Role for EC2 instances
resource "aws_iam_role" "ecs_ec2_instance_role" {
  name = "ecs_ec2_instance_role_${var.environment}"

  assume_role_policy = jsonencode({
    Version = "2012-10-17",
    Statement = [
      {
        Action = "sts:AssumeRole",
        Effect = "Allow",
        Principal = {
          Service = "ec2.amazonaws.com"
        }
      }
    ]
  })
}

# IAM Instance Profile for EC2 instances
resource "aws_iam_instance_profile" "ecs_ec2_instance_profile" {
  name = "ecs_ec2_instance_profile_${var.environment}"
  role = aws_iam_role.ecs_ec2_instance_role.name
}

# Attach ECS instance role policy
resource "aws_iam_role_policy_attachment" "ecs_ec2_instance_role_policy" {
  role       = aws_iam_role.ecs_ec2_instance_role.name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AmazonEC2ContainerServiceforEC2Role"
}

# Attach CloudWatch agent policy for EC2 instances
resource "aws_iam_role_policy_attachment" "ecs_ec2_cloudwatch_agent_policy" {
  role       = aws_iam_role.ecs_ec2_instance_role.name
  policy_arn = "arn:aws:iam::aws:policy/CloudWatchAgentServerPolicy"
}

# Get the latest ECS-optimized Amazon Linux 2 AMI
data "aws_ami" "ecs_optimized" {
  most_recent = true
  owners      = ["amazon"]

  filter {
    name   = "name"
    values = ["amzn2-ami-ecs-hvm-*-x86_64-ebs"]
  }

  filter {
    name   = "virtualization-type"
    values = ["hvm"]
  }
}

# Minimal launch template without user data
resource "aws_launch_template" "ecs_ec2_launch_template" {
  name_prefix   = "ecs-${var.environment}-"
  image_id      = data.aws_ami.ecs_optimized.id
  instance_type = var.instance_type
  
  vpc_security_group_ids = [aws_security_group.ecs_ec2_sg.id]
  
  iam_instance_profile {
    name = aws_iam_instance_profile.ecs_ec2_instance_profile.name
  }

  # No user data needed - capacity provider handles cluster registration

  lifecycle {
    create_before_destroy = true
  }
}

# Auto Scaling Group for EC2 instances
resource "aws_autoscaling_group" "ecs_ec2_asg" {
  name                = "ecs-asg-${var.environment}"
  vpc_zone_identifier = local.subnets
  target_group_arns   = [aws_lb_target_group.ai_gateway_tg.arn]
  health_check_type   = "ELB"
  health_check_grace_period = 300

  min_size         = var.asg_min_size
  max_size         = var.asg_max_size
  desired_capacity = var.asg_desired_capacity

  launch_template {
    id      = aws_launch_template.ecs_ec2_launch_template.id
    version = "$Latest"
  }

  # Required tag for ECS capacity provider to manage this ASG
  tag {
    key                 = "AmazonECSManaged"
    value               = true
    propagate_at_launch = false
  }

  tag {
    key                 = "Name"
    value               = "ecs-instance-${var.environment}"
    propagate_at_launch = true
  }

  tag {
    key                 = "Environment"
    value               = var.environment
    propagate_at_launch = true
  }

  lifecycle {
    create_before_destroy = true
  }
}

# ECS Capacity Provider
resource "aws_ecs_capacity_provider" "ec2_capacity_provider" {
  name = "ec2-capacity-provider-${var.environment}"

  auto_scaling_group_provider {
    auto_scaling_group_arn         = aws_autoscaling_group.ecs_ec2_asg.arn
    managed_termination_protection = "DISABLED"

    managed_scaling {
      maximum_scaling_step_size = 2
      minimum_scaling_step_size = 1
      status                    = "ENABLED"
      target_capacity           = 100
    }
  }
}

# Associate capacity provider with ECS cluster
resource "aws_ecs_cluster_capacity_providers" "cluster_capacity_providers" {
  cluster_name = aws_ecs_cluster.ai-gateway_service_cluster.name

  capacity_providers = [aws_ecs_capacity_provider.ec2_capacity_provider.name]

  default_capacity_provider_strategy {
    base              = 1
    weight            = 100
    capacity_provider = aws_ecs_capacity_provider.ec2_capacity_provider.name
  }
}

# ECS Service using capacity provider
resource "aws_ecs_service" "ai-gateway_service" {
  name                 = "ai-gateway-service-${var.environment}"
  cluster              = aws_ecs_cluster.ai-gateway_service_cluster.id
  task_definition      = aws_ecs_task_definition.ai-gateway_task.arn
  desired_count        = 3
  force_new_deployment = true

  capacity_provider_strategy {
    capacity_provider = aws_ecs_capacity_provider.ec2_capacity_provider.name
    weight           = 100
  }

  load_balancer {
    target_group_arn = aws_lb_target_group.ai_gateway_tg.arn
    container_name   = "ai-gateway-${var.environment}"
    container_port   = 8080
  }

  depends_on = [aws_lb_listener.http_listener, aws_ecs_cluster_capacity_providers.cluster_capacity_providers]

  lifecycle {
    ignore_changes = [desired_count]
  }
}

resource "null_resource" "scale_down_ecs_service" {
  triggers = {
    service_name = aws_ecs_service.ai-gateway_service.name
  }

  provisioner "local-exec" {
    command = "aws ecs update-service --region ${var.region} --cluster ${aws_ecs_cluster.ai-gateway_service_cluster.id} --service ${self.triggers.service_name} --desired-count 0"
  }
}

variable "use_remote_certificate" {
  description = "Whether to use certificate from remote state or local data source"
  type        = bool
  default     = false
}

# HTTP Listener (temporary - use while resolving certificate issues)
resource "aws_lb_listener" "http_listener" {
  load_balancer_arn = aws_lb.ai_gateway_lb.arn
  port              = 80
  protocol          = "HTTP"

  default_action {
    type             = "forward"
    target_group_arn = aws_lb_target_group.ai_gateway_tg.arn
  }

  depends_on = [aws_lb_target_group.ai_gateway_tg]

  lifecycle {
    create_before_destroy = true
  }
}

resource "aws_security_group_rule" "egress_https" {
  type              = "egress"
  from_port         = 443
  to_port           = 443
  protocol          = "tcp"
  cidr_blocks       = ["0.0.0.0/0"]
  security_group_id = aws_security_group.load_balancer_sg.id
}

# IAM Role for ECS Task Execution
resource "aws_iam_role" "ecs_execution_role" {
  name = "ecs_execution_role_${var.environment}"

  assume_role_policy = jsonencode({
    Version = "2012-10-17",
    Statement = [
      {
        Effect = "Allow",
        Principal = {
          Service = "ecs-tasks.amazonaws.com"
        },
        Action = "sts:AssumeRole"
      },
    ]
  })
}

resource "aws_iam_policy" "ecs_ecr_policy" {
  name        = "ecs_ecr_policy_${var.environment}"
  description = "Allows ECS tasks to interact with ECR"

  policy = jsonencode({
    Version = "2012-10-17",
    Statement = [
      {
        Effect = "Allow",
        Action = [
          "ecr:GetDownloadUrlForLayer",
          "ecr:BatchGetImage",
          "ecr:BatchCheckLayerAvailability",
          "ecr:GetAuthorizationToken"
        ],
        Resource = "*"
      },
    ]
  })
}

resource "aws_iam_policy" "ecs_cloudwatch_policy" {
  name        = "ecs_cloudwatch_policy_${var.environment}"
  description = "Allows ECS tasks to write to CloudWatch Logs"

  policy = jsonencode({
    Version = "2012-10-17",
    Statement = [
      {
        Effect = "Allow",
        Action = [
          "logs:CreateLogGroup",
          "logs:CreateLogStream",
          "logs:PutLogEvents"
        ],
        Resource = "arn:aws:logs:${var.region}:*:*"
      },
    ]
  })
}

resource "aws_iam_role_policy_attachment" "ecs_ecr_policy_attach" {
  role       = aws_iam_role.ecs_execution_role.name
  policy_arn = aws_iam_policy.ecs_ecr_policy.arn
}

resource "aws_iam_role_policy_attachment" "ecs_cloudwatch_policy_attach" {
  role       = aws_iam_role.ecs_execution_role.name
  policy_arn = aws_iam_policy.ecs_cloudwatch_policy.arn
}

# Attach the AWS managed ECS task execution role policy
resource "aws_iam_role_policy_attachment" "ecs_task_execution_role_policy" {
  role       = aws_iam_role.ecs_execution_role.name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AmazonECSTaskExecutionRolePolicy"
}
