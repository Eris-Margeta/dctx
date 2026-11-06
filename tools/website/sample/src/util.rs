/// The greeting shown on the kiosk screen.
pub fn greet(name: &str) -> String {
    format!("Hello, {}!", title_case(name))
}

/// Upper-case the first letter of a name, leave the rest as typed.
pub fn title_case(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}
