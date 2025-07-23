# ECR Repository for AI Gateway
resource "aws_ecr_repository" "ai_gateway" {
  name                 = "helicone/ai-gateway"
  image_tag_mutability = "MUTABLE"

  image_scanning_configuration {
    scan_on_push = true
  }

  tags = {
    Name        = "ai-gateway-${var.environment}"
    Environment = var.environment
  }
}

# Note: Using existing ECR repository from us-east-2 for now
# Cross-region ECR access is supported and works fine for ECS

# ECR Repository Policy is not needed - ECS execution role already has ECR access via attached policies

# ECS Cluster
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
resource "aws_ecs_task_definition" "ai-gateway_task" {
  family                   = "ai-gateway-${var.environment}"
  network_mode             = "awsvpc"
  requires_compatibilities = ["FARGATE"]
  execution_role_arn       = aws_iam_role.ecs_execution_role.arn
  cpu                      = "256"
  memory                   = "1024"

  container_definitions = jsonencode([
    {
      name  = "ai-gateway-${var.environment}"
      image = "${var.ecr_repository_url}:${var.image_tag}"
      portMappings = [
        {
          containerPort = var.container_port
          hostPort      = var.container_port
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

# ECS Service
resource "aws_ecs_service" "ai-gateway_service" {
  name                 = "ai-gateway-service-${var.environment}"
  cluster              = aws_ecs_cluster.ai-gateway_service_cluster.id
  task_definition      = aws_ecs_task_definition.ai-gateway_task.arn
  launch_type          = "FARGATE"
  desired_count        = 3
  force_new_deployment = true

  network_configuration {
    subnets          = local.subnets
    security_groups  = [aws_security_group.load_balancer_sg.id]
    assign_public_ip = true
  }

  load_balancer {
    target_group_arn = aws_lb_target_group.fargate_tg.arn
    container_name   = "ai-gateway-${var.environment}"
    container_port   = var.container_port
  }

  depends_on = [aws_lb_listener.http_listener]

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
  load_balancer_arn = aws_lb.fargate_lb.arn
  port              = 80
  protocol          = "HTTP"

  default_action {
    type             = "forward"
    target_group_arn = aws_lb_target_group.fargate_tg.arn
  }

  depends_on = [aws_lb_target_group.fargate_tg]

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
