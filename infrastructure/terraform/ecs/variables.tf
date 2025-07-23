variable "environment" {
  type    = string
  default = "dev"
}

variable "gw_version" {
  type    = string
  default = "latest"
}

variable "region" {
  type    = string
  default = "us-east-1"
}

variable "certificate_domain" {
  description = "The domain name for the ACM certificate (e.g., *.example.com)"
  type        = string
  default     = "heliconetest.com"
}

variable "cpu" {
  description = "Amount of CPU resources"
  type        = string
  default     = "1024"
}

variable "memory" {
  description = "Amount of memory"
  type        = string
  default     = "2048"
}

variable "desired_count" {
  description = "Number of service instances"
  type        = number
  default     = 1
}