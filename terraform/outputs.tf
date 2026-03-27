output "relay_fra_ip" {
  value       = module.relay_fra.public_ip
  description = "Frankfurt relay public IP"
}

output "relay_fra_url" {
  value       = module.relay_fra.relay_url
  description = "Frankfurt relay URL"
}

output "relay_isr_ip" {
  value       = module.relay_isr.public_ip
  description = "Tel Aviv relay public IP"
}

output "relay_isr_url" {
  value       = module.relay_isr.relay_url
  description = "Tel Aviv relay URL"
}

output "relay_mum_ip" {
  value       = module.relay_mum.public_ip
  description = "Mumbai relay public IP"
}

output "relay_mum_url" {
  value       = module.relay_mum.relay_url
  description = "Mumbai relay URL"
}
