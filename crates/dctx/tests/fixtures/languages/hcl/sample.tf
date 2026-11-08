variable "café" {
  type = string
  default = "fixture"
}
resource "example_box" "sample" {
  value = var.café
  nested {
    enabled = true
  }
}
