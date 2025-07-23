variable "environment" {
  type    = string
  default = "dev"
}

variable "region" {
  type    = string
  default = "us-east-1"
}

variable "ecr_repository_url" {
  description = "ECR repository URL for the AI Gateway image"
  type        = string
  default     = "849596434884.dkr.ecr.us-east-2.amazonaws.com/helicone/ai-gateway"
}

variable "image_tag" {
  description = "Tag for the Docker image"
  type        = string
  default     = "latest"
}

variable "container_port" {
  description = "Port the container listens on"
  type        = number
  default     = 8080
}

variable "secrets_manager_secret_name" {
  description = "Name of the AWS Secrets Manager secret containing application configuration"
  type        = string
  default     = "helicone/ai-gateway-cloud-secrets"
}

variable "secrets_region" {
  description = "AWS region where the secrets manager secret is stored"
  type        = string
  default     = "us-west-2"
}

variable "minio_host" {
  description = "MinIO host URL"
  type        = string
  default     = "https://s3.us-west-2.amazonaws.com"
}

variable "minio_region" {
  description = "MinIO region"
  type        = string
  default     = "us-west-2"
}

variable "redis_host" {
  description = "Redis host URL"
  type        = string
  default     = "rediss://helicone-cache.serverless.usw1.cache.amazonaws.com:5798"
}