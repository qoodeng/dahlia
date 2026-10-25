#![deny(clippy::undocumented_unsafe_blocks)]

// Keep the existing binary test/benchmark commands on the private application tree.
#[cfg(test)]
include!("application.rs");

fn main() {
    #[cfg(not(test))]
    dahlia::run();
}
