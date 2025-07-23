# AWS Systems Manager Parameter Store parameters for AI Gateway configuration

# Parameter for MinIO host
resource "aws_ssm_parameter" "minio_host" {
  name        = "/ai-gateway/${var.environment}/minio/host"
  description = "MinIO host URL for AI Gateway"
  type        = "String"
  value       = var.minio_host
  tier        = "Standard"

  tags = {
    Name        = "ai-gateway-minio-host-${var.environment}"
    Environment = var.environment
  }
}

# Parameter for MinIO region
resource "aws_ssm_parameter" "minio_region" {
  name        = "/ai-gateway/${var.environment}/minio/region"
  description = "MinIO region for AI Gateway"
  type        = "String"
  value       = var.minio_region
  tier        = "Standard"

  tags = {
    Name        = "ai-gateway-minio-region-${var.environment}"
    Environment = var.environment
  }
}

# Parameter for Redis host
resource "aws_ssm_parameter" "redis_host" {
  name        = "/ai-gateway/${var.environment}/redis/host"
  description = "Redis host URL for AI Gateway"
  type        = "String"
  value       = var.redis_host
  tier        = "Standard"

  tags = {
    Name        = "ai-gateway-redis-host-${var.environment}"
    Environment = var.environment
  }
}