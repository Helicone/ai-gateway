provider "aws" {
  region = var.aws_region
}

# Data sources for VPC and subnet information
data "aws_vpc" "default" {
  count   = var.create_subnet_group && var.vpc_id == "" ? 1 : 0
  default = true
}

data "aws_subnets" "default" {
  count = var.create_subnet_group && length(var.subnet_ids) == 0 ? 1 : 0
  filter {
    name   = "vpc-id"
    values = [var.vpc_id != "" ? var.vpc_id : data.aws_vpc.default[0].id]
  }
}

# Security Group for Valkey cache
resource "aws_security_group" "valkey_sg" {
  count       = var.create_security_group ? 1 : 0
  name        = "${var.valkey_cache_name}-sg"
  description = "Security group for Valkey serverless cache"
  vpc_id      = var.vpc_id != "" ? var.vpc_id : data.aws_vpc.default[0].id

  ingress {
    description     = "Valkey/Redis traffic"
    from_port       = 6379
    to_port         = 6379
    protocol        = "tcp"
    cidr_blocks     = var.allowed_cidr_blocks
    security_groups = var.allowed_security_group_ids
  }

  egress {
    description = "All outbound traffic"
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }

  tags = merge(var.common_tags, {
    Name = "${var.valkey_cache_name}-security-group"
  })
}

# Subnet Group for Valkey cache
resource "aws_elasticache_subnet_group" "valkey_subnet_group" {
  count      = var.create_subnet_group ? 1 : 0
  name       = "${var.valkey_cache_name}-subnet-group"
  # ElastiCache Serverless requires 2-3 subnets, so we take the first 3 available
  subnet_ids = length(var.subnet_ids) > 0 ? var.subnet_ids : slice(data.aws_subnets.default[0].ids, 0, min(3, length(data.aws_subnets.default[0].ids)))

  tags = merge(var.common_tags, {
    Name = "${var.valkey_cache_name}-subnet-group"
  })
}

# ElastiCache Serverless Cache for Valkey
resource "aws_elasticache_serverless_cache" "valkey" {
  engine               = var.engine
  name                 = var.valkey_cache_name
  description          = var.description
  major_engine_version = var.major_engine_version

  cache_usage_limits {
    data_storage {
      maximum = var.max_storage_gb
      unit    = "GB"
    }
    ecpu_per_second {
      maximum = var.max_ecpu_per_second
    }
  }

  daily_snapshot_time       = var.daily_snapshot_time
  snapshot_retention_limit  = var.snapshot_retention_limit
  
  security_group_ids = var.create_security_group ? [aws_security_group.valkey_sg[0].id] : var.allowed_security_group_ids
  subnet_ids        = var.create_subnet_group ? aws_elasticache_subnet_group.valkey_subnet_group[0].subnet_ids : var.subnet_ids

  tags = merge(var.common_tags, {
    Name = var.valkey_cache_name
  })
} 