use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "images/"]
pub struct Images;

#[derive(RustEmbed)]
#[folder = "lib/"]
pub struct LibData;

#[derive(RustEmbed)]
#[folder = "Notes/"]
pub struct Builds;
