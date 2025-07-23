# Cloudflare DNS Module

This module manages Cloudflare DNS records for the Helicone application, including:

- DNS records pointing to the EKS load balancer
- ACM certificate validation records
- Root domain A records (optional)

## Dependencies

This module depends on outputs from the EKS module. It uses Terraform remote state to access:

- Load balancer hostname from the ingress service
- ACM certificate validation options
- Other EKS-related outputs

## Setup

1. **Copy and configure variables:**

   ```bash
   cp terraform.tfvars.example terraform.tfvars
   ```

2. **Edit `terraform.tfvars`** with your actual values:

   - Set your Cloudflare API token
   - Configure domain names and subdomains
   - Adjust remote state configuration if not using local backend

3. **Configure remote state backend** (if not using local):
   - For S3 backend: Update the backend configuration in `main.tf`
   - For Terraform Cloud: Update the backend configuration accordingly
   - Update `remote_state_config` variable in `terraform.tfvars`

## Remote State Configuration

### Local Backend (Default)

The module is configured to read from the EKS module's local state file:

```
eks_terraform_state_path = "../eks/terraform.tfstate"
remote_state_backend = "local"
```

### S3 Backend

If your EKS module uses S3 backend, update your `terraform.tfvars`:

```hcl
remote_state_backend = "s3"
remote_state_config = {
  bucket = "your-terraform-state-bucket"
  key    = "eks/terraform.tfstate"
  region = "us-west-2"
}
```

And update the `data.terraform_remote_state.eks` block in `main.tf`:

```hcl
data "terraform_remote_state" "eks" {
  backend = "s3"

  config = {
    bucket = var.remote_state_config.bucket
    key    = var.remote_state_config.key
    region = var.remote_state_config.region
  }
}
```

## Usage

1. **Initialize Terraform:**

   ```bash
   terraform init
   ```

2. **Plan the deployment:**

   ```bash
   terraform plan
   ```

3. **Apply the configuration:**
   ```bash
   terraform apply
   ```

## Prerequisites

- EKS module must be deployed first
- Cloudflare API token with DNS edit permissions
- Cloudflare zones for your domains must exist

**Important:** If you don't have access to one or both Cloudflare zones, you can disable them:

- Set `enable_helicone_test_domain = false` if you don't have access to helicone-test.com
- Set `enable_helicone_ai_domain = false` if you don't have access to helicone.ai

## Working with Existing DNS Records

If you already have DNS records created manually, you can use this module to read them instead of
creating new ones:

```hcl
# Set this to true to read existing DNS records
use_existing_dns_records = true
```

When `use_existing_dns_records = true`:

- The module will use data sources to read existing DNS records
- No new DNS records will be created
- Outputs will still provide the DNS record information
- This is useful when DNS records were created manually but you want Terraform to know about them

## Resources Created

### For helicone-test.com (if enabled):

- CNAME record for subdomain pointing to load balancer
- DNS validation records for ACM certificate
- Optional root domain A records

### For helicone.ai:

- CNAME record for subdomain pointing to load balancer
- DNS validation records for ACM certificate
- Optional root domain A records

## Variables

| Variable                                | Description                        | Default               | Required |
| --------------------------------------- | ---------------------------------- | --------------------- | -------- |
| `cloudflare_api_token`                  | Cloudflare API token               | -                     | Yes      |
| `enable_helicone_test_domain`           | Enable helicone-test.com resources | `false`               | No       |
| `enable_helicone_ai_domain`             | Enable helicone.ai resources       | `true`                | No       |
| `use_existing_dns_records`              | Read existing DNS records          | `false`               | No       |
| `cloudflare_zone_name`                  | Cloudflare zone name               | `"helicone-test.com"` | No       |
| `cloudflare_subdomain`                  | Application subdomain              | `"filevine"`          | No       |
| `create_root_domain_record`             | Create root domain record          | `false`               | No       |
| `cloudflare_helicone_ai_zone_name`      | helicone.ai zone name              | `"helicone.ai"`       | No       |
| `cloudflare_helicone_ai_subdomain`      | helicone.ai subdomain              | `"filevine"`          | No       |
| `create_helicone_ai_root_domain_record` | Create helicone.ai root record     | `false`               | No       |

## Outputs

- `application_url` - Full URL for the test application
- `helicone_ai_application_url` - Full URL for the helicone.ai application
- `cloudflare_zone_id` - Cloudflare zone ID for helicone-test.com
- `cloudflare_helicone_ai_zone_id` - Cloudflare zone ID for helicone.ai
- Various DNS record details

## Troubleshooting

1. **Remote state not found**: Ensure the EKS module has been applied and the state path is correct
2. **Missing outputs**: Verify the EKS module exports the required outputs
3. **Cloudflare API errors**: Check your API token permissions and zone ownership
4. **DNS resolution**: Wait for DNS propagation after applying changes
