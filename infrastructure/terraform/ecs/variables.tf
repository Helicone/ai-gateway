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