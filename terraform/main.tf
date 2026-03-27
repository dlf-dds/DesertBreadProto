locals {
  common_tags = {
    Project     = var.project
    Environment = var.environment
    Owner       = var.owner
    ManagedBy   = "terraform"
  }
}

# --- Provider Configuration ---

provider "aws" {
  region = "eu-central-1"
  default_tags { tags = local.common_tags }
}

provider "aws" {
  alias  = "il"
  region = "il-central-1"
  default_tags { tags = local.common_tags }
}

provider "aws" {
  alias  = "ap"
  region = "ap-south-1"
  default_tags { tags = local.common_tags }
}

provider "cloudflare" {
  # API token from CLOUDFLARE_API_TOKEN env var
}

# --- DNS Zone Data ---

data "cloudflare_zone" "main" {
  name = var.cloudflare_zone
}

# --- iroh Relay Servers ---

module "relay_fra" {
  source = "./modules/relay"

  relay_name = "fra"
  region     = "eu-central-1"
  zone_id    = data.cloudflare_zone.main.id
  dns_name   = "relay"
  domain     = var.cloudflare_zone
  tags       = local.common_tags

  providers = {
    aws = aws
  }
}

module "relay_isr" {
  source = "./modules/relay"

  relay_name = "isr"
  region     = "il-central-1"
  zone_id    = data.cloudflare_zone.main.id
  dns_name   = "relay-isr"
  domain     = var.cloudflare_zone
  tags       = local.common_tags

  providers = {
    aws = aws.il
  }
}

module "relay_mum" {
  source = "./modules/relay"

  relay_name = "mum"
  region     = "ap-south-1"
  zone_id    = data.cloudflare_zone.main.id
  dns_name   = "relay-mum"
  domain     = var.cloudflare_zone
  tags       = local.common_tags

  providers = {
    aws = aws.ap
  }
}
