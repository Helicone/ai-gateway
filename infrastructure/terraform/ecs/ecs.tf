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
      name    = "ai-gateway-${var.environment}"
      image   = "${var.ecr_repository_url}:${var.image_tag}"
      command = ["/usr/local/bin/ai-gateway", "-c", "/etc/ai-gateway/helicone-cloud.yaml"]
      portMappings = [
        {
          containerPort = var.container_port
          hostPort      = var.container_port
        }
      ]

      # Add secrets here using the data source ARNs
      secrets = [
        {
          name      = "AI_GATEWAY__DATABASE__URL"
          valueFrom = "${data.aws_secretsmanager_secret.cloud_secrets.arn}:AI_GATEWAY__DATABASE__URL::"
        },
        {
          name      = "PGSSLROOTCERT"
          valueFrom = "${data.aws_secretsmanager_secret.cloud_secrets.arn}:PGSSLROOTCERT::"
        },
        {
          name      = "AI_GATEWAY__MINIO__ACCESS_KEY"
          valueFrom = "${data.aws_secretsmanager_secret.cloud_secrets.arn}:AI_GATEWAY__MINIO__ACCESS_KEY::"
        },
        {
          name      = "AI_GATEWAY__MINIO__SECRET_KEY"
          valueFrom = "${data.aws_secretsmanager_secret.cloud_secrets.arn}:AI_GATEWAY__MINIO__SECRET_KEY::"
        },
        {
          name      = "AI_GATEWAY__MINIO__HOST"
          valueFrom = aws_ssm_parameter.minio_host.arn
        },
        {
          name      = "AI_GATEWAY__MINIO__REGION"
          valueFrom = aws_ssm_parameter.minio_region.arn
        },
        {
          name      = "AI_GATEWAY__CACHE_STORE__HOST_URL"
          valueFrom = aws_ssm_parameter.redis_host.arn
        },
        {
          name      = "AI_GATEWAY__RATE_LIMIT_STORE__HOST_URL"
          valueFrom = aws_ssm_parameter.redis_host.arn
        }
      ]

      # For plain text environment variables
      environment = [
        {
          name  = "NO_COLOR"
          value = "true"
        }
      ]

      healthCheck = {
        command     = ["CMD-SHELL", "curl -f http://localhost:${var.container_port}/health || exit 1"]
        interval    = 20
        timeout     = 5
        retries     = 3
        startPeriod = 60
      }

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
  name                              = "ai-gateway-service-${var.environment}"
  cluster                           = aws_ecs_cluster.ai-gateway_service_cluster.id
  task_definition                   = aws_ecs_task_definition.ai-gateway_task.arn
  launch_type                       = "FARGATE"
  desired_count                     = 2
  force_new_deployment              = true
  health_check_grace_period_seconds = 45

  network_configuration {
    subnets          = length(var.private_subnet_ids) > 0 ? var.private_subnet_ids : data.aws_subnets.default.ids
    security_groups  = [aws_security_group.ecs_tasks_sg.id]
    assign_public_ip = true
  }

  load_balancer {
    target_group_arn = aws_lb_target_group.fargate_tg.arn
    container_name   = "ai-gateway-${var.environment}"
    container_port   = var.container_port
  }

  depends_on = [aws_lb_listener.https_listener]

}

# Security group for ECS tasks
resource "aws_security_group" "ecs_tasks_sg" {
  name        = "ai-gateway-tasks-sg-${var.environment}"
  description = "Security group for ECS tasks in ${var.environment} environment"
  vpc_id      = var.vpc_id

  # Allow all outbound traffic
  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
    description = "Allow all outbound traffic"
  }

  tags = {
    Name = "ai-gw-sg-${var.environment}"
  }
}

# Separate ingress rule to avoid circular dependency
resource "aws_security_group_rule" "ecs_from_lb_ingress" {
  type                     = "ingress"
  from_port                = var.container_port
  to_port                  = var.container_port
  protocol                 = "tcp"
  source_security_group_id = aws_security_group.load_balancer_sg.id
  security_group_id        = aws_security_group.ecs_tasks_sg.id
  description              = "Allow traffic from load balancer"
}

resource "null_resource" "scale_down_ecs_service" {
  triggers = {
    service_name = aws_ecs_service.ai-gateway_service.name
  }

  provisioner "local-exec" {
    command = "aws ecs update-service --region ${var.region} --cluster ${aws_ecs_cluster.ai-gateway_service_cluster.id} --service ${self.triggers.service_name} --desired-count 0"
  }
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
  name        = "ai_gw_ecs_cloudwatch_policy_${var.environment}"
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

resource "aws_iam_policy" "ecs_secrets_manager_policy" {
  name        = "ecs_secrets_manager_policy_${var.environment}"
  description = "Allows ECS tasks to access AWS Secrets Manager"

  policy = jsonencode({
    Version = "2012-10-17",
    Statement = [
      {
        Effect = "Allow",
        Action = [
          "secretsmanager:GetSecretValue"
        ],
        Resource = "arn:aws:secretsmanager:${var.secrets_region}:${data.aws_caller_identity.current.account_id}:secret:${var.secrets_manager_secret_name}*"
      },
    ]
  })
}

resource "aws_iam_policy" "ecs_parameter_store_policy" {
  name        = "ecs_parameter_store_policy_${var.environment}"
  description = "Allows ECS tasks to access AWS Systems Manager Parameter Store"

  policy = jsonencode({
    Version = "2012-10-17",
    Statement = [
      {
        Effect = "Allow",
        Action = [
          "ssm:GetParameter",
          "ssm:GetParameters"
        ],
        Resource = [
          "arn:aws:ssm:${var.region}:${data.aws_caller_identity.current.account_id}:parameter/ai-gateway/${var.environment}/*"
        ]
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

resource "aws_iam_role_policy_attachment" "ecs_secrets_manager_policy_attach" {
  role       = aws_iam_role.ecs_execution_role.name
  policy_arn = aws_iam_policy.ecs_secrets_manager_policy.arn
}

resource "aws_iam_role_policy_attachment" "ecs_parameter_store_policy_attach" {
  role       = aws_iam_role.ecs_execution_role.name
  policy_arn = aws_iam_policy.ecs_parameter_store_policy.arn
}

# Attach the AWS managed ECS task execution role policy
resource "aws_iam_role_policy_attachment" "ecs_task_execution_role_policy" {
  role       = aws_iam_role.ecs_execution_role.name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AmazonECSTaskExecutionRolePolicy"
}

