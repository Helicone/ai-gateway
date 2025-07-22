variable "aws_region" {
  description = "AWS region"
  type        = string
  default     = "us-west-1"
}

variable "project_name" {
  description = "Name of the project"
  type        = string
  default     = "helicone"
}

variable "environment" {
  description = "Environment name"
  type        = string
  default     = "dev"
}

variable "common_tags" {
  description = "Common tags to be applied to all resources"
  type        = map(string)
  default = {
    Project     = "helicone"
    Environment = "dev"
    ManagedBy   = "terraform"
  }
}

variable "valkey_cache_name" {
  description = "Name of the Valkey serverless cache"
  type        = string
  default     = "helicone-valkey-cache"
}

variable "engine" {
  description = "Cache engine to use (valkey or redis)"
  type        = string
  default     = "valkey"
  validation {
    condition     = contains(["valkey", "redis"], var.engine)
    error_message = "Engine must be either 'valkey' or 'redis'."
  }
}

variable "major_engine_version" {
  description = "Major engine version"
  type        = string
  default     = "8"
}

variable "max_storage_gb" {
  description = "Maximum storage in GB for the serverless cache"
  type        = number
  default     = 20
  validation {
    condition     = var.max_storage_gb >= 1 && var.max_storage_gb <= 5000
    error_message = "Max storage must be between 1 and 5000 GB."
  }
}

variable "max_ecpu_per_second" {
  description = "Maximum ECPU per second for the serverless cache"
  type        = number
  default     = 100000
  validation {
    condition     = var.max_ecpu_per_second >= 1000 && var.max_ecpu_per_second <= 15000000
    error_message = "Max ECPU per second must be between 1000 and 15000000."
  }
}

variable "snapshot_retention_limit" {
  description = "Number of days for which ElastiCache retains automatic snapshots"
  type        = number
  default     = 1
  validation {
    condition     = var.snapshot_retention_limit >= 0 && var.snapshot_retention_limit <= 35
    error_message = "Snapshot retention limit must be between 0 and 35 days."
  }
}

variable "daily_snapshot_time" {
  description = "Daily time when ElastiCache begins taking a daily snapshot in HH:MM UTC format"
  type        = string
  default     = "03:00"
}

variable "create_subnet_group" {
  description = "Whether to create a subnet group for the cache"
  type        = bool
  default     = true
}

variable "subnet_ids" {
  description = "List of subnet IDs for the cache subnet group"
  type        = list(string)
  default     = []
}

variable "create_security_group" {
  description = "Whether to create a security group for the cache"
  type        = bool
  default     = true
}

variable "vpc_id" {
  description = "VPC ID where the cache will be created"
  type        = string
  default     = ""
}

variable "allowed_cidr_blocks" {
  description = "List of CIDR blocks allowed to access the cache (leave empty for ECS-only access)"
  type        = list(string)
  default     = []
}

variable "allowed_security_group_ids" {
  description = "List of security group IDs allowed to access the cache (typically ECS cluster security group)"
  type        = list(string)
  default     = []
}

variable "description" {
  description = "Description for the Valkey serverless cache"
  type        = string
  default     = "Helicone Valkey serverless cache"
} 