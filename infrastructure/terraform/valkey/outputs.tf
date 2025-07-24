output "valkey_cache_name" {
  description = "Name of the Valkey serverless cache"
  value       = aws_elasticache_serverless_cache.valkey.name
}

output "valkey_cache_arn" {
  description = "ARN of the Valkey serverless cache"
  value       = aws_elasticache_serverless_cache.valkey.arn
}

output "valkey_cache_endpoint" {
  description = "Endpoint URL of the Valkey serverless cache"
  value       = aws_elasticache_serverless_cache.valkey.endpoint[0].address
  sensitive   = true
}

output "valkey_cache_port" {
  description = "Port of the Valkey serverless cache"
  value       = aws_elasticache_serverless_cache.valkey.endpoint[0].port
}

output "valkey_cache_reader_endpoint" {
  description = "Reader endpoint URL of the Valkey serverless cache"
  value       = length(aws_elasticache_serverless_cache.valkey.reader_endpoint) > 0 ? aws_elasticache_serverless_cache.valkey.reader_endpoint[0].address : null
  sensitive   = true
}

output "valkey_cache_reader_port" {
  description = "Reader port of the Valkey serverless cache"
  value       = length(aws_elasticache_serverless_cache.valkey.reader_endpoint) > 0 ? aws_elasticache_serverless_cache.valkey.reader_endpoint[0].port : null
}

output "valkey_cache_engine" {
  description = "Engine used by the cache"
  value       = aws_elasticache_serverless_cache.valkey.engine
}

output "valkey_cache_engine_version" {
  description = "Engine version of the cache"
  value       = aws_elasticache_serverless_cache.valkey.full_engine_version
}

output "valkey_cache_status" {
  description = "Status of the Valkey serverless cache"
  value       = aws_elasticache_serverless_cache.valkey.status
}

output "valkey_security_group_id" {
  description = "ID of the security group for the Valkey cache"
  value       = aws_security_group.valkey_sg.id
}

output "valkey_subnet_group_name" {
  description = "Name of the subnet group for the Valkey cache"
  value       = var.create_subnet_group ? aws_elasticache_subnet_group.valkey_subnet_group[0].name : null
}

output "valkey_cache_configuration" {
  description = "Cache configuration details"
  value = {
    max_storage_gb      = var.max_storage_gb
    max_ecpu_per_second = var.max_ecpu_per_second
    daily_snapshot_time = var.daily_snapshot_time
    snapshot_retention  = var.snapshot_retention_limit
    engine              = var.engine
    major_version       = var.major_engine_version
  }
}

output "connection_string" {
  description = "Connection string for the Valkey cache (format: host:port)"
  value       = "${aws_elasticache_serverless_cache.valkey.endpoint[0].address}:${aws_elasticache_serverless_cache.valkey.endpoint[0].port}"
  sensitive   = true
}