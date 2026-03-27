variable "project" {
  description = "Project name for resource tagging"
  type        = string
  default     = "desert-bread-proto"
}

variable "environment" {
  description = "Deployment environment"
  type        = string
  default     = "dev"
}

variable "owner" {
  description = "Owner email for resource tagging"
  type        = string
  default     = "dedd.flanders@gmail.com"
}

variable "cloudflare_zone" {
  description = "Cloudflare DNS zone"
  type        = string
  default     = "desertbread.net"
}

variable "cloudflare_account_id" {
  description = "Cloudflare account ID"
  type        = string
  default     = "507423f1d79e8924f0b2bdaf2711db1b"
}
